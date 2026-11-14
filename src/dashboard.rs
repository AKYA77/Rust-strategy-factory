//! Loopback-only dashboard. Official metrics are read-only; browser controls require session CSRF.
use crate::{Result, research, storage::Registry};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as Param, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
struct AuthFile {
    version: u8,
    password_hash: String,
}
fn random_hex() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| "OS randomness unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn initialize_auth(path: &Path) -> Result<()> {
    let mut password = String::new();
    std::io::stdin()
        .take(4096)
        .read_to_string(&mut password)
        .map_err(|e| e.to_string())?;
    let password = password.trim_end_matches(['\r', '\n']);
    if password.len() < 16 || password.len() > 1024 {
        return Err("password from stdin must contain 16–1024 bytes".into());
    }
    let mut salt = [0; 16];
    getrandom::fill(&mut salt).map_err(|_| "OS randomness unavailable")?;
    let salt = SaltString::encode_b64(&salt).map_err(|_| "salt encoding failed")?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| "password hashing failed")?
        .to_string();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(
        &serde_json::to_vec(&AuthFile {
            version: 1,
            password_hash: hash,
        })
        .map_err(|e| e.to_string())?,
    )
    .and_then(|()| file.sync_all())
    .map_err(|e| e.to_string())
}
#[derive(Clone)]
struct Session {
    csrf: String,
    expires: Instant,
}
struct App {
    registry: Mutex<Registry>,
    catalog_path: Option<PathBuf>,
    review_config: Option<ReviewConfig>,
    review_slots: Arc<tokio::sync::Semaphore>,
    review_bucket: Mutex<(Instant, u32)>,
    password_hash: String,
    sessions: Mutex<HashMap<String, Session>>,
    login_bucket: Mutex<(Instant, u32)>,
    requests: Mutex<(Instant, u32)>,
    login_slots: Arc<tokio::sync::Semaphore>,
    port: u16,
}
fn api_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({"error":message}))).into_response()
}
fn session(app: &App, headers: &HeaderMap) -> Option<Session> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    let sid = cookie
        .split(';')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("factory_sid="))?;
    if sid.len() != 64 || !sid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let key = blake3::hash(sid.as_bytes()).to_hex().to_string();
    let mut sessions = app.sessions.lock().ok()?;
    sessions.retain(|_, s| s.expires > Instant::now());
    sessions.get(&key).cloned()
}
fn bucket(bucket: &Mutex<(Instant, u32)>, limit: u32) -> bool {
    let Ok(mut b) = bucket.lock() else {
        return false;
    };
    if b.0.elapsed() >= Duration::from_secs(60) {
        *b = (Instant::now(), 0);
    }
    if b.1 >= limit {
        return false;
    }
    b.1 += 1;
    true
}
async fn guard(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let allowed = [
        format!("127.0.0.1:{}", app.port),
        format!("localhost:{}", app.port),
    ];
    if !headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| allowed.iter().any(|v| v == h))
    {
        return api_error(StatusCode::FORBIDDEN, "unrecognized host");
    }
    if let Some(origin) = headers.get(header::ORIGIN)
        && !origin
            .to_str()
            .ok()
            .is_some_and(|o| allowed.iter().any(|h| o == format!("http://{h}")))
    {
        return api_error(StatusCode::FORBIDDEN, "cross-origin request denied");
    }
    let path = request.uri().path();
    if path.starts_with("/api/") && path != "/api/login" && path != "/api/health" {
        let Some(s) = session(&app, headers) else {
            return api_error(StatusCode::UNAUTHORIZED, "sign in required");
        };
        if !bucket(&app.requests, 240) {
            return api_error(StatusCode::TOO_MANY_REQUESTS, "request rate limit");
        }
        if request.method() != axum::http::Method::GET {
            let csrf = headers
                .get("x-csrf-token")
                .map(|v| v.as_bytes())
                .unwrap_or_default();
            if !constant_time_eq::constant_time_eq(csrf, s.csrf.as_bytes()) {
                return api_error(StatusCode::FORBIDDEN, "invalid request token");
            }
        }
    }
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert(header::CONTENT_SECURITY_POLICY,"default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("x-frame-options", "DENY".parse().unwrap());
    response
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    password: String,
}
async fn login(State(app): State<Arc<App>>, Json(body): Json<Login>) -> Response {
    if body.password.len() > 1024 || !bucket(&app.login_bucket, 8) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "login rate limit");
    }
    let Ok(permit) = app.login_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "login busy");
    };
    let hash = app.password_hash.clone();
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(body.password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !verified {
        return api_error(StatusCode::UNAUTHORIZED, "invalid password");
    }
    let (Ok(sid), Ok(csrf)) = (random_hex(), random_hex()) else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "session unavailable");
    };
    let Ok(mut sessions) = app.sessions.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "session unavailable");
    };
    sessions.retain(|_, s| s.expires > Instant::now());
    if sessions.len() >= 64 {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "session capacity");
    }
    sessions.insert(
        blake3::hash(sid.as_bytes()).to_hex().to_string(),
        Session {
            csrf,
            expires: Instant::now() + Duration::from_secs(8 * 3600),
        },
    );
    let mut r = Json(serde_json::json!({"ok":true})).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        format!("factory_sid={sid}; HttpOnly; SameSite=Strict; Path=/; Max-Age=28800")
            .parse()
            .unwrap(),
    );
    r
}
async fn root(State(app): State<Arc<App>>, headers: HeaderMap) -> Html<&'static str> {
    Html(if session(&app, &headers).is_some() {
        include_str!("../web/dashboard.html")
    } else {
        include_str!("../web/login.html")
    })
}
async fn session_info(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(s) = session(&app, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "sign in required");
    };
    Json(serde_json::json!({"csrf":s.csrf})).into_response()
}
async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if let Some(cookie) = headers.get(header::COOKIE).and_then(|h| h.to_str().ok())
        && let Some(sid) = cookie
            .split(';')
            .map(str::trim)
            .find_map(|p| p.strip_prefix("factory_sid="))
        && let Ok(mut sessions) = app.sessions.lock()
    {
        sessions.remove(&blake3::hash(sid.as_bytes()).to_hex().to_string());
    }
    let mut r = Json(serde_json::json!({"ok":true})).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        "factory_sid=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
            .parse()
            .unwrap(),
    );
    r
}
// Only persistent failure markers: absence does not certify audit coverage or correctness.
fn audit_health(registry: &Registry) -> Result<serde_json::Value> {
    let (proof, indexed): (bool, bool) = registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE action='proof-audit-contradiction-v1'), EXISTS(SELECT 1 FROM events WHERE action='indexed-audit-contradiction-v1')",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"proof_blocked":proof,"indexed_blocked":indexed,"scope":"registry-local persisted failure markers; absence does not establish audit coverage or qualification"}),
    )
}
async fn overview(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let load = || -> Result<serde_json::Value> {
        let mut query = registry
            .connection
            .prepare("SELECT id FROM research_runs ORDER BY rowid DESC LIMIT 100")
            .map_err(|e| e.to_string())?;
        let ids = query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let runs = ids
            .map(|id| {
                research::status(
                    &registry,
                    &id.map_err(|e| e.to_string())?,
                    chrono::Utc::now().timestamp_millis(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(
            serde_json::json!({"software_status":"SOFTWARE_INCOMPLETE","historical_readiness":"FULL_HISTORICAL_DATA_PENDING","qualification":"Independent validation/profile/coverage evidence is incomplete; no qualified strategies","audit_health":audit_health(&registry)?,"counts":registry.counts()?,"runs":runs,"engine":crate::ENGINE_VERSION,"indicators":crate::INDICATOR_VERSION,"compute_revision":research::compute_revision()?}),
        )
    };
    match load() {
        Ok(v) => Json(v).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read persisted overview",
        ),
    }
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogQuery {
    after: Option<String>,
    limit: Option<usize>,
}
fn catalog_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
async fn catalog_page(State(app): State<Arc<App>>, Query(query): Query<CatalogQuery>) -> Response {
    let limit = query.limit.unwrap_or(25);
    if limit == 0 || limit > 100 || query.after.as_deref().is_some_and(|id| !catalog_id(id)) {
        return api_error(StatusCode::BAD_REQUEST, "invalid catalog cursor or limit");
    }
    let Some(path) = &app.catalog_path else {
        return Json(serde_json::json!({"configured":false,"entries":[],"next_cursor":null,"limitations":"No dataset catalog configured. Experiment-page provenance is separate."})).into_response();
    };
    match crate::catalog::Catalog::existing(path).and_then(|c|c.page(query.after.as_deref(),limit)) {
        Ok(page)=>Json(serde_json::json!({"configured":true,"entries":page.entries,"next_cursor":page.next_cursor,"limitations":page.limitations})).into_response(),
        Err(_)=>api_error(StatusCode::INTERNAL_SERVER_ERROR,"unable to read verified catalog observations"),
    }
}
async fn catalog_detail(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid catalog identity");
    }
    let Some(path) = &app.catalog_path else {
        return api_error(StatusCode::NOT_FOUND, "dataset catalog not configured");
    };
    match crate::catalog::Catalog::existing(path).and_then(|c| c.entry(&id)) {
        Ok(Some(entry)) => Json(entry).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "catalog observation not found"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read verified catalog observation",
        ),
    }
}
// Operator-owned artifact selection; requests never choose local paths.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    authority: PathBuf,
    artifacts: Vec<ReviewArtifact>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewArtifact {
    machine_id: String,
    archive: PathBuf,
    dataset: PathBuf,
}
impl ReviewConfig {
    fn load(path: &Path) -> Result<Self> {
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 128 * 1024 {
            return Err("review configuration exceeds128KiB".into());
        }
        let body = fs::read(path).map_err(|e| e.to_string())?;
        if body.len() > 128 * 1024 {
            return Err("review configuration exceeds128KiB".into());
        }
        let mut cfg: Self =
            serde_json::from_slice(&body).map_err(|_| "invalid review configuration")?;
        if cfg.artifacts.is_empty() || cfg.artifacts.len() > 64 {
            return Err("review configuration requires1..64 artifacts".into());
        }
        let base = path.parent().unwrap_or(Path::new("."));
        cfg.authority = base.join(cfg.authority);
        let mut seen = std::collections::HashSet::new();
        for a in &mut cfg.artifacts {
            if !catalog_id(&a.machine_id)
                || !seen.insert(a.machine_id.clone())
                || a.archive.as_os_str().is_empty()
                || a.dataset.as_os_str().is_empty()
            {
                return Err("invalid or duplicate configured review artifact".into());
            }
            a.archive = base.join(&a.archive);
            a.dataset = base.join(&a.dataset);
        }
        crate::review_authority::Authority::load(&cfg.authority)?;
        Ok(cfg)
    }
}
fn review_dataset(path: &Path) -> Result<crate::data::Dataset> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rsf") => crate::native::read(path),
        Some("rsfz") => crate::compressed::read(path),
        Some("parquet") => crate::parquet_store::read(path),
        _ => {
            let f = fs::File::open(path).map_err(|e| e.to_string())?;
            if f.metadata().map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
                return Err("review dataset exceeds128MiB".into());
            }
            serde_json::from_reader(std::io::BufReader::new(f)).map_err(|e| e.to_string())
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewQuery {
    before: Option<String>,
    limit: Option<usize>,
}
async fn data_review_state(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Query(query): Query<ReviewQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(25);
    let before = match query.before {
        Some(s) => match s.parse::<i64>() {
            Ok(n) if n > 0 && n.to_string() == s => Some(n),
            _ => return api_error(StatusCode::BAD_REQUEST, "invalid review cursor"),
        },
        None => None,
    };
    if !catalog_id(&id) || limit == 0 || limit > 25 {
        return api_error(StatusCode::BAD_REQUEST, "invalid review identity/limit");
    }
    let (Some(config), Some(catalog)) = (&app.review_config, &app.catalog_path) else {
        return Json(serde_json::json!({"configured":false,"message":"Independent review workflow is not configured."})).into_response();
    };
    let config = config.clone();
    let catalog = catalog.clone();
    let Ok(permit) = app.review_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "review work busy");
    };
    let result=tokio::task::spawn_blocking(move||->Result<serde_json::Value>{
        let _permit=permit;
        let a=crate::review_authority::Authority::load(&config.authority)?;
        let c=crate::catalog::Catalog::existing(&catalog)?;
        let state=c.review_state(&id,&a)?;
        let mut events=state.events.iter().rev().filter(|e|before.is_none_or(|b|e.record.sequence<b)).take(limit+1).collect::<Vec<_>>();
        let more=events.len()>limit;events.truncate(limit);
        let next=if more{events.last().map(|e|e.record.sequence.to_string())}else{None};
        let mut admissions=Vec::new();
        for artifact in &config.artifacts {
            let r=c.machine(&artifact.machine_id)?.ok_or("configured machine admission missing")?;
            if r.record.binding.catalog_identity==id {admissions.push(serde_json::json!({"id":r.id,"hash":r.hash,"verified_at":r.record.verified_at,"archive_hash":r.record.binding.archive_hash,"scope":r.record.binding.calendar.scope}));}
        }
        Ok(serde_json::json!({"configured":true,"catalog_identity":id,"head":state.head,"events":events,"next_cursor":next,"current_authority_authenticated":state.current_authority_authenticated,"admissions":admissions,"limitations":state.limitations}))
    }).await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "review history/configuration integrity failed",
        ),
    }
}
async fn live_eligibility(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid machine identity");
    }
    let (Some(config), Some(catalog)) = (&app.review_config, &app.catalog_path) else {
        return api_error(
            StatusCode::NOT_FOUND,
            "independent review workflow not configured",
        );
    };
    let Some(artifact) = config
        .artifacts
        .iter()
        .find(|a| a.machine_id == id)
        .cloned()
    else {
        return api_error(
            StatusCode::NOT_FOUND,
            "artifact is not configured for review",
        );
    };
    let authority = config.authority.clone();
    let catalog = catalog.clone();
    let Ok(permit) = app.review_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "review work busy");
    };
    let result =
        tokio::task::spawn_blocking(move || -> Result<crate::data_approval::Eligibility> {
            let _permit = permit;
            let a = crate::review_authority::Authority::load(&authority)?;
            let c = crate::catalog::Catalog::existing(&catalog)?;
            c.data_eligibility(
                &a,
                &id,
                &artifact.archive,
                &review_dataset(&artifact.dataset)?,
            )
        })
        .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "live artifact verification unavailable; no eligibility grant",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewDecision {
    reviewer: String,
    password: String,
    request: crate::data_approval::Request,
}
async fn data_review_decide(
    State(app): State<Arc<App>>,
    Json(body): Json<ReviewDecision>,
) -> Response {
    let (Some(config), Some(catalog)) = (&app.review_config, &app.catalog_path) else {
        return api_error(
            StatusCode::NOT_FOUND,
            "independent review workflow not configured",
        );
    };
    if body.reviewer.len() > 128
        || body.password.len() > 1024
        || !config
            .artifacts
            .iter()
            .any(|a| a.machine_id == body.request.machine_id)
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid or unconfigured review request",
        );
    }
    if !bucket(&app.review_bucket, 8) {
        return api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "review authentication rate limit",
        );
    }
    let Ok(permit) = app.review_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "review work busy");
    };
    let authority = config.authority.clone();
    let catalog = catalog.clone();
    let owner_hash = app.password_hash.clone();
    let result = tokio::task::spawn_blocking(
        move || -> std::result::Result<crate::data_approval::Event, bool> {
            let _permit = permit;
            let a = crate::review_authority::Authority::load(&authority).map_err(|_| false)?;
            // Reusing the dashboard owner's credential is not independent review.
            if PasswordHash::new(&owner_hash).ok().is_some_and(|h| {
                Argon2::default()
                    .verify_password(body.password.as_bytes(), &h)
                    .is_ok()
            }) {
                return Err(true);
            }
            let p = a
                .authenticate(&body.reviewer, &body.password)
                .map_err(|_| true)?;
            let mut c = crate::catalog::Catalog::open(&catalog).map_err(|_| false)?;
            c.decide(&a, &p, body.request).map_err(|_| false)
        },
    )
    .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(true)) => api_error(
            StatusCode::FORBIDDEN,
            "independent reviewer credentials refused",
        ),
        _ => api_error(
            StatusCode::CONFLICT,
            "review refused; inspect scope, synthetic/profile limits and latest history",
        ),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    before: Option<String>,
    instrument: Option<String>,
    limit: Option<usize>,
}
#[derive(Serialize)]
struct HistoryPage {
    rows: Vec<serde_json::Value>,
    next_cursor: Option<String>,
}
fn experiment_page(
    registry: &Registry,
    before: Option<i64>,
    instrument: Option<&str>,
    limit: usize,
) -> Result<HistoryPage> {
    let mut sql = String::from(
        "SELECT rowid,id,state,key_json,json_extract(result_json,'$.data_manifest'),json_extract(result_json,'$.strategy'),json_array_length(result_json,'$.tick_replay.trades'),CASE WHEN typeof(json_extract(result_json,'$.tick_replay.realized_micro_usd'))='integer' AND json_extract(result_json,'$.tick_replay.realized_micro_usd') BETWEEN -9007199254740991 AND 9007199254740991 THEN json_extract(result_json,'$.tick_replay.realized_micro_usd') ELSE NULL END FROM experiments WHERE 1=1",
    );
    let mut parameters = Vec::<rusqlite::types::Value>::new();
    if let Some(before) = before {
        sql.push_str(" AND rowid < ?");
        parameters.push(before.into());
    }
    if let Some(instrument) = instrument {
        sql.push_str(" AND json_extract(key_json,'$.instrument') = ?");
        parameters.push(instrument.to_string().into());
    }
    sql.push_str(" ORDER BY rowid DESC LIMIT ?");
    parameters.push(((limit + 1) as i64).into());
    let mut query = registry
        .connection
        .prepare(&sql)
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map(rusqlite::params_from_iter(parameters), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut records = Vec::new();
    for row in rows {
        let (cursor, id, state, key, manifest, strategy, trades, equity) =
            row.map_err(|e| e.to_string())?;
        records.push((
            cursor,
            serde_json::json!({"id":id,"state":state,
            "key":serde_json::from_str::<serde_json::Value>(&key).map_err(|e|e.to_string())?,
            "manifest":manifest.and_then(|v|serde_json::from_str::<serde_json::Value>(&v).ok()),
            "strategy":strategy.and_then(|v|serde_json::from_str::<serde_json::Value>(&v).ok()),
            "closed_trades":trades,"ending_micro_usd":equity}),
        ));
    }
    let more = records.len() > limit;
    records.truncate(limit);
    // Decimal strings keep SQLite's full i64 rowid range exact in browser cursors.
    let next_cursor = more.then(|| {
        records
            .last()
            .expect("nonempty bounded history page")
            .0
            .to_string()
    });
    Ok(HistoryPage {
        rows: records.into_iter().map(|(_, row)| row).collect(),
        next_cursor,
    })
}
async fn history(State(app): State<Arc<App>>, Query(query): Query<HistoryQuery>) -> Response {
    let limit = query.limit.unwrap_or(25);
    let before = match query.before {
        None => None,
        Some(value) => match value.parse::<i64>() {
            Ok(n) if n > 0 && value.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => return api_error(StatusCode::BAD_REQUEST, "invalid history cursor"),
        },
    };
    if !(1..=100).contains(&limit)
        || query
            .instrument
            .as_ref()
            .is_some_and(|s| !crate::contracts::SYMBOLS.contains(&s.as_str()))
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid history instrument or page size",
        );
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match experiment_page(&registry, before, query.instrument.as_deref(), limit) {
        Ok(page) => Json(page).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read persisted experiment history",
        ),
    }
}
async fn experiments(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match experiment_page(&registry, None, None, 100) {
        Ok(page) => Json(page.rows).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read persisted experiments",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrainingQuery {
    parent: String,
    before: Option<String>,
    limit: Option<usize>,
}
fn valid_identity(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn training_record(
    registry: &Registry,
    id: &str,
    reviews: bool,
) -> Result<Option<serde_json::Value>> {
    if reviews {
        crate::training_review::read_report(registry, id)?.map(serde_json::to_value)
    } else {
        crate::improvement::read_report(registry, id)?.map(serde_json::to_value)
    }
    .transpose()
    .map_err(|e| e.to_string())
}
fn training_page(
    registry: &Registry,
    parent: &str,
    before: Option<i64>,
    limit: usize,
    reviews: bool,
) -> Result<HistoryPage> {
    let (table, field) = if reviews {
        ("training_reviews", "diagnosis_id")
    } else {
        ("training_diagnoses", "parent_id")
    };
    let mut sql = format!("SELECT rowid,id FROM {table} WHERE {field}=?");
    let mut values = vec![rusqlite::types::Value::from(parent.to_string())];
    if let Some(before) = before {
        sql.push_str(" AND rowid < ?");
        values.push(before.into());
    }
    sql.push_str(" ORDER BY rowid DESC LIMIT ?");
    values.push(((limit + 1) as i64).into());
    let mut query = registry
        .connection
        .prepare(&sql)
        .map_err(|e| e.to_string())?;
    let mut ids = query
        .query_map(rusqlite::params_from_iter(values), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let more = ids.len() > limit;
    ids.truncate(limit);
    let next_cursor = if more {
        ids.last().map(|(cursor, _)| cursor.to_string())
    } else {
        None
    };
    let mut rows = Vec::new();
    for (_, id) in ids {
        let report = training_record(registry, &id, reviews)?.ok_or("training record missing")?;
        let children = report[if reviews { "children" } else { "proposals" }]
            .as_array()
            .ok_or("training child evidence missing")?
            .len();
        rows.push(serde_json::json!({"id":id,"version":report["version"],"status":report["status"],"children":children}));
    }
    Ok(HistoryPage { rows, next_cursor })
}
async fn training_list(app: Arc<App>, query: TrainingQuery, reviews: bool) -> Response {
    let before = match query.before {
        None => None,
        Some(value) => match value.parse::<i64>() {
            Ok(n) if n > 0 && value.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => return api_error(StatusCode::BAD_REQUEST, "invalid training cursor"),
        },
    };
    let limit = query.limit.unwrap_or(10);
    if !valid_identity(&query.parent) || !(1..=25).contains(&limit) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid training identity or page size",
        );
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match training_page(&registry, &query.parent, before, limit, reviews) {
        Ok(page) => Json(page).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "training evidence integrity/read failure",
        ),
    }
}
async fn diagnoses(State(app): State<Arc<App>>, Query(query): Query<TrainingQuery>) -> Response {
    training_list(app, query, false).await
}
async fn reviews(State(app): State<Arc<App>>, Query(query): Query<TrainingQuery>) -> Response {
    training_list(app, query, true).await
}
async fn training_detail(app: Arc<App>, id: String, reviews: bool) -> Response {
    if !valid_identity(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid training identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match training_record(&registry, &id, reviews) {
        Ok(Some(report)) => Json(report).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "training record not found"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "training evidence integrity/read failure",
        ),
    }
}
async fn diagnosis_detail(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    training_detail(app, id, false).await
}
async fn review_detail(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    training_detail(app, id, true).await
}
async fn detail(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return api_error(StatusCode::BAD_REQUEST, "invalid experiment identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match registry.result(&id) {
        Ok(Some(v)) => {
            let assessment = (|| -> Result<crate::validation::Assessment> {
                if v.result["proof_rejection"] == true && v.result["tick_replay"].is_null() {
                    return Err(
                        "entry infeasibility proven; no tick trade series was simulated".into(),
                    );
                }
                if v.result["tick_replay"]["version"].as_str() != Some(crate::ENGINE_VERSION) {
                    return Err("engine version requires fresh verification".into());
                }
                let cutoff = match v.result["tick_replay"]["evaluation_window"]["end_exclusive_ms"]
                    .as_i64()
                {
                    Some(end) => end.checked_sub(1).ok_or("cutoff overflow")?,
                    None => v.result["data_manifest"]["last_ms"]
                        .as_i64()
                        .ok_or("dataset cutoff missing")?,
                };
                let observations: Vec<crate::validation::Observation> =
                    serde_json::from_value(v.result["tick_replay"]["trades"].clone())
                        .map_err(|_| "no complete tick trade series")?;
                let unresolved = usize::from(!v.result["tick_replay"]["unresolved"].is_null());
                crate::validation::assess(&observations, cutoff, unresolved)
            })();
            let (assessment, error) = match assessment {
                Ok(a) => (Some(a), None),
                Err(e) => (None, Some(e)),
            };
            // Decimal strings preserve u64 event identity in the visible trade page.
            // The immutable result/export keeps its original exact numeric values.
            let trade_event_ids=v.result["tick_replay"]["trades"].as_array().into_iter().flatten().take(500).map(|trade|{
                serde_json::json!({"entry":trade["entry_event"].as_u64().map(|n|n.to_string()),"exit":trade["exit_event"].as_u64().map(|n|n.to_string())})
            }).collect::<Vec<_>>();
            Json(serde_json::json!({"outcome":v.outcome,"result":v.result,"result_hash":v.result_hash,"trade_event_ids":trade_event_ids,"numeric_training_assessment":assessment,"assessment_unavailable_reason":error})).into_response()
        }
        Ok(None) => api_error(StatusCode::NOT_FOUND, "result not completed"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "result integrity check failed",
        ),
    }
}
async fn protocols(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let load = || -> Result<Vec<serde_json::Value>> {
        let mut query=registry.connection.prepare("SELECT id,report_json IS NOT NULL FROM scientific_protocols ORDER BY rowid DESC LIMIT 100").map_err(|e|e.to_string())?;
        query.query_map([],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"report_ready":r.get::<_,bool>(1)?}))).map_err(|e|e.to_string())?.map(|r|r.map_err(|e|e.to_string())).collect()
    };
    match load() {
        Ok(v) => Json(v).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read protocol records",
        ),
    }
}
async fn protocol_detail(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return api_error(StatusCode::BAD_REQUEST, "invalid protocol identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::science::read_report(&registry, &id) {
        Ok(Some(r)) => Json(r).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "protocol report is not complete"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "scientific report integrity failure",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    state: String,
}
async fn control(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<Control>,
) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match research::control(&registry, &id, &body.state) {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(_) => api_error(StatusCode::BAD_REQUEST, "invalid run or control state"),
    }
}
pub async fn serve(database: &Path, auth_path: &Path, port: u16) -> Result<()> {
    serve_with_catalog(database, auth_path, port, None).await
}
pub async fn serve_with_catalog(
    database: &Path,
    auth_path: &Path,
    port: u16,
    catalog: Option<&Path>,
) -> Result<()> {
    serve_with_reviews(database, auth_path, port, catalog, None).await
}
pub async fn serve_with_reviews(
    database: &Path,
    auth_path: &Path,
    port: u16,
    catalog: Option<&Path>,
    review_path: Option<&Path>,
) -> Result<()> {
    let review_config = review_path.map(ReviewConfig::load).transpose()?;
    if review_config.is_some() && catalog.is_none() {
        return Err("review workflow requires a configured catalog".into());
    }
    if port != 0 && port < 1024 {
        return Err("use an unprivileged loopback port or zero for allocation".into());
    }
    if !database.is_file() {
        return Err("registry must already exist".into());
    }
    if let Some(path) = catalog {
        crate::catalog::Catalog::existing(path)?.page(None, 1)?;
    }
    let metadata = fs::metadata(auth_path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("authentication file must have owner-only permissions (0600)".into());
        }
    }
    if metadata.len() > 16384 {
        return Err("authentication file too large".into());
    }
    let auth: AuthFile = serde_json::from_slice(&fs::read(auth_path).map_err(|e| e.to_string())?)
        .map_err(|_| "invalid authentication file")?;
    if auth.version != 1 || PasswordHash::new(&auth.password_hash).is_err() {
        return Err("unsupported password hash configuration".into());
    }
    if let Some(cfg) = &review_config {
        let authority = crate::review_authority::Authority::load(&cfg.authority)?;
        if authority.shares_password_hash(&auth.password_hash) {
            return Err(
                "independent reviewer credential cannot equal dashboard owner credential".into(),
            );
        }
        let c = crate::catalog::Catalog::existing(catalog.ok_or("review catalog missing")?)?;
        for artifact in &cfg.artifacts {
            c.machine(&artifact.machine_id)?
                .ok_or("configured machine admission missing")?;
        }
    }
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let app = Arc::new(App {
        registry: Mutex::new(Registry::open(database)?),
        catalog_path: catalog.map(Path::to_path_buf),
        review_config,
        review_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        review_bucket: Mutex::new((Instant::now(), 0)),
        password_hash: auth.password_hash,
        sessions: Mutex::new(HashMap::new()),
        login_bucket: Mutex::new((Instant::now(), 0)),
        requests: Mutex::new((Instant::now(), 0)),
        login_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        port,
    });
    let router = Router::new()
        .route("/", get(root))
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/javascript")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/api/health",
            get(|| async {
                Json(serde_json::json!({"ok":true,"software_status":"SOFTWARE_INCOMPLETE"}))
            }),
        )
        .route("/api/login", post(login))
        .route("/api/session", get(session_info))
        .route("/api/logout", post(logout))
        .route("/api/overview", get(overview))
        .route("/api/catalog", get(catalog_page))
        .route("/api/catalog/{id}", get(catalog_detail))
        .route("/api/data-review/{id}", get(data_review_state))
        .route("/api/data-review", post(data_review_decide))
        .route("/api/data-eligibility/{id}", get(live_eligibility))
        .route("/api/experiments", get(experiments))
        .route("/api/experiment-history", get(history))
        .route("/api/training-diagnoses", get(diagnoses))
        .route("/api/training-diagnoses/{id}", get(diagnosis_detail))
        .route("/api/training-reviews", get(reviews))
        .route("/api/training-reviews/{id}", get(review_detail))
        .route("/api/protocols", get(protocols))
        .route("/api/protocols/{id}", get(protocol_detail))
        .route("/api/experiments/{id}", get(detail))
        .route("/api/runs/{id}/control", post(control))
        .layer(DefaultBodyLimit::max(8192))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app);
    println!(
        "{}",
        serde_json::json!({"listening_port":port,"loopback_only":true})
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn history_keeps_full_sqlite_cursor_range_and_empty_boundaries_exact() {
        // Isolated SQL projection fixture, not a valid research/evidence registry.
        let registry = Registry {
            connection: rusqlite::Connection::open_in_memory().unwrap(),
        };
        registry
            .connection
            .execute_batch(
                "CREATE TABLE experiments(id TEXT,key_json TEXT,state TEXT,result_json TEXT);",
            )
            .unwrap();
        for (rowid, instrument) in [
            (1, "EURUSD"),
            (i64::MAX - 1, "GBPUSD"),
            (i64::MAX, "EURUSD"),
        ] {
            registry.connection.execute("INSERT INTO experiments(rowid,id,key_json,state,result_json) VALUES(?1,?2,?3,'completed',?4)",
                rusqlite::params![rowid,rowid.to_string(),serde_json::json!({"instrument":instrument}).to_string(),r#"{"tick_replay":{"realized_micro_usd":9007199254740992}}"#]).unwrap();
        }
        let first = experiment_page(&registry, None, Some("EURUSD"), 1).unwrap();
        assert_eq!(first.rows.len(), 1);
        assert_eq!(first.next_cursor.as_deref(), Some("9223372036854775807"));
        assert_eq!(first.rows[0]["id"], "9223372036854775807");
        assert!(first.rows[0]["ending_micro_usd"].is_null());
        let last = experiment_page(&registry, Some(i64::MAX), Some("EURUSD"), 1).unwrap();
        assert_eq!(last.rows[0]["id"], "1");
        assert!(last.next_cursor.is_none());
        let empty = experiment_page(&registry, Some(1), None, 25).unwrap();
        assert!(empty.rows.is_empty());
        assert!(empty.next_cursor.is_none());
        let missing = experiment_page(&registry, None, Some("XAUUSD"), 25).unwrap();
        assert!(missing.rows.is_empty());
        assert!(missing.next_cursor.is_none());
    }
}

#[cfg(test)]
mod training_lineage_tests {
    use super::*;
    #[test]
    fn training_pages_follow_verified_lineage_and_refuse_tampered_evidence() {
        let directory = std::env::temp_dir().join(format!(
            "factory-dashboard-lineage-{}",
            random_hex().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let mut registry = Registry::open(&directory.join("state.sqlite")).unwrap();
        let dataset = crate::data::synthetic_fixture("EURUSD").unwrap();
        let input = research::Input {
            conversions: vec![crate::contracts::Conversion::usd(); dataset.ticks.len()],
            dataset,
        };
        let strategy = crate::strategies::Strategy {
            timeframe: crate::candles::Timeframe::M1,
            side: crate::contracts::Side::Long,
            entry: None,
            stop_ticks: 100,
            stop_policy: None,
            rule: crate::strategies::Rule::AboveMean { window: 100000 },
        };
        let parent = research::evaluate(
            &mut registry,
            &input,
            &strategy,
            &crate::tick_engine::Costs::default(),
            "lineage-query-fixture",
            10000,
        )
        .unwrap();
        let mut report_ids = Vec::new();
        for seed in 0..3 {
            report_ids.push(
                crate::improvement::diagnose(
                    &mut registry,
                    &parent.experiment_id,
                    &crate::improvement::Options {
                        seed,
                        max_children: 1,
                        ..Default::default()
                    },
                )
                .unwrap()
                .report
                .id,
            );
        }
        let first = training_page(&registry, &parent.experiment_id, None, 2, false).unwrap();
        assert_eq!(
            first
                .rows
                .iter()
                .map(|r| r["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![report_ids[2].as_str(), report_ids[1].as_str()]
        );
        let last = training_page(
            &registry,
            &parent.experiment_id,
            Some(first.next_cursor.unwrap().parse().unwrap()),
            2,
            false,
        )
        .unwrap();
        assert_eq!(last.rows.len(), 1);
        assert_eq!(last.rows[0]["id"], report_ids[0]);
        assert!(last.next_cursor.is_none());
        let review = crate::training_review::execute(
            &mut registry,
            &input,
            &report_ids[0],
            10000,
            256 * 1024 * 1024,
        )
        .unwrap();
        let reviews = training_page(&registry, &report_ids[0], None, 10, true).unwrap();
        assert_eq!(reviews.rows[0]["id"], review.report.id);
        assert_eq!(
            training_record(&registry, &review.report.id, true)
                .unwrap()
                .unwrap()["diagnosis_id"],
            report_ids[0]
        );
        registry.integrity().unwrap();
        registry.connection.execute("UPDATE training_diagnoses SET report_json=json_set(report_json,'$.status','FORGED') WHERE id=?1",[&report_ids[2]]).unwrap();
        assert!(training_record(&registry, &report_ids[2], false).is_err());
        assert!(training_page(&registry, &parent.experiment_id, None, 2, false).is_err());
        drop(registry);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
