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
    operations: Option<OperationsConfig>,
    review_slots: Arc<tokio::sync::Semaphore>,
    recovery_slots: Arc<tokio::sync::Semaphore>,
    recovery_read_slots: Arc<tokio::sync::Semaphore>,
    operations_slots: Arc<tokio::sync::Semaphore>,
    review_bucket: Mutex<(Instant, u32)>,
    operations_bucket: Mutex<(Instant, u32)>,
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
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewQuery {
    runs: Option<bool>,
}
async fn overview(
    State(app): State<Arc<App>>,
    Query(overview_query): Query<OverviewQuery>,
) -> Response {
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
            .take(if overview_query.runs.unwrap_or(true) {
                100
            } else {
                0
            })
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
        Some("rsfb") => crate::blocked_native::read(path, 2000000),
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
        "SELECT rowid,id,state,key_json,json_extract(result_json,'$.data_manifest'),json_extract(result_json,'$.strategy'),coalesce(json_array_length(result_json,'$.tick_replay.trades'),json_extract(result_json,'$._receipt_summary.closed_trades')),CASE WHEN typeof(json_extract(result_json,'$.tick_replay.realized_micro_usd'))='integer' AND json_extract(result_json,'$.tick_replay.realized_micro_usd') BETWEEN -9007199254740991 AND 9007199254740991 THEN json_extract(result_json,'$.tick_replay.realized_micro_usd') ELSE NULL END FROM experiments WHERE 1=1",
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

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TradeQuery {
    after: Option<String>,
    limit: Option<usize>,
}
fn trade_cursor(cursor: Option<&str>) -> Result<Option<(String, usize)>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let (hash, offset) = cursor.split_once(':').ok_or("invalid trade cursor")?;
    if !crate::machine_evidence::hex(hash)
        || offset.is_empty()
        || !offset.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("invalid trade cursor".into());
    }
    let offset = offset
        .parse::<usize>()
        .map_err(|_| "invalid trade cursor")?;
    if !(1..=1_000_000).contains(&offset) {
        return Err("invalid trade cursor offset".into());
    }
    Ok(Some((hash.into(), offset)))
}
fn trade_page(
    result: &crate::storage::StoredResult,
    after: Option<&(String, usize)>,
    limit: usize,
) -> Result<serde_json::Value> {
    if !(1..=100).contains(&limit) {
        return Err("invalid trade page size".into());
    }
    let offset = if let Some((hash, offset)) = after {
        if hash != &result.result_hash {
            return Err("trade cursor belongs to another immutable receipt".into());
        }
        *offset
    } else {
        0
    };
    let trades = result.result["tick_replay"]["trades"].as_array();
    let total = trades.map_or(0, Vec::len);
    if total > 1_000_000 || offset > total {
        return Err("invalid trade page extent".into());
    }
    let end = offset.saturating_add(limit).min(total);
    let rows=trades.into_iter().flatten().skip(offset).take(limit).enumerate().map(|(i,trade)|serde_json::json!({"ordinal":(offset+i+1).to_string(),"trade":trade,"event_ids":{"entry":trade["entry_event"].as_u64().map(|n|n.to_string()),"exit":trade["exit_event"].as_u64().map(|n|n.to_string())}})).collect::<Vec<_>>();
    Ok(
        serde_json::json!({"result_hash":result.result_hash,"replay_present":!result.result["tick_replay"].is_null(),"total_trades":total,"first_ordinal":if rows.is_empty(){None}else{Some((offset+1).to_string())},"last_ordinal":if rows.is_empty(){None}else{Some(end.to_string())},"rows":rows,"next_cursor":if end<total{Some(format!("{}:{end}",result.result_hash))}else{None},"limitations":"Immutable receipt page; the complete stored receipt is still loaded and hash-verified before slicing. Full exact export remains separate."}),
    )
}
async fn trades(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Query(query): Query<TradeQuery>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid experiment identity");
    }
    let limit = query.limit.unwrap_or(100);
    if !(1..=100).contains(&limit) {
        return api_error(StatusCode::BAD_REQUEST, "invalid trade page size");
    }
    let after = match trade_cursor(query.after.as_deref()) {
        Ok(v) => v,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "invalid trade cursor"),
    };
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match registry.result(&id) {
        Ok(Some(result)) => match trade_page(&result, after.as_ref(), limit) {
            Ok(page) => Json(page).into_response(),
            Err(_) => api_error(
                StatusCode::CONFLICT,
                "trade cursor or receipt extent mismatch",
            ),
        },
        Ok(None) => api_error(StatusCode::NOT_FOUND, "result not completed"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "result integrity check failed",
        ),
    }
}
async fn campaign_rotations(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::campaign_rotation::list(&registry) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "campaign rotation inventory integrity unavailable",
        ),
    }
}
async fn campaign_rotation_status(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid campaign rotation identity",
        );
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let exists: std::result::Result<bool, _> = registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_campaign_rotations WHERE id=?1)",
        [&id],
        |r| r.get(0),
    );
    if matches!(exists, Ok(false)) {
        return api_error(StatusCode::NOT_FOUND, "campaign rotation not found");
    }
    match crate::campaign_rotation::status(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "campaign rotation snapshot integrity unavailable",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignRotationControl {
    state: String,
    expected_epoch: String,
}
async fn campaign_rotation_control(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<CampaignRotationControl>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid campaign rotation identity",
        );
    }
    let Ok(epoch) = body.expected_epoch.parse::<i64>() else {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign rotation epoch");
    };
    if epoch < 0 || epoch.to_string() != body.expected_epoch {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign rotation epoch");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::campaign_rotation::control_expected(&mut registry, &id, &body.state, epoch) {
        Ok(()) => match crate::campaign_rotation::status(&registry, &id) {
            Ok(value) => Json(value).into_response(),
            Err(_) => api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "campaign rotation snapshot integrity unavailable",
            ),
        },
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "campaign rotation epoch or control transition refused",
        ),
    }
}

async fn campaigns(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_campaign::list(&registry) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "campaign inventory integrity unavailable",
        ),
    }
}
async fn campaign_status(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let exists: std::result::Result<bool, _> = registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_campaigns WHERE id=?1)",
        [&id],
        |r| r.get(0),
    );
    if matches!(exists, Ok(false)) {
        return api_error(StatusCode::NOT_FOUND, "campaign not found");
    }
    match crate::corpus_campaign::status(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "campaign snapshot integrity unavailable",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignControl {
    state: String,
    expected_epoch: String,
}
async fn campaign_owned_page(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Query(query): Query<ProcessHistoryQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(32);
    if !catalog_id(&id)
        || !(1..=128).contains(&limit)
        || query.after.as_ref().is_some_and(|a| !catalog_id(a))
    {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign owned page");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_campaigns WHERE id=?1)",
        [&id],
        |r| r.get::<_, bool>(0),
    ) {
        Ok(true) => {}
        Ok(false) => return api_error(StatusCode::NOT_FOUND, "campaign not found"),
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "campaign inventory unavailable",
            );
        }
    }
    if let Some(after) = &query.after {
        match registry.connection.query_row("SELECT EXISTS(SELECT 1 FROM corpus_campaign_owned_visits WHERE campaign_id=?1 AND invocation_id=?2)",rusqlite::params![id,after],|r|r.get::<_,bool>(0)) {
            Ok(true)=>{},Ok(false)=>return api_error(StatusCode::BAD_REQUEST,"unknown campaign owned cursor"),Err(_)=>return api_error(StatusCode::INTERNAL_SERVER_ERROR,"campaign ledger unavailable"),
        }
    }
    match crate::corpus_campaign::owned_visit_page(&registry, &id, query.after.as_deref(), limit) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified campaign ledger unavailable",
        ),
    }
}
async fn campaign_control(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<CampaignControl>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign identity");
    }
    let Ok(epoch) = body.expected_epoch.parse::<i64>() else {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign epoch");
    };
    if epoch < 0 || epoch.to_string() != body.expected_epoch {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign epoch");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_campaign::control_expected(&mut registry, &id, &body.state, epoch) {
        Ok(()) => match crate::corpus_campaign::status(&registry, &id) {
            Ok(value) => Json(value).into_response(),
            Err(_) => api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "campaign snapshot integrity unavailable",
            ),
        },
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "campaign epoch or control transition refused",
        ),
    }
}

async fn corpus_fleets(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_fleet::list(&registry) {
        Ok(rows) => Json(rows).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "multi-corpus inventory unavailable",
        ),
    }
}
async fn corpus_fleet_status(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid multi-corpus identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let exists: std::result::Result<bool, _> = registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_fleets WHERE id=?1)",
        [&id],
        |r| r.get(0),
    );
    if matches!(exists, Ok(false)) {
        return api_error(StatusCode::NOT_FOUND, "multi-corpus coordinator not found");
    }
    match crate::corpus_fleet::status(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "multi-corpus rotation integrity unavailable",
        ),
    }
}
async fn corpus_fleet_control(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<Control>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid multi-corpus identity");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_fleet::control(&mut registry, &id, &body.state) {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid multi-corpus control transition",
        ),
    }
}
async fn generated_searches(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_search::list(&registry) {
        Ok(list) => Json(list).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to verify generated search inventory",
        ),
    }
}
async fn generated_search_status(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid generated search identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let exists: std::result::Result<bool, _> = registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_searches WHERE id=?1)",
        [&id],
        |r| r.get(0),
    );
    if matches!(exists, Ok(false)) {
        return api_error(StatusCode::NOT_FOUND, "generated search not found");
    }
    match crate::corpus_search::status(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "generated search integrity unavailable",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionStopRequest {}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessHistoryQuery {
    after: Option<String>,
    limit: Option<usize>,
}
fn process_history_response(
    registry: &Registry,
    query: ProcessHistoryQuery,
    owned: bool,
) -> Response {
    let limit = query.limit.unwrap_or(128);
    if !(1..=128).contains(&limit) || query.after.as_ref().is_some_and(|id| !catalog_id(id)) {
        return api_error(StatusCode::BAD_REQUEST, "invalid process history page");
    }
    if let Some(id) = &query.after {
        let table = if owned {
            "corpus_owned_invocations"
        } else {
            "corpus_supervision_sessions"
        };
        match registry.connection.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
            [id],
            |row| row.get::<_, bool>(0),
        ) {
            Ok(true) => {}
            Ok(false) => {
                return api_error(StatusCode::BAD_REQUEST, "unknown process history cursor");
            }
            Err(_) => {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "process history unavailable",
                );
            }
        }
    }
    let result = if owned {
        crate::owned_history::page(registry, query.after.as_deref(), limit)
    } else {
        crate::supervision_session::page(registry, query.after.as_deref(), limit)
    };
    match result {
        Ok(page) => Json(page).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified process history unavailable",
        ),
    }
}
async fn owned_invocations(
    State(app): State<Arc<App>>,
    Query(query): Query<ProcessHistoryQuery>,
) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    process_history_response(&registry, query, true)
}
async fn owned_invocation_status(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid invocation identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_owned_invocations WHERE id=?1)",
        [&id],
        |r| r.get::<_, bool>(0),
    ) {
        Ok(false) => return api_error(StatusCode::NOT_FOUND, "invocation not found"),
        Ok(true) => {}
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invocation inventory unavailable",
            );
        }
    }
    match crate::owned_history::status(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified owned invocation unavailable",
        ),
    }
}
async fn supervision_sessions(
    State(app): State<Arc<App>>,
    Query(query): Query<ProcessHistoryQuery>,
) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    process_history_response(&registry, query, false)
}
fn session_exists(registry: &Registry, id: &str) -> crate::Result<bool> {
    registry
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM corpus_supervision_sessions WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}
async fn supervision_status(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid session identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match session_exists(&registry, &id) {
        Ok(true) => {}
        Ok(false) => return api_error(StatusCode::NOT_FOUND, "session not found"),
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session inventory unavailable",
            );
        }
    }
    match crate::supervision_session::status(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified session status unavailable",
        ),
    }
}
async fn supervision_stop(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(_body): Json<SessionStopRequest>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid session identity");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match session_exists(&registry, &id) {
        Ok(true) => {}
        Ok(false) => return api_error(StatusCode::NOT_FOUND, "session not found"),
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session inventory unavailable",
            );
        }
    }
    match crate::supervision_session::request_stop(
        &mut registry,
        &id,
        chrono::Utc::now().timestamp_millis(),
    ) {
        Ok(requested) => {
            Json(serde_json::json!({"session_id":id,"requested":requested,"workers_started":0}))
                .into_response()
        }
        Err(_) => api_error(StatusCode::CONFLICT, "verified session stop unavailable"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequeueRequest {
    maximum_slots: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryEpoch {
    expected_epoch: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryPageQuery {
    after: Option<String>,
    limit: Option<usize>,
}
async fn retry_decisions(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Query(query): Query<RetryPageQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(16);
    if !catalog_id(&id) || !(1..=32).contains(&limit) {
        return api_error(StatusCode::BAD_REQUEST, "invalid retry history scope/limit");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::retry_policy::decisions(&registry, &id, query.after.as_deref(), limit) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "verified retry history/cursor unavailable",
        ),
    }
}
async fn retry_configure(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(policy): Json<crate::retry_policy::Policy>,
) -> Response {
    if !catalog_id(&id) || policy.validate().is_err() {
        return api_error(StatusCode::BAD_REQUEST, "invalid retry identity or policy");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::retry_policy::configure(&mut registry, &id, &policy) {
        Ok(hash) => {
            Json(serde_json::json!({"search_id":id,"policy_hash":hash,"policy":{"backoff_ms":policy.backoff_ms.to_string(),"maximum_slots":policy.maximum_slots.to_string()},"workers_started":0})).into_response()
        }
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "retry policy unavailable or immutable policy differs",
        ),
    }
}
async fn retry_admit(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<RetryEpoch>,
) -> Response {
    let Ok(epoch) = body.expected_epoch.parse::<i64>() else {
        return api_error(StatusCode::BAD_REQUEST, "invalid retry epoch");
    };
    if !catalog_id(&id) || epoch < 0 {
        return api_error(StatusCode::BAD_REQUEST, "invalid retry identity or epoch");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::retry_policy::admit(
        &mut registry,
        &id,
        epoch,
        chrono::Utc::now().timestamp_millis(),
    ) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "retry admission unavailable; inspect verified state and budgets",
        ),
    }
}
async fn generated_search_requeue(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<RequeueRequest>,
) -> Response {
    if !catalog_id(&id) || !(1..=32).contains(&body.maximum_slots) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid search identity or requeue bound",
        );
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_search::requeue_errors(&mut registry, &id, body.maximum_slots) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "requeue unavailable; inspect paused state, coordinator lease, queue capacity and bounded retry budget",
        ),
    }
}
async fn generated_search_control(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<Control>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid generated search identity");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::corpus_search::control(&mut registry, &id, &body.state) {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid generated search control transition",
        ),
    }
}
async fn artifact_status(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::result_artifact::inventory(&registry) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "artifact inventory unavailable; preserve evidence and inspect registry",
        ),
    }
}
async fn disk_status(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let Some(path) = registry.connection.path().filter(|s| !s.is_empty()) else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "filesystem observations unavailable",
        );
    };
    match crate::disk::observe(Path::new(path)) {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "filesystem observations unavailable",
        ),
    }
}
async fn resource_status() -> Response {
    let observe = || -> Result<serde_json::Value> {
        let snapshot = crate::resource::observe()?;
        let mut value = serde_json::json!(snapshot);
        for field in [
            "process_rss_bytes",
            "host_available_memory_bytes",
            "available_parallelism",
        ] {
            value[field] = serde_json::Value::String(value[field].to_string());
        }
        let peak = crate::resource::observe_peak_rss()?;
        let process = crate::resource::observe_process(std::process::id())?;
        value["peak_rss_bytes"] = serde_json::json!(peak.to_string());
        value["process_user_ticks"] = serde_json::json!(process.user_ticks.to_string());
        value["process_system_ticks"] = serde_json::json!(process.system_ticks.to_string());
        value["process_start_ticks"] = serde_json::json!(process.start_ticks.to_string());
        value["cpu_scope"] = serde_json::json!(
            "Kernel accounting ticks for the dashboard process only; raw units, not milliseconds, not worker aggregates and not hard OS enforcement."
        );
        Ok(value)
    };
    match observe() {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "resource observations unavailable",
        ),
    }
}
async fn corpus_status(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid corpus identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "registry unavailable");
    };
    match registry.corpus_status(&id) {
        Ok(v) => Json(v).into_response(),
        Err(_) => api_error(
            StatusCode::NOT_FOUND,
            "verified corpus recovery status unavailable",
        ),
    }
}
async fn corpus_control(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(body): Json<Control>,
) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid corpus identity");
    }
    let Ok(mut registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "registry unavailable");
    };
    match registry.set_corpus_control(&id, &body.state, chrono::Utc::now().timestamp_millis()) {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid corpus job or control transition",
        ),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerQuery {
    before: Option<String>,
    limit: Option<usize>,
}
fn ledger_page(
    registry: &Registry,
    before: Option<i64>,
    limit: usize,
    runs: bool,
) -> Result<HistoryPage> {
    if !(1..=100).contains(&limit) || before.is_some_and(|n| n <= 0) {
        return Err("invalid ledger page bounds".into());
    }
    let tx = registry
        .connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let table = if runs {
        "research_runs"
    } else {
        "scientific_protocols"
    };
    let columns = if runs {
        "rowid,id,NULL,NULL,0"
    } else {
        "rowid,id,definition_json,definition_hash,report_json IS NOT NULL"
    };
    let mut sql = format!("SELECT {columns} FROM {table}");
    let mut params = Vec::<rusqlite::types::Value>::new();
    if let Some(before) = before {
        sql.push_str(" WHERE rowid < ?");
        params.push(before.into());
    }
    sql.push_str(" ORDER BY rowid DESC LIMIT ?");
    params.push(((limit + 1) as i64).into());
    let mut records = {
        let mut q = tx.prepare(&sql).map_err(|e| e.to_string())?;
        q.query_map(rusqlite::params_from_iter(params), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    let more = records.len() > limit;
    records.truncate(limit);
    let next_cursor = if more {
        records.last().map(|r| r.0.to_string())
    } else {
        None
    };
    let mut rows = Vec::new();
    for (_, id, definition, hash, report_ready) in records {
        if !catalog_id(&id) {
            return Err("invalid ledger record identity".into());
        }
        if runs {
            let status = research::status(registry, &id, chrono::Utc::now().timestamp_millis())?;
            let mut value = serde_json::to_value(status).map_err(|e| e.to_string())?;
            for field in ["cursor", "executed", "reused", "rejected", "failed"] {
                value[field] = serde_json::Value::String(value[field].to_string());
            }
            for field in ["queued", "completed", "failed", "child_receipt_links"] {
                value["improvements"][field] =
                    serde_json::Value::String(value["improvements"][field].to_string());
            }
            rows.push(value);
        } else {
            let definition = definition.ok_or("missing protocol definition")?;
            if definition.len() > 8 * 1024 * 1024 {
                return Err("protocol definition exceeds page budget".into());
            }
            let definition: serde_json::Value =
                serde_json::from_str(&definition).map_err(|e| e.to_string())?;
            if hash.as_deref() != Some(id.as_str()) || crate::data::hash(&definition)? != id {
                return Err("protocol definition integrity failure".into());
            }
            rows.push(serde_json::json!({"id":id,"report_ready":report_ready}));
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(HistoryPage { rows, next_cursor })
}
fn ledger_response(app: &App, query: LedgerQuery, runs: bool) -> Response {
    let before = match query.before {
        None => None,
        Some(v) => match v.parse::<i64>() {
            Ok(n) if n > 0 && v.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => return api_error(StatusCode::BAD_REQUEST, "invalid ledger cursor"),
        },
    };
    let limit = query.limit.unwrap_or(25);
    if !(1..=100).contains(&limit) {
        return api_error(StatusCode::BAD_REQUEST, "invalid ledger page size");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match ledger_page(&registry, before, limit, runs) {
        Ok(page) => Json(page).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to verify persisted ledger page",
        ),
    }
}
async fn protocol_history(
    State(app): State<Arc<App>>,
    Query(query): Query<LedgerQuery>,
) -> Response {
    ledger_response(&app, query, false)
}
async fn run_history(State(app): State<Arc<App>>, Query(query): Query<LedgerQuery>) -> Response {
    ledger_response(&app, query, true)
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryCreate {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryRestore {
    id: String,
    expected_manifest_hash: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryVerify {}
fn recovery_database(app: &App) -> Result<PathBuf> {
    let reg = app.registry.lock().map_err(|_| "database unavailable")?;
    let path = reg
        .connection
        .path()
        .filter(|s| !s.is_empty() && *s != ":memory:")
        .ok_or("filesystem registry unavailable")?;
    Ok(PathBuf::from(path))
}
async fn recovery_inventory(State(app): State<Arc<App>>) -> Response {
    recovery_read(app, None).await
}
async fn recovery_detail(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !crate::recovery::valid_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid snapshot identity");
    }
    recovery_read(app, Some(id)).await
}
async fn recovery_read(app: Arc<App>, id: Option<String>) -> Response {
    let Ok(path) = recovery_database(&app) else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "filesystem recovery unavailable",
        );
    };
    let Ok(permit) = app.recovery_read_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "recovery inspection busy");
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if let Some(id) = id {
            crate::recovery::describe(&path, &id)
        } else {
            crate::recovery::list(&path).map(Some)
        }
    })
    .await;
    match result {
        Ok(Ok(Some(value))) => Json(value).into_response(),
        Ok(Ok(None)) => api_error(StatusCode::NOT_FOUND, "complete snapshot not found"),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified recovery metadata unavailable; preserve private evidence",
        ),
    }
}
async fn recovery_create(
    State(app): State<Arc<App>>,
    Json(body): Json<RecoveryCreate>,
) -> Response {
    if !crate::recovery::valid_id(&body.id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid new snapshot identity");
    }
    recovery_action(app, None, body.id, None).await
}
async fn recovery_verify(
    State(app): State<Arc<App>>,
    Param(id): Param<String>,
    Json(_body): Json<RecoveryVerify>,
) -> Response {
    if !crate::recovery::valid_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid snapshot identity");
    }
    recovery_action(app, Some(id), String::new(), None).await
}
async fn recovery_restore(
    State(app): State<Arc<App>>,
    Param(source): Param<String>,
    Json(body): Json<RecoveryRestore>,
) -> Response {
    if !crate::recovery::valid_id(&source)
        || !crate::recovery::valid_id(&body.id)
        || source == body.id
        || !crate::machine_evidence::hex(&body.expected_manifest_hash)
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid distinct restore identity or reviewed manifest",
        );
    }
    recovery_action(
        app,
        Some(source),
        body.id,
        Some(body.expected_manifest_hash),
    )
    .await
}
async fn recovery_action(
    app: Arc<App>,
    source: Option<String>,
    id: String,
    expected: Option<String>,
) -> Response {
    let Ok(path) = recovery_database(&app) else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "filesystem recovery unavailable",
        );
    };
    let Ok(permit) = app.recovery_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::CONFLICT, "another recovery operation is active");
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        match (source, expected) {
            (None, _) => crate::recovery::backup(&path, &id),
            (Some(source), None) => crate::recovery::verify(&path, &source),
            (Some(source), Some(expected)) => {
                crate::recovery::restore_expected(&path, &source, &id, Some(&expected))
            }
        }
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        _ => api_error(
            StatusCode::CONFLICT,
            "recovery operation refused; inspect private state, existing paths and retained evidence",
        ),
    }
}
async fn protocol_summary(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid protocol identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    match crate::science::read_report(&registry, &id)
        .and_then(|r| r.map(|v| crate::science::summary(&v)).transpose())
    {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "protocol report is not complete"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified scientific summary unavailable",
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
// Operator-owned operations configuration: requests never choose local paths.
// Import activation registers a local catalog observation only; a declared
// class is never approval, eligibility or qualification.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationsConfig {
    #[serde(default)]
    imports: Vec<ImportTarget>,
    #[serde(default)]
    coverage: Vec<CoverageTarget>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportTarget {
    name: String,
    directory: PathBuf,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageTarget {
    name: String,
    dataset: PathBuf,
    start_ms: i64,
    end_exclusive_ms: i64,
    gap_threshold_ms: i64,
}
fn lower_alnum(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}
fn operations_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && lower_alnum(bytes[0])
        && bytes.iter().all(|b| lower_alnum(*b) || *b == b'-')
}
impl OperationsConfig {
    fn load(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > 128 * 1024 {
            return Err("operations configuration must be a regular file within128KiB".into());
        }
        let body = fs::read(path).map_err(|e| e.to_string())?;
        if body.len() > 128 * 1024 {
            return Err("operations configuration exceeds128KiB".into());
        }
        let mut cfg: Self =
            serde_json::from_slice(&body).map_err(|_| "invalid operations configuration")?;
        if cfg.imports.len() > 16 || cfg.coverage.len() > 16 {
            return Err(
                "operations configuration allows at most16 imports and16 coverage targets".into(),
            );
        }
        if cfg.imports.is_empty() && cfg.coverage.is_empty() {
            return Err("operations configuration declares no targets".into());
        }
        let base = path.parent().unwrap_or(Path::new("."));
        let mut seen = std::collections::HashSet::new();
        for import in &mut cfg.imports {
            if !operations_name(&import.name)
                || !seen.insert(import.name.clone())
                || import.directory.as_os_str().is_empty()
            {
                return Err("invalid or duplicate configured import target".into());
            }
            import.directory = base.join(&import.directory);
            if import.directory.exists() && !import.directory.is_dir() {
                return Err("configured import target must be a directory".into());
            }
        }
        for target in &mut cfg.coverage {
            if !operations_name(&target.name)
                || !seen.insert(target.name.clone())
                || target.dataset.as_os_str().is_empty()
            {
                return Err("invalid or duplicate configured coverage target".into());
            }
            target.dataset = base.join(&target.dataset);
            if chrono::DateTime::from_timestamp_millis(target.start_ms).is_none()
                || chrono::DateTime::from_timestamp_millis(target.end_exclusive_ms).is_none()
                || target.start_ms >= target.end_exclusive_ms
                || target.gap_threshold_ms <= 0
                || target.gap_threshold_ms > 366 * 24 * 3600 * 1000
            {
                return Err("invalid configured coverage interval or gap threshold".into());
            }
            if target.dataset.exists() && !target.dataset.is_file() {
                return Err("configured coverage dataset must be a regular file".into());
            }
        }
        Ok(cfg)
    }
    fn import(&self, name: &str) -> Option<&ImportTarget> {
        self.imports.iter().find(|t| t.name == name)
    }
    fn coverage(&self, name: &str) -> Option<&CoverageTarget> {
        self.coverage.iter().find(|t| t.name == name)
    }
    fn redaction_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for import in &self.imports {
            paths.push(import.directory.to_string_lossy().into_owned());
        }
        for target in &self.coverage {
            paths.push(target.dataset.to_string_lossy().into_owned());
        }
        paths
    }
}
/// Exact module refusal reasons stay visible; configured local paths never leak.
fn redact(text: &str, paths: &[String]) -> String {
    let mut value = text.to_string();
    for path in paths {
        if !path.is_empty() {
            value = value.replace(path.as_str(), "[redacted-path]");
        }
    }
    value
}
const DECISION_FILE: &str = "activation-decisions.jsonl";
const DECISION_LIMIT: usize = 1024;
const DECISION_LINE_LIMIT: usize = 8192;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationDecision {
    version: String,
    kind: String,
    name: String,
    at_ms: i64,
    expected_archive_hash: String,
    verification: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verification_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    events: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dataset_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalog_entry_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    declared_class: Option<String>,
    hash: String,
}
fn decision_body_hash(value: &serde_json::Value) -> Result<String> {
    let mut body = value.clone();
    body.as_object_mut()
        .ok_or("decision receipt must be an object")?
        .remove("hash");
    crate::data::hash(&body)
}
fn read_decisions(directory: &Path) -> Result<Vec<ActivationDecision>> {
    let path = directory.join(DECISION_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take((DECISION_LIMIT * DECISION_LINE_LIMIT + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > DECISION_LIMIT * DECISION_LINE_LIMIT {
        return Err("activation decision history exceeds budget".into());
    }
    let mut decisions = Vec::new();
    for line in text.lines() {
        if line.len() > DECISION_LINE_LIMIT {
            return Err("activation decision receipt exceeds budget".into());
        }
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|_| "activation decision history corrupt")?;
        let decision: ActivationDecision = serde_json::from_value(value.clone())
            .map_err(|_| "activation decision history corrupt")?;
        if decision.version != "import-activation-decision-v1"
            || decision.hash != decision_body_hash(&value)?
        {
            return Err("activation decision history integrity mismatch".into());
        }
        decisions.push(decision);
        if decisions.len() > DECISION_LIMIT {
            return Err("activation decision history exceeds budget".into());
        }
    }
    Ok(decisions)
}
fn append_decision(directory: &Path, mut decision: ActivationDecision) -> Result<()> {
    use fs2::FileExt;
    let path = directory.join(DECISION_FILE);
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.lock_exclusive().map_err(|e| e.to_string())?;
    let result = (|| -> Result<()> {
        let mut text = String::new();
        (&file)
            .take((DECISION_LIMIT * DECISION_LINE_LIMIT + 1) as u64)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > DECISION_LIMIT * DECISION_LINE_LIMIT
            || text.lines().count() >= DECISION_LIMIT
        {
            return Err("activation decision history full; preserve retained evidence".into());
        }
        for line in text.lines() {
            let value: serde_json::Value =
                serde_json::from_str(line).map_err(|_| "activation decision history corrupt")?;
            let stored: ActivationDecision = serde_json::from_value(value.clone())
                .map_err(|_| "activation decision history corrupt")?;
            if stored.hash != decision_body_hash(&value)? {
                return Err("activation decision history integrity mismatch".into());
            }
        }
        decision.hash = String::new();
        let mut body = serde_json::to_value(&decision).map_err(|e| e.to_string())?;
        let hash = decision_body_hash(&body)?;
        body["hash"] = serde_json::Value::String(hash.clone());
        decision.hash = hash;
        let line = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        if line.len() > DECISION_LINE_LIMIT {
            return Err("activation decision receipt exceeds budget".into());
        }
        let mut writer = &file;
        writer
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        writer.write_all(b"\n").map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    })();
    let _ = file.unlock();
    result
}
fn import_observation(target: &ImportTarget, paths: &[String]) -> serde_json::Value {
    let mut value = serde_json::json!({"name": target.name});
    if !target.directory.join("normalized-import.sqlite").is_file() {
        value["state"] = serde_json::json!("not_initialized");
        value["limitations"] = serde_json::json!(
            "No import ledger exists in the configured directory. Nothing is inferred about source data."
        );
        return value;
    }
    match crate::import_job::status(&target.directory) {
        Ok(p) => {
            value["state"] = serde_json::json!(if p.complete {
                "complete"
            } else {
                "in_progress"
            });
            value["complete"] = serde_json::json!(p.complete);
            value["events"] = serde_json::json!(p.events.to_string());
            value["chunks"] = serde_json::json!(p.chunks.len().to_string());
            value["archive_hash"] = serde_json::json!(p.configuration.archive_hash);
            value["archive_bytes"] = serde_json::json!(p.configuration.archive_bytes.to_string());
            value["synthetic"] = serde_json::json!(p.configuration.synthetic);
            value["started_at"] = serde_json::json!(p.started_at);
            value["limitations"] = serde_json::json!(p.limitations);
        }
        Err(e) => {
            value["state"] = serde_json::json!("refused");
            value["reason"] = serde_json::json!(redact(&e.to_string(), paths));
        }
    }
    match read_decisions(&target.directory) {
        Ok(decisions) => {
            value["decision_count"] = serde_json::json!(decisions.len().to_string());
            value["last_decision"] = serde_json::json!(decisions.last().map(|d| {
                serde_json::json!({"kind": d.kind, "at_ms": d.at_ms.to_string(), "hash": d.hash})
            }));
        }
        Err(e) => {
            value["decision_history"] = serde_json::json!("corrupt");
            value["decision_history_reason"] = serde_json::json!(redact(&e.to_string(), paths));
        }
    }
    value
}
async fn imports_inventory(State(app): State<Arc<App>>) -> Response {
    let Some(config) = app.operations.clone() else {
        return Json(serde_json::json!({"configured":false,"imports":[],"limitations":"No import directories configured. Import activation is unavailable."})).into_response();
    };
    if !bucket(&app.operations_bucket, 60) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations rate limit");
    }
    let Ok(permit) = app.operations_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations work busy");
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let paths = config.redaction_paths();
        config
            .imports
            .iter()
            .map(|t| import_observation(t, &paths))
            .collect::<Vec<_>>()
    })
    .await;
    match result {
        Ok(rows) => Json(serde_json::json!({"configured":true,"imports":rows,"maximum_imports":16,"limitations":"Verified checkpoint observations only. Complete means this exact source snapshot was parsed; not historical completeness, calendar/quality/profile approval or research eligibility. Activation registers a local catalog observation; a declared class is never approval."})).into_response(),
        _ => api_error(StatusCode::INTERNAL_SERVER_ERROR, "import inventory unavailable"),
    }
}
async fn import_detail(State(app): State<Arc<App>>, Param(name): Param<String>) -> Response {
    let Some(config) = app.operations.clone() else {
        return api_error(StatusCode::NOT_FOUND, "import operations not configured");
    };
    let Some(target) = config.import(&name).cloned() else {
        return api_error(StatusCode::NOT_FOUND, "import target not configured");
    };
    if !bucket(&app.operations_bucket, 60) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations rate limit");
    }
    let Ok(permit) = app.operations_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations work busy");
    };
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _permit = permit;
        let paths = config.redaction_paths();
        let progress = crate::import_job::status(&target.directory)
            .map_err(|e| redact(&e.to_string(), &paths))?;
        let decisions = read_decisions(&target.directory)
            .map_err(|e| redact(&e.to_string(), &paths))?;
        Ok(serde_json::json!({"name":target.name,"state":if progress.complete{"complete"}else{"in_progress"},"progress":progress,"decisions":decisions,"limitations":"Every published chunk was re-verified for this receipt. Decisions are operator evidence, not approval. A declared class is never eligibility or qualification."}))
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(reason)) => api_error(StatusCode::CONFLICT, &reason),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified import receipt unavailable",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportActivate {
    expected_archive_hash: String,
    expected_events: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRefuse {
    expected_archive_hash: String,
    reason: String,
}
fn canonical_events(value: &str) -> Option<usize> {
    if value.is_empty() || value.len() > 7 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: usize = value.parse().ok()?;
    (n.to_string() == value && n <= 2_000_000).then_some(n)
}
async fn import_activate(
    State(app): State<Arc<App>>,
    Param(name): Param<String>,
    Json(body): Json<ImportActivate>,
) -> Response {
    let (Some(config), Some(catalog)) = (app.operations.clone(), app.catalog_path.clone()) else {
        return api_error(
            StatusCode::CONFLICT,
            "activation requires configured imports and a dataset catalog",
        );
    };
    let Some(target) = config.import(&name).cloned() else {
        return api_error(StatusCode::NOT_FOUND, "import target not configured");
    };
    if !crate::machine_evidence::hex(&body.expected_archive_hash)
        || canonical_events(&body.expected_events).is_none()
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid expected archive hash or event count",
        );
    }
    if !bucket(&app.operations_bucket, 60) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations rate limit");
    }
    let Ok(permit) = app.operations_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations work busy");
    };
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _permit = permit;
        let paths = config.redaction_paths();
        let fail = |e: String| redact(&e, &paths);
        let progress = crate::import_job::status(&target.directory).map_err(fail)?;
        if !progress.complete || progress.events == 0 || progress.chunks.is_empty() {
            return Err("only a complete nonempty import can be activated".into());
        }
        if progress.configuration.archive_hash != body.expected_archive_hash {
            return Err("expected archive hash does not match the verified import".into());
        }
        if progress.events.to_string() != body.expected_events {
            return Err("expected event count does not match the verified import".into());
        }
        read_decisions(&target.directory).map_err(fail)?;
        let dataset = crate::import_job::assemble(&target.directory).map_err(fail)?;
        let mut catalog = crate::catalog::Catalog::open(&catalog).map_err(fail)?;
        let registration = catalog.register(&dataset).map_err(fail)?;
        let decision = ActivationDecision {
            version: "import-activation-decision-v1".into(),
            kind: "activated".into(),
            name: target.name.clone(),
            at_ms: chrono::Utc::now().timestamp_millis(),
            expected_archive_hash: body.expected_archive_hash.clone(),
            verification: "verified".into(),
            verification_error: None,
            reason: None,
            events: Some(progress.events.to_string()),
            dataset_identity: Some(registration.entry.receipt.facts.dataset_identity.clone()),
            catalog_entry_hash: Some(registration.entry.hash.clone()),
            reused: Some(registration.reused),
            declared_class: Some(format!("{:?}", dataset.manifest.class)),
            hash: String::new(),
        };
        append_decision(&target.directory, decision.clone()).map_err(fail)?;
        Ok(serde_json::json!({"decision":decision,"registration":registration.entry,"reused":registration.reused,"activation_performed":true,"limitations":"Activation registered a local catalog observation only. The declared class is not approval; no calendar/profile/legal/scientific gate, research eligibility or qualification is granted. No acquisition or live execution occurred."}))
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(reason)) => api_error(StatusCode::CONFLICT, &reason),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "import activation unavailable",
        ),
    }
}
async fn import_refuse(
    State(app): State<Arc<App>>,
    Param(name): Param<String>,
    Json(body): Json<ImportRefuse>,
) -> Response {
    let Some(config) = app.operations.clone() else {
        return api_error(StatusCode::NOT_FOUND, "import operations not configured");
    };
    let Some(target) = config.import(&name).cloned() else {
        return api_error(StatusCode::NOT_FOUND, "import target not configured");
    };
    if !crate::machine_evidence::hex(&body.expected_archive_hash)
        || body.reason.trim().is_empty()
        || body.reason.chars().count() > 4096
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid expected archive hash or refusal reason",
        );
    }
    if !bucket(&app.operations_bucket, 60) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations rate limit");
    }
    let Ok(permit) = app.operations_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations work busy");
    };
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _permit = permit;
        let paths = config.redaction_paths();
        let fail = |e: String| redact(&e, &paths);
        read_decisions(&target.directory).map_err(fail)?;
        let (verification, verification_error, events) =
            match crate::import_job::status(&target.directory) {
                Ok(p) => {
                    if p.configuration.archive_hash != body.expected_archive_hash {
                        return Err("expected archive hash does not match the verified import".into());
                    }
                    ("verified", None, Some(p.events.to_string()))
                }
                Err(e) => ("unverified", Some(redact(&e.to_string(), &paths)), None),
            };
        let decision = ActivationDecision {
            version: "import-activation-decision-v1".into(),
            kind: "refused".into(),
            name: target.name.clone(),
            at_ms: chrono::Utc::now().timestamp_millis(),
            expected_archive_hash: body.expected_archive_hash.clone(),
            verification: verification.into(),
            verification_error,
            reason: Some(body.reason.clone()),
            events,
            dataset_identity: None,
            catalog_entry_hash: None,
            reused: None,
            declared_class: None,
            hash: String::new(),
        };
        append_decision(&target.directory, decision.clone()).map_err(fail)?;
        Ok(serde_json::json!({"decision":decision,"activation_performed":false,"limitations":"Refusal is retained operator evidence. No import artifact, checkpoint or catalog entry was changed or deleted."}))
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(reason)) => api_error(StatusCode::CONFLICT, &reason),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "import refusal unavailable",
        ),
    }
}
async fn coverage_inventory(State(app): State<Arc<App>>) -> Response {
    let Some(config) = &app.operations else {
        return Json(serde_json::json!({"configured":false,"targets":[],"limitations":"No coverage targets configured. Coverage observations are unavailable."})).into_response();
    };
    let targets = config
        .coverage
        .iter()
        .map(|t| {
            serde_json::json!({"name":t.name,"start_ms":t.start_ms.to_string(),"end_exclusive_ms":t.end_exclusive_ms.to_string(),"gap_threshold_ms":t.gap_threshold_ms.to_string(),"detail_limit":100})
        })
        .collect::<Vec<_>>();
    Json(serde_json::json!({"configured":true,"targets":targets,"maximum_targets":16,"limitations":"Operator-configured observation windows only. Empty months and elapsed gaps do not prove missing source ranges without an approved trading calendar/source inventory. No completeness, approval or eligibility is established."})).into_response()
}
async fn coverage_report(State(app): State<Arc<App>>, Param(name): Param<String>) -> Response {
    let Some(config) = app.operations.clone() else {
        return api_error(StatusCode::NOT_FOUND, "coverage operations not configured");
    };
    let Some(target) = config.coverage(&name).cloned() else {
        return api_error(StatusCode::NOT_FOUND, "coverage target not configured");
    };
    if !bucket(&app.operations_bucket, 60) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations rate limit");
    }
    let Ok(permit) = app.operations_slots.clone().try_acquire_owned() else {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations work busy");
    };
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _permit = permit;
        let paths = config.redaction_paths();
        let dataset =
            review_dataset(&target.dataset).map_err(|e| redact(&e.to_string(), &paths))?;
        let report = crate::coverage::report(
            &dataset,
            crate::validation::Interval {
                start_ms: target.start_ms,
                end_exclusive_ms: target.end_exclusive_ms,
            },
            target.gap_threshold_ms,
            100,
        )
        .map_err(|e| redact(&e.to_string(), &paths))?;
        let mut value = serde_json::to_value(report).map_err(|e| e.to_string())?;
        value["name"] = serde_json::json!(target.name);
        Ok(value)
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(reason)) => api_error(StatusCode::CONFLICT, &reason),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "verified coverage report unavailable",
        ),
    }
}
async fn registry_audit(State(app): State<Arc<App>>) -> Response {
    if !bucket(&app.operations_bucket, 60) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "operations rate limit");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let audit = || -> Result<serde_json::Value> {
        registry.integrity()?;
        Ok(
            serde_json::json!({"integrity":"ok","counts":registry.counts()?,"audit_health":audit_health(&registry)?,"scope":"Read-only registry integrity observation at request time. Not a continuous audit, hard enforcement, audit coverage certification or qualification."}),
        )
    };
    match audit() {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "registry integrity verification refused; preserve evidence and inspect the registry",
        ),
    }
}
async fn campaign_verify_inputs(State(app): State<Arc<App>>, Param(id): Param<String>) -> Response {
    if !catalog_id(&id) {
        return api_error(StatusCode::BAD_REQUEST, "invalid campaign identity");
    }
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let exists: std::result::Result<bool, _> = registry.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM corpus_campaigns WHERE id=?1)",
        [&id],
        |r| r.get(0),
    );
    if matches!(exists, Ok(false)) {
        return api_error(StatusCode::NOT_FOUND, "campaign not found");
    }
    match crate::corpus_campaign::verify_inputs(&registry, &id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => api_error(
            StatusCode::CONFLICT,
            "campaign frozen input verification refused; the frozen plan, build or trial scope changed or is unavailable",
        ),
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
    serve_with_operations(database, auth_path, port, catalog, review_path, None).await
}
pub async fn serve_with_operations(
    database: &Path,
    auth_path: &Path,
    port: u16,
    catalog: Option<&Path>,
    review_path: Option<&Path>,
    operations_path: Option<&Path>,
) -> Result<()> {
    let review_config = review_path.map(ReviewConfig::load).transpose()?;
    let operations = operations_path.map(OperationsConfig::load).transpose()?;
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
        operations,
        review_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        recovery_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        recovery_read_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        operations_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        review_bucket: Mutex::new((Instant::now(), 0)),
        operations_bucket: Mutex::new((Instant::now(), 0)),
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
        .route(
            "/api/recovery/snapshots",
            get(recovery_inventory).post(recovery_create),
        )
        .route("/api/recovery/snapshots/{id}", get(recovery_detail))
        .route("/api/recovery/snapshots/{id}/verify", post(recovery_verify))
        .route(
            "/api/recovery/snapshots/{id}/restore",
            post(recovery_restore),
        )
        .route("/api/resources", get(resource_status))
        .route("/api/disk", get(disk_status))
        .route("/api/artifacts", get(artifact_status))
        .route("/api/campaign-rotations", get(campaign_rotations))
        .route(
            "/api/campaign-rotations/{id}",
            get(campaign_rotation_status),
        )
        .route(
            "/api/campaign-rotations/{id}/control",
            post(campaign_rotation_control),
        )
        .route("/api/campaigns", get(campaigns))
        .route("/api/campaigns/{id}", get(campaign_status))
        .route("/api/campaigns/{id}/control", post(campaign_control))
        .route("/api/campaigns/{id}/owned-visits", get(campaign_owned_page))
        .route(
            "/api/campaigns/{id}/verify-inputs",
            get(campaign_verify_inputs),
        )
        .route("/api/imports", get(imports_inventory))
        .route("/api/imports/{name}", get(import_detail))
        .route("/api/imports/{name}/activate", post(import_activate))
        .route("/api/imports/{name}/refuse", post(import_refuse))
        .route("/api/coverage", get(coverage_inventory))
        .route("/api/coverage/{name}", get(coverage_report))
        .route("/api/registry-audit", get(registry_audit))
        .route("/api/corpus-fleets", get(corpus_fleets))
        .route("/api/corpus-fleets/{id}", get(corpus_fleet_status))
        .route(
            "/api/corpus-fleets/{id}/control",
            post(corpus_fleet_control),
        )
        .route("/api/owned-invocations", get(owned_invocations))
        .route("/api/owned-invocations/{id}", get(owned_invocation_status))
        .route("/api/supervision-sessions", get(supervision_sessions))
        .route("/api/supervision-sessions/{id}", get(supervision_status))
        .route(
            "/api/supervision-sessions/{id}/stop",
            post(supervision_stop),
        )
        .route("/api/corpus-searches", get(generated_searches))
        .route(
            "/api/corpus-searches/{id}/retry-decisions",
            get(retry_decisions),
        )
        .route(
            "/api/corpus-searches/{id}/retry-policy",
            post(retry_configure),
        )
        .route("/api/corpus-searches/{id}/retry-admit", post(retry_admit))
        .route("/api/corpus-searches/{id}", get(generated_search_status))
        .route(
            "/api/corpus-searches/{id}/requeue",
            post(generated_search_requeue),
        )
        .route(
            "/api/corpus-searches/{id}/control",
            post(generated_search_control),
        )
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
        .route("/api/protocol-history", get(protocol_history))
        .route("/api/run-history", get(run_history))
        .route("/api/corpus/{id}/status", get(corpus_status))
        .route("/api/corpus/{id}/control", post(corpus_control))
        .route("/api/protocols/{id}/summary", get(protocol_summary))
        .route("/api/protocols/{id}", get(protocol_detail))
        .route("/api/experiments/{id}", get(detail))
        .route("/api/experiments/{id}/trades", get(trades))
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
        let registry =
            crate::storage::wrap_connection(rusqlite::Connection::open_in_memory().unwrap());
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

#[cfg(test)]
mod ledger_page_tests {
    use super::*;
    #[test]
    fn bounded_run_and_protocol_pages_keep_exact_cursors_across_inserts() {
        for runs in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "factory-ledger-pages-{}-{runs}",
                std::process::id()
            ));
            std::fs::create_dir(&root).unwrap();
            let registry = Registry::open(&root.join("registry.sqlite")).unwrap();
            let insert = |index: i64| {
                let value = serde_json::json!({"synthetic_page_fixture":index});
                let id = crate::data::hash(&value).unwrap();
                let row = 9007199254740993i64 + index;
                if runs {
                    registry.connection.execute("INSERT INTO research_runs(rowid,id,configuration,cursor,executed) VALUES(?1,?2,'{}',9007199254740993,9007199254740993)",rusqlite::params![row,id]).unwrap();
                } else {
                    registry.connection.execute("INSERT INTO scientific_protocols(rowid,id,definition_json,definition_hash) VALUES(?1,?2,?3,?2)",rusqlite::params![row,id,value.to_string()]).unwrap();
                }
                id
            };
            let expected: std::collections::BTreeSet<_> = (0..129).map(insert).collect();
            let first = ledger_page(&registry, None, 25, runs).unwrap();
            assert_eq!(first.next_cursor.as_deref(), Some("9007199254741097"));
            if runs {
                assert_eq!(first.rows[0]["cursor"], "9007199254740993");
                assert_eq!(first.rows[0]["executed"], "9007199254740993");
            }
            let mut seen: Vec<_> = first
                .rows
                .iter()
                .map(|r| r["id"].as_str().unwrap().to_owned())
                .collect();
            let mut cursor = first.next_cursor;
            let added = insert(999);
            assert!(!seen.contains(&added));
            while let Some(before) = cursor {
                let page = ledger_page(&registry, Some(before.parse().unwrap()), 25, runs).unwrap();
                assert!(page.rows.len() <= 25);
                seen.extend(
                    page.rows
                        .iter()
                        .map(|r| r["id"].as_str().unwrap().to_owned()),
                );
                cursor = page.next_cursor;
            }
            assert_eq!(seen.len(), 129);
            assert_eq!(
                seen.into_iter().collect::<std::collections::BTreeSet<_>>(),
                expected
            );
            assert_eq!(
                ledger_page(&registry, None, 25, runs).unwrap().rows[0]["id"],
                added
            );
            assert!(ledger_page(&registry, None, 0, runs).is_err());
            assert!(ledger_page(&registry, None, 101, runs).is_err());
            assert!(ledger_page(&registry, Some(0), 25, runs).is_err());
            registry.integrity().unwrap();
            drop(registry);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn protocol_page_refuses_corrupted_definition_identity() {
        let root =
            std::env::temp_dir().join(format!("factory-ledger-corrupt-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let value = serde_json::json!({"synthetic_fixture":1});
        let id = crate::data::hash(&value).unwrap();
        registry.connection.execute("INSERT INTO scientific_protocols(id,definition_json,definition_hash) VALUES(?1,?2,?1)",rusqlite::params![id,value.to_string()]).unwrap();
        assert_eq!(
            ledger_page(&registry, None, 25, false).unwrap().rows.len(),
            1
        );
        registry
            .connection
            .execute("UPDATE scientific_protocols SET definition_json='{}'", [])
            .unwrap();
        assert!(ledger_page(&registry, None, 25, false).is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod trade_page_tests {
    use super::*;
    #[test]
    fn receipt_bound_pages_cover_all_trades_and_exact_unsigned_ids() {
        let trades: Vec<_> = (0..602u64)
            .map(|i| serde_json::json!({"entry_event":u64::MAX-602+i,"exit_event":u64::MAX-601+i}))
            .collect();
        let result = serde_json::json!({"tick_replay":{"trades":trades}});
        let result = crate::storage::StoredResult {
            outcome: "completed".into(),
            result_hash: crate::data::hash(&result).unwrap(),
            result,
        };
        let mut after = None;
        let mut seen = Vec::new();
        loop {
            let page = trade_page(&result, after.as_ref(), 100).unwrap();
            assert!(page["rows"].as_array().unwrap().len() <= 100);
            for row in page["rows"].as_array().unwrap() {
                seen.push(row["event_ids"]["exit"].as_str().unwrap().to_owned());
            }
            if let Some(cursor) = page["next_cursor"].as_str() {
                after = trade_cursor(Some(cursor)).unwrap();
            } else {
                break;
            }
        }
        assert_eq!(seen.len(), 602);
        assert_eq!(seen[601], u64::MAX.to_string());
        assert_eq!(
            seen.into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            602
        );
        let first = trade_page(&result, None, 100).unwrap();
        let cursor = trade_cursor(first["next_cursor"].as_str()).unwrap();
        let mut other = result.clone();
        other.result["other_receipt"] = serde_json::json!(true);
        other.result_hash = crate::data::hash(&other.result).unwrap();
        assert!(trade_page(&other, cursor.as_ref(), 100).is_err());
        assert!(trade_page(&result, Some(&(result.result_hash.clone(), 603)), 100).is_err());
        assert!(trade_page(&result, None, 101).is_err());
    }
    #[test]
    fn malformed_trade_cursors_and_missing_replays_remain_explicit() {
        for cursor in [
            "",
            "bad:1",
            &format!("{}:0", "0".repeat(64)),
            &format!("{}:-1", "0".repeat(64)),
            &format!("{}:1000001", "0".repeat(64)),
            &format!("{}:1:2", "0".repeat(64)),
        ] {
            assert!(trade_cursor(Some(cursor)).is_err());
        }
        let value = serde_json::json!({"proof_rejection":true,"tick_replay":null});
        let result = crate::storage::StoredResult {
            outcome: "rejected".into(),
            result_hash: crate::data::hash(&value).unwrap(),
            result: value,
        };
        let page = trade_page(&result, None, 100).unwrap();
        assert_eq!(page["replay_present"], false);
        assert_eq!(page["total_trades"], 0);
        assert!(page["next_cursor"].is_null());
        assert!(page["first_ordinal"].is_null());
    }
}

#[cfg(test)]
mod operations_tests {
    use super::*;
    use crate::{contracts::Profile, data, import_job, native};
    use axum::extract::{Path as Param, State};

    struct OpsFixture {
        root: PathBuf,
        db: PathBuf,
        catalog: PathBuf,
    }
    impl OpsFixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "factory-dashboard-ops-{label}-{}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self {
                db: root.join("state.sqlite"),
                catalog: root.join("catalog.sqlite"),
                root,
            }
        }
        fn import_dir(&self, name: &str) -> PathBuf {
            let dir = self.root.join(name);
            fs::create_dir_all(&dir).unwrap();
            dir
        }
        fn complete_import(&self, name: &str) -> PathBuf {
            let dir = self.import_dir(name);
            let input = self.root.join(format!("{name}.csv"));
            let rows = (0..8)
                .map(|i| format!("{},{},1.00000,1.00010", 1735689600000i64 + i * 60000, i))
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(&input, format!("timestamp_ms,sequence,bid,ask\n{rows}\n")).unwrap();
            let profile = Profile::synthetic("EURUSD").unwrap();
            import_job::step(
                &input,
                &dir,
                profile.clone(),
                "SYNTHETIC_TEST_ONLY dashboard ops fixture",
                true,
                4,
            )
            .unwrap();
            import_job::step(
                &input,
                &dir,
                profile,
                "SYNTHETIC_TEST_ONLY dashboard ops fixture",
                true,
                4,
            )
            .unwrap();
            dir
        }
        fn operations(&self, body: serde_json::Value) -> OperationsConfig {
            let path = self.root.join("operations.json");
            fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
            OperationsConfig::load(&path).unwrap()
        }
        fn app(&self, operations: Option<OperationsConfig>, catalog: bool) -> Arc<App> {
            Arc::new(App {
                registry: Mutex::new(Registry::open(&self.db).unwrap()),
                catalog_path: catalog.then(|| self.catalog.clone()),
                review_config: None,
                operations,
                review_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                recovery_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                recovery_read_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                operations_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                review_bucket: Mutex::new((Instant::now(), 0)),
                operations_bucket: Mutex::new((Instant::now(), 0)),
                password_hash: "fixture-not-a-real-hash".into(),
                sessions: Mutex::new(HashMap::new()),
                login_bucket: Mutex::new((Instant::now(), 0)),
                requests: Mutex::new((Instant::now(), 0)),
                login_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                port: 0,
            })
        }
    }
    impl Drop for OpsFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    async fn json_of(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
    fn config_body(import: &str) -> serde_json::Value {
        serde_json::json!({"imports":[{"name":import,"directory":import}]})
    }

    #[test]
    fn operations_config_enforces_names_duplicates_bounds_and_intervals() {
        let fixture = OpsFixture::new("config");
        let write = |value: serde_json::Value| {
            let path = fixture.root.join("bad-operations.json");
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            OperationsConfig::load(&path)
        };
        assert!(write(serde_json::json!({})).is_err());
        assert!(write(serde_json::json!({"imports":[],"coverage":[]})).is_err());
        assert!(
            write(serde_json::json!({"imports":[{"name":"Bad_Name","directory":"x"}]})).is_err()
        );
        assert!(write(serde_json::json!({"imports":[{"name":"-lead","directory":"x"}]})).is_err());
        assert!(
            write(serde_json::json!({"imports":[{"name":"x".repeat(65),"directory":"x"}]}))
                .is_err()
        );
        assert!(write(serde_json::json!({"imports":[{"name":"a","directory":"x"},{"name":"a","directory":"y"}]})).is_err());
        assert!(write(serde_json::json!({"imports":[{"name":"a","directory":"x"}],"coverage":[{"name":"a","dataset":"d.rsf","start_ms":0,"end_exclusive_ms":1,"gap_threshold_ms":1}]})).is_err());
        assert!(
            write(serde_json::json!({"imports":[{"name":"a","directory":"x","extra":1}]})).is_err()
        );
        assert!(write(serde_json::json!({"coverage":[{"name":"c","dataset":"d.rsf","start_ms":5,"end_exclusive_ms":5,"gap_threshold_ms":1}]})).is_err());
        assert!(write(serde_json::json!({"coverage":[{"name":"c","dataset":"d.rsf","start_ms":0,"end_exclusive_ms":1,"gap_threshold_ms":0}]})).is_err());
        assert!(write(serde_json::json!({"coverage":[{"name":"c","dataset":"d.rsf","start_ms":0,"end_exclusive_ms":1,"gap_threshold_ms":366i64*24*3600*1000+1}]})).is_err());
        let many = (0..17)
            .map(|i| serde_json::json!({"name":format!("import-{i}"),"directory":format!("dir-{i}")}))
            .collect::<Vec<_>>();
        assert!(write(serde_json::json!({"imports":many})).is_err());
        // Valid relative paths resolve against the configuration file's parent.
        let ok = fixture
            .operations(serde_json::json!({"imports":[{"name":"job","directory":"sub/dir"}]}));
        assert_eq!(ok.imports[0].directory, fixture.root.join("sub/dir"));
        assert!(ok.import("job").is_some());
        assert!(ok.import("missing").is_none());
    }

    #[tokio::test]
    async fn import_activation_lifecycle_verifies_confirms_and_registers_observation_only() {
        let fixture = OpsFixture::new("activate");
        let dir = fixture.complete_import("import-complete");
        let config = fixture.operations(config_body("import-complete"));
        let app = fixture.app(Some(config), true);
        let inventory = json_of(imports_inventory(State(app.clone())).await).await;
        assert_eq!(inventory["configured"], true);
        assert_eq!(inventory["imports"][0]["state"], "complete");
        assert_eq!(inventory["imports"][0]["events"], "8");
        assert_eq!(inventory["imports"][0]["chunks"], "2");
        let progress = import_job::status(&dir).unwrap();
        let hash = progress.configuration.archive_hash.clone();
        // Stale or wrong confirmation refuses before any catalog write.
        let wrong_hash = import_activate(
            State(app.clone()),
            Param("import-complete".into()),
            Json(ImportActivate {
                expected_archive_hash: "0".repeat(64),
                expected_events: "8".into(),
            }),
        )
        .await;
        assert_eq!(wrong_hash.status(), StatusCode::CONFLICT);
        let wrong_events = import_activate(
            State(app.clone()),
            Param("import-complete".into()),
            Json(ImportActivate {
                expected_archive_hash: hash.clone(),
                expected_events: "9".into(),
            }),
        )
        .await;
        assert_eq!(wrong_events.status(), StatusCode::CONFLICT);
        assert!(!fixture.catalog.exists());
        let activated = import_activate(
            State(app.clone()),
            Param("import-complete".into()),
            Json(ImportActivate {
                expected_archive_hash: hash.clone(),
                expected_events: "8".into(),
            }),
        )
        .await;
        assert_eq!(activated.status(), StatusCode::OK);
        let value = json_of(activated).await;
        assert_eq!(value["decision"]["kind"], "activated");
        assert_eq!(value["decision"]["verification"], "verified");
        assert_eq!(value["decision"]["declared_class"], "SyntheticTestOnly");
        assert_eq!(value["reused"], false);
        assert_eq!(value["registration"]["receipt"]["facts"]["events"], 8);
        let identity = value["registration"]["receipt"]["facts"]["dataset_identity"]
            .as_str()
            .unwrap()
            .to_string();
        let entry = crate::catalog::Catalog::existing(&fixture.catalog)
            .unwrap()
            .entry(&identity)
            .unwrap();
        assert!(entry.is_some());
        // Repeat activation is idempotent and retained as a second decision.
        let again = json_of(
            import_activate(
                State(app.clone()),
                Param("import-complete".into()),
                Json(ImportActivate {
                    expected_archive_hash: hash,
                    expected_events: "8".into(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(again["reused"], true);
        let decisions = read_decisions(&dir).unwrap();
        assert_eq!(decisions.len(), 2);
        assert!(decisions.iter().all(|d| d.kind == "activated"));
        let detail =
            json_of(import_detail(State(app.clone()), Param("import-complete".into())).await).await;
        assert_eq!(detail["state"], "complete");
        assert_eq!(detail["decisions"].as_array().unwrap().len(), 2);
        assert!(
            !serde_json::to_string(&detail)
                .unwrap()
                .contains(fixture.root.to_str().unwrap())
        );
    }

    #[tokio::test]
    async fn import_refusal_retains_evidence_and_preserves_artifacts() {
        let fixture = OpsFixture::new("refuse");
        let dir = fixture.complete_import("import-refused");
        let config = fixture.operations(config_body("import-refused"));
        let app = fixture.app(Some(config), true);
        let progress = import_job::status(&dir).unwrap();
        let hash = progress.configuration.archive_hash.clone();
        let chunk = fs::read(dir.join("chunk-000000.rsf")).unwrap();
        let refused = import_refuse(
            State(app.clone()),
            Param("import-refused".into()),
            Json(ImportRefuse {
                expected_archive_hash: hash.clone(),
                reason: "source revision superseded; keep for audit".into(),
            }),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::OK);
        let value = json_of(refused).await;
        assert_eq!(value["decision"]["kind"], "refused");
        assert_eq!(value["decision"]["verification"], "verified");
        assert_eq!(value["activation_performed"], false);
        assert_eq!(fs::read(dir.join("chunk-000000.rsf")).unwrap(), chunk);
        assert!(!fixture.catalog.exists());
        let inventory = json_of(imports_inventory(State(app.clone())).await).await;
        assert_eq!(inventory["imports"][0]["last_decision"]["kind"], "refused");
        assert_eq!(inventory["imports"][0]["decision_count"], "1");
        let mismatch = import_refuse(
            State(app.clone()),
            Param("import-refused".into()),
            Json(ImportRefuse {
                expected_archive_hash: "0".repeat(64),
                reason: "stale".into(),
            }),
        )
        .await;
        assert_eq!(mismatch.status(), StatusCode::CONFLICT);
        // Activation remains possible after a refusal; history keeps both decisions.
        let activated = import_activate(
            State(app.clone()),
            Param("import-refused".into()),
            Json(ImportActivate {
                expected_archive_hash: hash,
                expected_events: "8".into(),
            }),
        )
        .await;
        assert_eq!(activated.status(), StatusCode::OK);
        assert_eq!(read_decisions(&dir).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn tampered_chunk_refuses_inventory_detail_and_activation() {
        let fixture = OpsFixture::new("tamper");
        let dir = fixture.complete_import("import-tampered");
        let config = fixture.operations(config_body("import-tampered"));
        let app = fixture.app(Some(config), true);
        let progress = import_job::status(&dir).unwrap();
        let hash = progress.configuration.archive_hash.clone();
        let path = dir.join("chunk-000000.rsf");
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();
        let inventory = json_of(imports_inventory(State(app.clone())).await).await;
        assert_eq!(inventory["imports"][0]["state"], "refused");
        assert!(
            inventory["imports"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("mismatch")
        );
        let detail = import_detail(State(app.clone()), Param("import-tampered".into())).await;
        assert_eq!(detail.status(), StatusCode::CONFLICT);
        let activation = import_activate(
            State(app.clone()),
            Param("import-tampered".into()),
            Json(ImportActivate {
                expected_archive_hash: hash,
                expected_events: "8".into(),
            }),
        )
        .await;
        assert_eq!(activation.status(), StatusCode::CONFLICT);
        assert!(!fixture.catalog.exists());
        // Operator refusal of an unverified import is retained with its exact obstacle.
        let refusal = import_refuse(
            State(app.clone()),
            Param("import-tampered".into()),
            Json(ImportRefuse {
                expected_archive_hash: "0".repeat(64),
                reason: "tampered chunk retained for investigation".into(),
            }),
        )
        .await;
        assert_eq!(refusal.status(), StatusCode::OK);
        let value = json_of(refusal).await;
        assert_eq!(value["decision"]["verification"], "unverified");
        assert!(
            value["decision"]["verification_error"]
                .as_str()
                .unwrap()
                .contains("mismatch")
        );
    }

    #[tokio::test]
    async fn corrupt_decision_history_refuses_actions_and_uninitialized_imports_report_no_state() {
        let fixture = OpsFixture::new("corrupt");
        let dir = fixture.complete_import("import-corrupt");
        fixture.import_dir("import-empty");
        let config = fixture.operations(serde_json::json!({"imports":[
            {"name":"import-corrupt","directory":"import-corrupt"},
            {"name":"import-empty","directory":"import-empty"}
        ]}));
        let app = fixture.app(Some(config), true);
        fs::write(dir.join(DECISION_FILE), "{\"forged\":true}\n").unwrap();
        let inventory = json_of(imports_inventory(State(app.clone())).await).await;
        assert_eq!(inventory["imports"][0]["decision_history"], "corrupt");
        assert_eq!(inventory["imports"][1]["state"], "not_initialized");
        let progress = import_job::status(&dir).unwrap();
        let activation = import_activate(
            State(app.clone()),
            Param("import-corrupt".into()),
            Json(ImportActivate {
                expected_archive_hash: progress.configuration.archive_hash,
                expected_events: "8".into(),
            }),
        )
        .await;
        assert_eq!(activation.status(), StatusCode::CONFLICT);
        let refusal = import_refuse(
            State(app.clone()),
            Param("import-corrupt".into()),
            Json(ImportRefuse {
                expected_archive_hash: "0".repeat(64),
                reason: "history corrupt".into(),
            }),
        )
        .await;
        assert_eq!(refusal.status(), StatusCode::CONFLICT);
        let missing_detail = import_detail(State(app.clone()), Param("import-empty".into())).await;
        assert_eq!(missing_detail.status(), StatusCode::CONFLICT);
        let unknown = import_detail(State(app.clone()), Param("not-configured".into())).await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        // Decision history budget refuses instead of truncating retained evidence.
        let budget_dir = fixture.import_dir("import-budget");
        fs::write(
            budget_dir.join(DECISION_FILE),
            "x".repeat(DECISION_LIMIT * DECISION_LINE_LIMIT + 1),
        )
        .unwrap();
        assert!(read_decisions(&budget_dir).is_err());
    }

    #[tokio::test]
    async fn activation_requires_catalog_and_valid_confirmation_fields() {
        let fixture = OpsFixture::new("nocatalog");
        fixture.complete_import("import-nocatalog");
        let config = fixture.operations(config_body("import-nocatalog"));
        let app = fixture.app(Some(config), false);
        let progress = import_job::status(&fixture.root.join("import-nocatalog")).unwrap();
        let refused = import_activate(
            State(app.clone()),
            Param("import-nocatalog".into()),
            Json(ImportActivate {
                expected_archive_hash: progress.configuration.archive_hash,
                expected_events: "8".into(),
            }),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::CONFLICT);
        let app = fixture.app(
            Some(fixture.operations(config_body("import-nocatalog"))),
            true,
        );
        for events in ["08", "-1", "", "2000001", "1e3", "99999999"] {
            let bad = import_activate(
                State(app.clone()),
                Param("import-nocatalog".into()),
                Json(ImportActivate {
                    expected_archive_hash: "0".repeat(64),
                    expected_events: events.into(),
                }),
            )
            .await;
            assert_eq!(bad.status(), StatusCode::BAD_REQUEST, "events {events}");
        }
        assert_eq!(canonical_events("2000000"), Some(2000000));
        assert_eq!(canonical_events("0"), Some(0));
        assert_eq!(canonical_events("08"), None);
    }

    #[tokio::test]
    async fn coverage_reports_are_exact_bounded_and_labelled_observations() {
        let fixture = OpsFixture::new("coverage");
        let dataset = data::synthetic_fixture("EURUSD").unwrap();
        let artifact = fixture.root.join("coverage.rsf");
        native::write(&artifact, &dataset).unwrap();
        let config = fixture.operations(serde_json::json!({"coverage":[
            {"name":"month-window","dataset":"coverage.rsf","start_ms":1735689600000i64,"end_exclusive_ms":1738368000000i64,"gap_threshold_ms":60000},
            {"name":"full-span","dataset":"coverage.rsf","start_ms":1735689600000i64,"end_exclusive_ms":1769904000000i64,"gap_threshold_ms":60000},
            {"name":"dense-gaps","dataset":"coverage.rsf","start_ms":1735689600000i64,"end_exclusive_ms":1769904000000i64,"gap_threshold_ms":1}
        ]}));
        let app = fixture.app(Some(config), false);
        let inventory = json_of(coverage_inventory(State(app.clone())).await).await;
        assert_eq!(inventory["configured"], true);
        assert_eq!(inventory["targets"].as_array().unwrap().len(), 3);
        assert_eq!(inventory["targets"][0]["start_ms"], "1735689600000");
        let month =
            json_of(coverage_report(State(app.clone()), Param("month-window".into())).await).await;
        assert_eq!(month["events"], 1000);
        assert_eq!(month["months"].as_array().unwrap().len(), 1);
        assert_eq!(month["gap_count"], 0);
        assert_eq!(month["name"], "month-window");
        let span =
            json_of(coverage_report(State(app.clone()), Param("full-span".into())).await).await;
        assert_eq!(span["events"], 2000);
        assert_eq!(span["months"].as_array().unwrap().len(), 13);
        assert_eq!(span["gap_count"], 1);
        assert_eq!(span["gaps"][0]["elapsed_ms"], 31526010000i64);
        assert_eq!(span["gap_details_truncated"], false);
        assert!(
            span["limitations"]
                .as_str()
                .unwrap()
                .contains("No acquisition")
        );
        let dense =
            json_of(coverage_report(State(app.clone()), Param("dense-gaps".into())).await).await;
        assert_eq!(dense["gap_count"], 1999);
        assert_eq!(dense["gaps"].as_array().unwrap().len(), 100);
        assert_eq!(dense["gap_details_truncated"], true);
        let unknown = coverage_report(State(app.clone()), Param("missing".into())).await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        // A missing configured artifact refuses; no coverage is fabricated.
        fs::remove_file(&artifact).unwrap();
        let missing = coverage_report(State(app.clone()), Param("month-window".into())).await;
        assert_eq!(missing.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn registry_audit_observes_integrity_and_refuses_corruption() {
        let fixture = OpsFixture::new("audit");
        let app = fixture.app(None, false);
        let healthy = registry_audit(State(app.clone())).await;
        assert_eq!(healthy.status(), StatusCode::OK);
        let value = json_of(healthy).await;
        assert_eq!(value["integrity"], "ok");
        assert_eq!(value["counts"]["registered"], 0);
        assert!(
            value["scope"]
                .as_str()
                .unwrap()
                .contains("not a continuous audit")
                || value["scope"]
                    .as_str()
                    .unwrap()
                    .contains("Not a continuous audit")
        );
        {
            let registry = app.registry.lock().unwrap();
            registry.connection.execute(
                "INSERT INTO experiments(id,key_json,state,result_json,result_hash) VALUES('corrupt','{}','completed','{}',?1)",
                rusqlite::params!["0".repeat(64)],
            ).unwrap();
        }
        let corrupt = registry_audit(State(app.clone())).await;
        assert_eq!(corrupt.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn resource_observations_extend_with_exact_peak_and_cpu_strings() {
        let response = resource_status().await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = json_of(response).await;
        for field in [
            "process_rss_bytes",
            "host_available_memory_bytes",
            "available_parallelism",
            "peak_rss_bytes",
            "process_user_ticks",
            "process_system_ticks",
            "process_start_ticks",
        ] {
            assert!(
                value[field].is_string(),
                "field {field} must stay an exact string"
            );
        }
        assert!(
            value["peak_rss_bytes"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert!(
            value["cpu_scope"]
                .as_str()
                .unwrap()
                .contains("not milliseconds")
        );
    }

    #[test]
    fn redaction_keeps_exact_reasons_without_local_paths() {
        let paths = vec!["/tmp/private/import-dir".to_string()];
        let text = "committed import chunk mismatch at /tmp/private/import-dir/chunk-000000.rsf";
        let redacted = redact(text, &paths);
        assert!(redacted.contains("committed import chunk mismatch"));
        assert!(redacted.contains("[redacted-path]"));
        assert!(!redacted.contains("/tmp/private"));
    }

    #[tokio::test]
    async fn unconfigured_operations_report_explicit_absence() {
        let fixture = OpsFixture::new("unconfigured");
        let app = fixture.app(None, false);
        let imports = json_of(imports_inventory(State(app.clone())).await).await;
        assert_eq!(imports["configured"], false);
        let coverage = json_of(coverage_inventory(State(app.clone())).await).await;
        assert_eq!(coverage["configured"], false);
        let detail = import_detail(State(app.clone()), Param("any".into())).await;
        assert_eq!(detail.status(), StatusCode::NOT_FOUND);
        let report = coverage_report(State(app.clone()), Param("any".into())).await;
        assert_eq!(report.status(), StatusCode::NOT_FOUND);
    }
}
