//! Loopback-only dashboard. Official metrics are read-only; browser controls require session CSRF.
use crate::{Result, research, storage::Registry};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as Param, Request, State},
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
    path::Path,
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
            serde_json::json!({"software_status":"SOFTWARE_INCOMPLETE","historical_readiness":"FULL_HISTORICAL_DATA_PENDING","qualification":"Independent validation/profile/coverage evidence is incomplete; no qualified strategies","counts":registry.counts()?,"runs":runs,"engine":crate::ENGINE_VERSION,"indicators":crate::INDICATOR_VERSION}),
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
async fn experiments(State(app): State<Arc<App>>) -> Response {
    let Ok(registry) = app.registry.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    };
    let load = || -> Result<Vec<serde_json::Value>> {
        let mut q=registry.connection.prepare("SELECT id,state,key_json,json_extract(result_json,'$.data_manifest'),json_extract(result_json,'$.strategy'),json_array_length(result_json,'$.tick_replay.trades'),CASE WHEN typeof(json_extract(result_json,'$.tick_replay.realized_micro_usd'))='integer' AND json_extract(result_json,'$.tick_replay.realized_micro_usd') BETWEEN -9007199254740991 AND 9007199254740991 THEN json_extract(result_json,'$.tick_replay.realized_micro_usd') ELSE NULL END FROM experiments ORDER BY rowid DESC LIMIT 100").map_err(|e|e.to_string())?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row|{let (id,state,key,manifest,strategy,trades,equity)=row.map_err(|e|e.to_string())?;Ok(serde_json::json!({"id":id,"state":state,"key":serde_json::from_str::<serde_json::Value>(&key).map_err(|e|e.to_string())?,"manifest":manifest.and_then(|v|serde_json::from_str::<serde_json::Value>(&v).ok()),"strategy":strategy.and_then(|v|serde_json::from_str::<serde_json::Value>(&v).ok()),"closed_trades":trades,"ending_micro_usd":equity}))}).collect()
    };
    match load() {
        Ok(v) => Json(v).into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read persisted experiments",
        ),
    }
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
                let cutoff = v.result["data_manifest"]["last_ms"]
                    .as_i64()
                    .ok_or("dataset cutoff missing")?;
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
            Json(serde_json::json!({"outcome":v.outcome,"result":v.result,"result_hash":v.result_hash,"numeric_training_assessment":assessment,"assessment_unavailable_reason":error})).into_response()
        }
        Ok(None) => api_error(StatusCode::NOT_FOUND, "result not completed"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "result integrity check failed",
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
    if port != 0 && port < 1024 {
        return Err("use an unprivileged loopback port or zero for allocation".into());
    }
    if !database.is_file() {
        return Err("registry must already exist".into());
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
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let app = Arc::new(App {
        registry: Mutex::new(Registry::open(database)?),
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
        .route("/api/experiments", get(experiments))
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
