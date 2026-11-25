//! Durable exact experiment identities and fenced, restart-safe leases.
use crate::{Result, contracts::SYMBOLS, data::hash};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    CandleScreen,
    TickVerification,
    IndependentValidation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentKey {
    pub concept_hash: String,
    pub instrument: String,
    pub dataset_hash: String,
    pub start_ms: i64,
    pub end_exclusive_ms: i64,
    pub signal_version: String,
    pub indicator_version: String,
    pub cost_hash: String,
    pub fill_policy: String,
    pub profile_hash: String,
    pub engine_version: String,
    pub protocol_hash: String,
    pub code_revision: String,
    pub configuration_hash: String,
    pub stage: Stage,
}
impl ExperimentKey {
    pub fn identity(&self) -> Result<String> {
        if !SYMBOLS.contains(&self.instrument.as_str())
            || self.start_ms >= self.end_exclusive_ms
            || [
                &self.concept_hash,
                &self.dataset_hash,
                &self.signal_version,
                &self.indicator_version,
                &self.cost_hash,
                &self.fill_policy,
                &self.profile_hash,
                &self.engine_version,
                &self.protocol_hash,
                &self.code_revision,
                &self.configuration_hash,
            ]
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256)
        {
            return Err("incomplete experiment identity".into());
        }
        hash(&("experiment-v1", self))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Completed,
    Rejected,
    Invalid,
    Inconclusive,
    Failed,
}
impl Outcome {
    fn name(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::Invalid => "invalid",
            Self::Inconclusive => "inconclusive",
            Self::Failed => "failed",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub experiment_id: String,
    pub token: String,
    pub expires_ms: i64,
    pub attempt: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredResult {
    pub outcome: String,
    pub result: serde_json::Value,
    pub result_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Claim {
    Acquired(Lease),
    Busy,
    Reused(StoredResult),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Counts {
    pub registered: i64,
    pub completed: i64,
    pub running: i64,
    pub attempts: i64,
}
/// Bounded immutable-ID keyset page; record status remains an observation, not PID liveness.
#[derive(Debug, Serialize)]
pub struct ProcessHistoryPage {
    pub rows: Vec<serde_json::Value>,
    pub next_cursor: Option<String>,
    pub maximum_page_size: &'static str,
    pub maximum_unclosed_owners: &'static str,
}
pub(crate) enum ProcessHistory {
    Owned,
    Supervision,
}
impl ProcessHistory {
    fn table(&self) -> &'static str {
        match self {
            Self::Owned => "corpus_owned_invocations",
            Self::Supervision => "corpus_supervision_sessions",
        }
    }
}
pub(crate) fn process_history_ids(
    registry: &Registry,
    kind: ProcessHistory,
    after: Option<&str>,
    limit: usize,
) -> Result<(Vec<String>, Option<String>)> {
    if !(1..=128).contains(&limit) {
        return Err("process history page limit must be1..128".into());
    }
    let table = kind.table();
    let before: Option<i64> = if let Some(id) = after {
        if !crate::machine_evidence::hex(id) {
            return Err("invalid process history cursor".into());
        }
        Some(
            registry
                .connection
                .query_row(
                    &format!("SELECT rowid FROM {table} WHERE id=?1"),
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_err)?
                .ok_or("unknown process history cursor")?,
        )
    } else {
        None
    };
    // Separate SQL keeps the after-page query a rowid range, without an OR predicate.
    let mut ids: Vec<String> = if let Some(before) = before {
        let mut query = registry
            .connection
            .prepare(&format!(
                "SELECT CASE WHEN length(CAST(id AS BLOB))=64 THEN id END FROM {table} WHERE rowid < ?1 ORDER BY rowid DESC LIMIT ?2"
            ))
            .map_err(db_err)?;
        query
            .query_map(params![before, limit + 1], |row| row.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?
    } else {
        let mut query = registry
            .connection
            .prepare(&format!(
                "SELECT CASE WHEN length(CAST(id AS BLOB))=64 THEN id END FROM {table} ORDER BY rowid DESC LIMIT ?1"
            ))
            .map_err(db_err)?;
        query
            .query_map([limit + 1], |row| row.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?
    };
    if ids.iter().any(|id| !crate::machine_evidence::hex(id)) {
        return Err("invalid process history identity".into());
    }
    let more = ids.len() > limit;
    ids.truncate(limit);
    let next = if more { ids.last().cloned() } else { None };
    Ok((ids, next))
}
pub const SCHEMA_VERSION: u32 = 24;
pub struct Registry {
    pub(crate) connection: Connection,
    pub(crate) writer_gate: std::sync::Arc<WriterGate>,
}
fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}
// ---------------------------------------------------------------------------
// Bounded writer admission (implementation queue item 9).
//
// SQLite WAL serializes writer transactions per database file. Previously every
// writer queued blindly inside SQLite's busy handler for up to the connection
// busy timeout, then surfaced an unclassified "database is locked" error that
// could abort an otherwise healthy worker. Writer admission now bounds how many
// writer transactions one process admits per database file: excess callers wait
// within an explicit budget and are then refused with the typed registry
// admission busy classification (schema12), which the frozen opt-in retry
// policy (schema13) already understands. Transient SQLite lock contention on
// BEGIN IMMEDIATE is retried a bounded number of times with backoff; permanent
// errors stay typed and are never retried.
//
// Declared scope and limitations (not measured OS enforcement):
// - The gate is per process and per database file. Cross-process writers still
//   serialize through SQLite's WAL write lock; the connection busy timeout plus
//   bounded typed retries absorb that contention.
// - Observations are runtime counters, not persisted schema. They live while a
//   registry handle on the file is open in this process and reset on restart.
// - Waits are cooperative wall-clock budgets; a descheduled thread can exceed
//   them, exactly like the existing sampled resource observations.
// - Readers are not gated: WAL readers do not block on writers. Backpressure
//   bounds writer queueing so worker threads cannot pile up in the busy handler.
// ---------------------------------------------------------------------------
/// Maximum writer transactions admitted concurrently per database file per process.
pub const WRITER_ADMISSION_CAPACITY: usize = 2;
/// Default bounded wait for an admission slot before typed refusal.
pub const WRITER_ADMISSION_WAIT_MS: u64 = 2_000;
/// Bounded retries of BEGIN IMMEDIATE after the busy timeout reports typed busy.
pub const WRITER_LOCK_RETRIES: u32 = 3;
/// Busy timeout of registry connections. Admission above bounds the queue, so a
/// shorter timeout plus bounded typed retry replaces the old blind 30 s wait.
const WRITER_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Startup admission refuses faster: a locked registry must not start workers.
const OPEN_GATE_WAIT_MS: u64 = 250;
#[derive(Debug, Clone, Copy)]
pub(crate) struct WriterPolicy {
    pub gate_wait_ms: u64,
    pub lock_retries: u32,
}
impl WriterPolicy {
    fn validate(self) -> Result<()> {
        if self.gate_wait_ms > 60_000 || self.lock_retries > 8 {
            return Err("writer admission policy outside declared bounds".into());
        }
        Ok(())
    }
}
pub(crate) const WRITE_POLICY: WriterPolicy = WriterPolicy {
    gate_wait_ms: WRITER_ADMISSION_WAIT_MS,
    lock_retries: WRITER_LOCK_RETRIES,
};
const OPEN_POLICY: WriterPolicy = WriterPolicy {
    gate_wait_ms: OPEN_GATE_WAIT_MS,
    // Startup stays bounded but patient enough for a healthy worker to open a
    // momentarily locked registry: 5 s busy timeout x (1 + 2 retries) + backoff.
    lock_retries: 2,
};
/// Short-wait policy for the parent supervisor's own bookkeeping writes; the
/// caller owns the bounded retry loop and the connection a 10 ms busy timeout.
pub(crate) const SUPERVISED_POLICY: WriterPolicy = WriterPolicy {
    gate_wait_ms: 100,
    lock_retries: 0,
};
#[derive(Debug, Default)]
struct GateState {
    in_flight: usize,
    admitted: u64,
    refused: u64,
    waited: u64,
    wait_ms_total: u64,
    busy_retries: u64,
    max_in_flight: usize,
}
/// Bounded counting admission gate shared by every connection this process
/// opens on one database file. Nothing here is persisted or schema-visible.
pub struct WriterGate {
    id: u64,
    state: std::sync::Mutex<GateState>,
    ready: std::sync::Condvar,
}
static NEXT_GATE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static WRITER_GATES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::sync::Weak<WriterGate>>>,
> = std::sync::OnceLock::new();
impl WriterGate {
    fn new() -> Self {
        Self {
            id: NEXT_GATE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            state: std::sync::Mutex::new(GateState::default()),
            ready: std::sync::Condvar::new(),
        }
    }
    /// Cumulative runtime observations with exact string counters.
    pub fn observe(&self) -> serde_json::Value {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        serde_json::json!({
            "capacity": WRITER_ADMISSION_CAPACITY.to_string(),
            "default_wait_budget_ms": WRITER_ADMISSION_WAIT_MS.to_string(),
            "lock_retries": WRITER_LOCK_RETRIES.to_string(),
            "busy_timeout_ms": WRITER_BUSY_TIMEOUT.as_millis().to_string(),
            "in_flight": s.in_flight.to_string(),
            "admitted": s.admitted.to_string(),
            "refused": s.refused.to_string(),
            "waited": s.waited.to_string(),
            "wait_ms_total": s.wait_ms_total.to_string(),
            "busy_retries": s.busy_retries.to_string(),
            "max_in_flight": s.max_in_flight.to_string(),
            "limitations": "Declared in-process admission per database file; cross-process writers serialize through SQLite WAL. Runtime counters only, not persisted; not measured OS enforcement.",
        })
    }
}
thread_local! {
    /// Reentrancy depth per gate for the current thread: a nested writer
    /// transaction on the same connection must not deadlock on its own slot.
    /// Depth is recorded only after a slot is actually admitted, and every
    /// guard (outermost or reentrant) decrements it on drop, so a refusal can
    /// never leave a stale depth that bypasses admission later.
    static HELD_GATES: std::cell::RefCell<std::collections::HashMap<u64, usize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
/// Resolve (or create) the shared gate for a connection's database file.
/// In-memory databases are per-connection and get a private gate.
pub(crate) fn writer_gate_for(path: Option<&str>) -> std::sync::Arc<WriterGate> {
    let Some(path) = path.filter(|p| !p.is_empty() && *p != ":memory:") else {
        return std::sync::Arc::new(WriterGate::new());
    };
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
    let map = WRITER_GATES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = map.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(gate) = map.get(&canonical).and_then(std::sync::Weak::upgrade) {
        return gate;
    }
    let gate = std::sync::Arc::new(WriterGate::new());
    map.insert(canonical, std::sync::Arc::downgrade(&gate));
    // Bounded housekeeping: drop dead entries once the map grows large.
    if map.len() > 4096 {
        map.retain(|_, weak| weak.strong_count() > 0);
    }
    gate
}
/// Admission guard: releases the slot (or reentrancy depth) on drop.
pub(crate) struct WriterGuard {
    gate: std::sync::Arc<WriterGate>,
    reentrant: bool,
}
impl Drop for WriterGuard {
    fn drop(&mut self) {
        HELD_GATES.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(depth) = held.get_mut(&self.gate.id) {
                *depth -= 1;
                if *depth == 0 {
                    held.remove(&self.gate.id);
                }
            }
        });
        if self.reentrant {
            return;
        }
        let mut state = self.gate.state.lock().unwrap_or_else(|p| p.into_inner());
        state.in_flight = state.in_flight.saturating_sub(1);
        drop(state);
        self.gate.ready.notify_all();
    }
}
fn admit(
    gate: &std::sync::Arc<WriterGate>,
    policy: WriterPolicy,
) -> std::result::Result<WriterGuard, crate::corpus_failure::AdmissionFailure> {
    // A nested writer on this thread already holds a slot for this gate; it
    // must not wait on itself. SQLite itself refuses a nested BEGIN, so the
    // reentrant path only bypasses the gate, never the database's own rules.
    let reentrant = HELD_GATES.with(|held| {
        let mut held = held.borrow_mut();
        match held.get_mut(&gate.id) {
            Some(depth) => {
                *depth += 1;
                true
            }
            None => false,
        }
    });
    if reentrant {
        return Ok(WriterGuard {
            gate: gate.clone(),
            reentrant: true,
        });
    }
    let started = std::time::Instant::now();
    let deadline = started + Duration::from_millis(policy.gate_wait_ms);
    let mut state = gate.state.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if state.in_flight < WRITER_ADMISSION_CAPACITY {
            state.in_flight += 1;
            state.admitted = state.admitted.saturating_add(1);
            state.max_in_flight = state.max_in_flight.max(state.in_flight);
            let waited_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            if waited_ms > 0 {
                state.waited = state.waited.saturating_add(1);
                state.wait_ms_total = state.wait_ms_total.saturating_add(waited_ms);
            }
            drop(state);
            HELD_GATES.with(|held| held.borrow_mut().insert(gate.id, 1));
            return Ok(WriterGuard {
                gate: gate.clone(),
                reentrant: false,
            });
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            state.refused = state.refused.saturating_add(1);
            let message = format!(
                "registry writer admission refused: {} writers in flight (cap {}), waited {} ms of {} ms budget",
                state.in_flight,
                WRITER_ADMISSION_CAPACITY,
                u64::try_from(now.duration_since(started).as_millis()).unwrap_or(u64::MAX),
                policy.gate_wait_ms
            );
            drop(state);
            let failure = crate::corpus_failure::AdmissionFailure {
                kind: crate::corpus_failure::Kind::RegistryAdmissionBusy,
                message,
            };
            crate::corpus_failure::record_admission_failure(failure.clone());
            return Err(failure);
        }
        state = gate
            .ready
            .wait_timeout(state, deadline.saturating_duration_since(now))
            .unwrap_or_else(|p| p.into_inner())
            .0;
    }
}
/// A writer transaction admitted through the bounded gate. Derefs to the
/// rusqlite transaction; dropping it rolls back and releases the slot.
pub(crate) struct WriterTransaction<'a> {
    tx: rusqlite::Transaction<'a>,
    _guard: WriterGuard,
}
impl std::fmt::Debug for WriterTransaction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterTransaction").finish_non_exhaustive()
    }
}
impl<'a> std::ops::Deref for WriterTransaction<'a> {
    type Target = rusqlite::Transaction<'a>;
    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}
impl<'a> std::ops::DerefMut for WriterTransaction<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tx
    }
}
impl WriterTransaction<'_> {
    pub(crate) fn commit(self) -> std::result::Result<(), rusqlite::Error> {
        let result = self.tx.commit();
        if let Err(error) = &result {
            let failure = crate::corpus_failure::AdmissionFailure::from_sqlite(error);
            if failure.kind == crate::corpus_failure::Kind::RegistryAdmissionBusy {
                crate::corpus_failure::record_admission_failure(failure);
            }
        }
        result
    }
}
/// Admit one writer transaction with the default bounded wait/retry policy.
/// Errors are String-typed for existing call sites; the typed admission cause
/// remains recoverable through `corpus_failure::classify_error`.
pub(crate) fn writer_tx(c: &Connection) -> Result<WriterTransaction<'_>> {
    writer_tx_classified(c).map_err(|e| e.to_string())
}
pub(crate) fn writer_tx_classified(
    c: &Connection,
) -> std::result::Result<WriterTransaction<'_>, crate::corpus_failure::AdmissionFailure> {
    writer_tx_with_policy(c, WRITE_POLICY)
}
pub(crate) fn writer_tx_open(
    c: &Connection,
) -> std::result::Result<WriterTransaction<'_>, crate::corpus_failure::AdmissionFailure> {
    writer_tx_with_policy(c, OPEN_POLICY)
}
/// Supervisor bookkeeping (owned invocation journal) keeps the parent sampling
/// loop responsive: a short gate wait and no extra BEGIN retries, because the
/// supervisor connection already carries a 10 ms busy timeout and its caller
/// (`owned_process::write_with_retry`) owns the bounded retry loop.
pub(crate) fn writer_tx_supervised(c: &Connection) -> Result<WriterTransaction<'_>> {
    writer_tx_with_policy(c, SUPERVISED_POLICY).map_err(|e| e.to_string())
}
pub(crate) fn writer_tx_with_policy(
    c: &Connection,
    policy: WriterPolicy,
) -> std::result::Result<WriterTransaction<'_>, crate::corpus_failure::AdmissionFailure> {
    policy.validate()?;
    let gate = writer_gate_for(c.path());
    let guard = admit(&gate, policy)?;
    let mut attempt = 0u32;
    loop {
        // `new_unchecked` issues BEGIN IMMEDIATE on a shared connection borrow
        // (rusqlite's `transaction_with_behavior` only adds a compile-time
        // nesting guard; SQLite itself still refuses a nested BEGIN, and the
        // gate's reentrancy tracking keeps a nested writer from deadlocking on
        // its own slot). The shared borrow lets the bounded retry loop re-attempt
        // without conflicting with the returned transaction's lifetime.
        match rusqlite::Transaction::new_unchecked(c, TransactionBehavior::Immediate) {
            Ok(tx) => return Ok(WriterTransaction { tx, _guard: guard }),
            Err(error) => {
                let failure = crate::corpus_failure::AdmissionFailure::from_sqlite(&error);
                if failure.kind == crate::corpus_failure::Kind::RegistryAdmissionBusy
                    && attempt < policy.lock_retries
                {
                    attempt += 1;
                    {
                        let mut state = gate.state.lock().unwrap_or_else(|p| p.into_inner());
                        state.busy_retries = state.busy_retries.saturating_add(1);
                    }
                    std::thread::sleep(Duration::from_millis(25 << attempt.min(4)));
                    continue;
                }
                crate::corpus_failure::record_admission_failure(failure.clone());
                return Err(failure);
            }
        }
    }
}
/// Admission guard for a single autocommit write statement on the same gate.
pub(crate) fn writer_guard(c: &Connection) -> Result<WriterGuard> {
    let gate = writer_gate_for(c.path());
    admit(&gate, WRITE_POLICY).map_err(|e| e.to_string())
}
/// Wrap an existing connection (read-only recovery snapshots, fixtures) with
/// its per-file admission gate.
pub(crate) fn wrap_connection(connection: Connection) -> Registry {
    let writer_gate = writer_gate_for(connection.path());
    Registry {
        connection,
        writer_gate,
    }
}
/// String-typed initializers keep their typed admission cause through the
/// provenance cell. Exact message equality only correlates the recorded
/// failure with the propagated error; the kind itself is never parsed from
/// human text (schema12 rule).
fn init_classified(
    result: Result<()>,
) -> std::result::Result<(), crate::corpus_failure::AdmissionFailure> {
    result.map_err(|message| {
        crate::corpus_failure::take_admission_failure()
            .filter(|failure| failure.message == message)
            .unwrap_or_else(|| crate::corpus_failure::AdmissionFailure::from(message))
    })
}
impl Registry {
    /// Runtime writer-admission observations for this registry's database file.
    /// Declared in-process caps and cumulative counters; not persisted schema
    /// and not measured OS enforcement.
    pub fn writer_admission(&self) -> serde_json::Value {
        self.writer_gate.observe()
    }
}
impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_classified(path).map_err(|e| e.to_string())
    }
    pub fn open_classified(
        path: &Path,
    ) -> std::result::Result<Self, crate::corpus_failure::AdmissionFailure> {
        let mut connection =
            Connection::open(path).map_err(crate::corpus_failure::AdmissionFailure::from)?;
        connection
            // WAL serializes writers. Writer admission (above) now bounds the
            // in-process write queue per database file, so the busy timeout no
            // longer needs to absorb an unbounded queue blindly: a shorter
            // timeout plus bounded typed retries keeps contention typed and
            // bounded instead of stalling workers for half a minute.
            .busy_timeout(WRITER_BUSY_TIMEOUT)
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
            )
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        if version > i64::from(SCHEMA_VERSION) {
            return Err("database schema newer than supported".into());
        }
        if version == 0 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS experiments (
                id TEXT PRIMARY KEY, key_json TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('queued','running','completed','rejected','invalid','inconclusive','failed')),
                token TEXT, expires_ms INTEGER, attempt INTEGER NOT NULL DEFAULT 0,
                result_json TEXT, result_hash TEXT,
                CHECK ((state IN ('queued','running') AND result_json IS NULL AND result_hash IS NULL)
                    OR (state NOT IN ('queued','running') AND result_json IS NOT NULL AND result_hash IS NOT NULL))
            );
            CREATE TABLE IF NOT EXISTS events (
                event_id INTEGER PRIMARY KEY, experiment_id TEXT NOT NULL REFERENCES experiments(id),
                at_ms INTEGER NOT NULL, action TEXT NOT NULL, attempt INTEGER NOT NULL
            );
            PRAGMA user_version=1;").map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        if version < 2 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS holdouts (
                dataset_hash TEXT PRIMARY KEY, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                protocol_hash TEXT NOT NULL, selected_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS holdout_exposures (
                dataset_hash TEXT NOT NULL REFERENCES holdouts(dataset_hash), concept_hash TEXT NOT NULL,
                experiment_id TEXT NOT NULL REFERENCES experiments(id),
                PRIMARY KEY(dataset_hash,concept_hash), UNIQUE(experiment_id)
            ); PRAGMA user_version=2;").map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        if version < 3 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS research_runs (
                id TEXT PRIMARY KEY, configuration TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'running' CHECK(state IN ('running','paused','stopped')),
                cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0), token TEXT, expires_ms INTEGER,
                executed INTEGER NOT NULL DEFAULT 0, reused INTEGER NOT NULL DEFAULT 0,
                rejected INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0,
                active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0,1)), latest TEXT, last_error TEXT
            );
            CREATE TABLE IF NOT EXISTS research_steps (
                run_id TEXT NOT NULL REFERENCES research_runs(id), cursor INTEGER NOT NULL,
                experiment_id TEXT, outcome TEXT NOT NULL, reused INTEGER NOT NULL, error TEXT,
                PRIMARY KEY(run_id,cursor)
            ); PRAGMA user_version=3;").map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        if version < 4 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS scientific_protocols (
                id TEXT PRIMARY KEY, definition_json TEXT NOT NULL, definition_hash TEXT NOT NULL,
                report_json TEXT, report_hash TEXT,
                CHECK ((report_json IS NULL AND report_hash IS NULL) OR (report_json IS NOT NULL AND report_hash IS NOT NULL))
            ); PRAGMA user_version=4;")?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS instrument_holdouts (
                dataset_hash TEXT NOT NULL, instrument TEXT NOT NULL, start_ms INTEGER NOT NULL,end_ms INTEGER NOT NULL,
                protocol_hash TEXT NOT NULL,selected_json TEXT NOT NULL,PRIMARY KEY(dataset_hash,instrument)
            );CREATE TABLE IF NOT EXISTS instrument_holdout_exposures (
                dataset_hash TEXT NOT NULL,instrument TEXT NOT NULL,concept_hash TEXT NOT NULL,experiment_id TEXT NOT NULL REFERENCES experiments(id),
                PRIMARY KEY(dataset_hash,instrument,concept_hash),UNIQUE(experiment_id),
                FOREIGN KEY(dataset_hash,instrument) REFERENCES instrument_holdouts(dataset_hash,instrument)
            );PRAGMA user_version=5;")?;
            tx.commit()?;
        }
        if version < 6 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS protocol_ranges (
                instrument TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                dataset_hash TEXT NOT NULL, protocol_hash TEXT NOT NULL,
                CHECK(start_ms<end_ms), PRIMARY KEY(instrument,start_ms,end_ms)
            );CREATE INDEX IF NOT EXISTS experiment_instrument ON experiments(json_extract(key_json,'$.instrument'));
            PRAGMA user_version=6;").map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        if version < 7 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS training_diagnoses (
                id TEXT PRIMARY KEY, parent_id TEXT NOT NULL REFERENCES experiments(id),
                report_json TEXT NOT NULL, report_hash TEXT NOT NULL
            ); PRAGMA user_version=7;",
            )
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        if version < 8 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS training_reviews (
                id TEXT PRIMARY KEY, diagnosis_id TEXT NOT NULL REFERENCES training_diagnoses(id),
                report_json TEXT NOT NULL, report_hash TEXT NOT NULL
            ); PRAGMA user_version=8;",
            )
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        if version < 9 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS improvement_jobs (
                run_id TEXT NOT NULL REFERENCES research_runs(id), parent_cursor INTEGER NOT NULL,
                input_index INTEGER NOT NULL CHECK(input_index>=0), parent_id TEXT NOT NULL REFERENCES experiments(id),
                state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','completed','failed')),
                diagnosis_id TEXT REFERENCES training_diagnoses(id), review_id TEXT REFERENCES training_reviews(id), error TEXT,
                PRIMARY KEY(run_id,parent_cursor), FOREIGN KEY(run_id,parent_cursor) REFERENCES research_steps(run_id,cursor)
            ); CREATE UNIQUE INDEX IF NOT EXISTS one_pending_improvement ON improvement_jobs(run_id) WHERE state='queued';
            PRAGMA user_version=9;").map_err(crate::corpus_failure::AdmissionFailure::from)?;
            tx.commit()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
        }
        // Additive lookup optimization: no table/row-format or version change.
        // Older v9 readers/writers remain compatible and SQLite maintains it.
        // Idempotent schema bootstrap is admitted as one writer transaction.
        {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("CREATE INDEX IF NOT EXISTS event_action_lookup ON events(action) WHERE action='proof-audit-contradiction-v1' OR action='indexed-audit-contradiction-v1';")?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS replay_checkpoints(experiment_id TEXT NOT NULL REFERENCES experiments(id),cursor INTEGER NOT NULL CHECK(cursor>0),body BLOB NOT NULL CHECK(typeof(body)='blob' AND length(body)<=4194304),hash TEXT NOT NULL CHECK(length(hash)=64),PRIMARY KEY(experiment_id,cursor));CREATE TRIGGER IF NOT EXISTS replay_checkpoint_no_update BEFORE UPDATE ON replay_checkpoints BEGIN SELECT RAISE(ABORT,'checkpoint immutable');END;CREATE TRIGGER IF NOT EXISTS replay_checkpoint_no_delete BEFORE DELETE ON replay_checkpoints BEGIN SELECT RAISE(ABORT,'checkpoint immutable');END;")?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_controls(experiment_id TEXT PRIMARY KEY REFERENCES experiments(id),state TEXT NOT NULL CHECK(state IN ('running','paused','stopped')));")?;
            tx.commit()?;
        }
        init_classified(crate::result_artifact::initialize(&connection))?;
        if version < 10 {
            let tx = writer_tx_open(&connection)?;
            tx.execute_batch("PRAGMA user_version=10;")?;
            tx.commit()?;
        }
        init_classified(crate::corpus_search::initialize(&connection))?;
        crate::search_control::initialize(&mut connection, version < 11)?;
        init_classified(crate::corpus_fleet::initialize(&connection))?;
        init_classified(crate::corpus_failure::initialize(
            &mut connection,
            version < 12,
        ))?;
        init_classified(crate::retry_policy::initialize(
            &mut connection,
            version < 13,
        ))?;
        init_classified(crate::retry_audit::initialize(
            &mut connection,
            version < 14,
        ))?;
        init_classified(crate::supervision_session::initialize(
            &mut connection,
            version < 15,
        ))?;
        init_classified(crate::owned_history::initialize(
            &mut connection,
            version < 16,
        ))?;
        init_classified(crate::corpus_fleet::initialize_owned(
            &mut connection,
            version < 17,
        ))?;
        crate::fleet_control::initialize(&mut connection, version < 18)?;
        init_classified(crate::corpus_campaign::initialize(&mut connection, version))?;
        init_classified(crate::campaign_rotation::initialize(
            &mut connection,
            version,
        ))?;
        init_classified(crate::corpus_campaign::initialize_long_ledger(
            &mut connection,
        ))?;
        crate::research::initialize_recursion(&mut connection, version)?;
        init_classified(crate::stage_machine::initialize(&connection))?;
        init_classified(crate::telemetry::initialize(&connection))?;
        let writer_gate = writer_gate_for(connection.path());
        Ok(Self {
            connection,
            writer_gate,
        })
    }
    /// Atomically reserve OOS time across dataset aliases before any competing registration.
    /// No unlock API: retries may recover only the exact same dataset/protocol/ranges.
    pub fn reserve_protocol_ranges(
        &mut self,
        dataset: &str,
        instrument: &str,
        protocol: &str,
        ranges: &[crate::validation::Interval],
    ) -> Result<()> {
        if dataset.is_empty()
            || protocol.is_empty()
            || !SYMBOLS.contains(&instrument)
            || ranges.is_empty()
            || ranges.len() > 16
        {
            return Err("invalid protected OOS definition".into());
        }
        for (i, range) in ranges.iter().enumerate() {
            range.validate()?;
            if ranges[..i]
                .iter()
                .any(|r| r.start_ms < range.end_exclusive_ms && r.end_exclusive_ms > range.start_ms)
            {
                return Err("overlapping protected OOS ranges".into());
            }
        }
        let tx = writer_tx(&self.connection)?;
        let existing: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM protocol_ranges WHERE protocol_hash=?1",
                [protocol],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if existing > 0 {
            if existing as usize != ranges.len() {
                return Err("immutable OOS range set changed".into());
            }
            for range in ranges {
                let exact: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM protocol_ranges WHERE protocol_hash=?1 AND dataset_hash=?2 AND instrument=?3 AND start_ms=?4 AND end_ms=?5)",params![protocol,dataset,instrument,range.start_ms,range.end_exclusive_ms],|r|r.get(0)).map_err(db_err)?;
                if !exact {
                    return Err("immutable OOS range set changed".into());
                }
            }
        }
        for range in ranges {
            let conflict: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM protocol_ranges WHERE instrument=?1 AND start_ms<?3 AND end_ms>?2 AND NOT(start_ms=?2 AND end_ms=?3 AND dataset_hash=?4 AND protocol_hash=?5)
                UNION ALL SELECT 1 FROM instrument_holdouts WHERE instrument=?1 AND start_ms<?3 AND end_ms>?2
                UNION ALL SELECT 1 FROM holdouts WHERE start_ms<?3 AND end_ms>?2)",
                params![instrument,range.start_ms,range.end_exclusive_ms,dataset,protocol],|r|r.get(0)
            ).map_err(db_err)?;
            if conflict {
                return Err("OOS interval conflicts with an immutable protected range".into());
            }
            let exposed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM experiments WHERE json_extract(key_json,'$.instrument')=?1 AND json_extract(key_json,'$.start_ms')<?3 AND json_extract(key_json,'$.end_exclusive_ms')>?2 AND (json_extract(key_json,'$.dataset_hash')<>?4 OR json_extract(key_json,'$.protocol_hash')<>?5))",
                params![instrument,range.start_ms,range.end_exclusive_ms,dataset,protocol],|r|r.get(0)
            ).map_err(db_err)?;
            if exposed {
                return Err("OOS interval already exposed outside its frozen protocol".into());
            }
            tx.execute("INSERT OR IGNORE INTO protocol_ranges(instrument,start_ms,end_ms,dataset_hash,protocol_hash) VALUES (?1,?2,?3,?4,?5)",params![instrument,range.start_ms,range.end_exclusive_ms,dataset,protocol]).map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }
    /// Freeze a final holdout and selected concepts before any recorded experiment touches it.
    /// One reservation per immutable dataset; there is deliberately no reset/unlock API.
    pub fn reserve_holdout(
        &mut self,
        dataset_hash: &str,
        period: &crate::validation::Interval,
        protocol_hash: &str,
        selected: &[String],
    ) -> Result<()> {
        period.validate()?;
        let mut selection = selected.to_vec();
        selection.sort();
        selection.dedup();
        if dataset_hash.is_empty()
            || protocol_hash.is_empty()
            || selection.is_empty()
            || selection.len() > 100
            || selection.iter().any(|s| s.is_empty() || s.len() > 256)
        {
            return Err("invalid frozen holdout selection".into());
        }
        let tx = writer_tx(&self.connection)?;
        let existing:Option<(i64,i64,String,String)>=tx.query_row("SELECT start_ms,end_ms,protocol_hash,selected_json FROM holdouts WHERE dataset_hash=?1",[dataset_hash],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_err)?;
        if let Some((start, end, protocol, json)) = existing {
            let stored: Vec<String> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if start == period.start_ms
                && end == period.end_exclusive_ms
                && protocol == protocol_hash
                && stored == selection
            {
                return Ok(());
            }
            return Err("holdout definition is already immutable".into());
        }
        {
            let mut query = tx
                .prepare("SELECT key_json FROM experiments")
                .map_err(db_err)?;
            let rows = query
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(db_err)?;
            for row in rows {
                let key: ExperimentKey =
                    serde_json::from_str(&row.map_err(db_err)?).map_err(|e| e.to_string())?;
                if key.start_ms < period.end_exclusive_ms && key.end_exclusive_ms > period.start_ms
                {
                    return Err("holdout already exposed to registered research".into());
                }
            }
        }
        let conflicting: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM holdouts WHERE start_ms<?2 AND end_ms>?1 UNION ALL SELECT 1 FROM instrument_holdouts WHERE start_ms<?2 AND end_ms>?1 UNION ALL SELECT 1 FROM protocol_ranges WHERE start_ms<?2 AND end_ms>?1)",
            params![period.start_ms,period.end_exclusive_ms],|r|r.get(0)
        ).map_err(db_err)?;
        if conflicting {
            return Err("legacy global reservation overlaps another protected interval".into());
        }
        tx.execute("INSERT INTO holdouts(dataset_hash,start_ms,end_ms,protocol_hash,selected_json) VALUES (?1,?2,?3,?4,?5)",params![dataset_hash,period.start_ms,period.end_exclusive_ms,protocol_hash,serde_json::to_string(&selection).map_err(|e|e.to_string())?]).map_err(db_err)?;
        tx.commit().map_err(db_err)
    }
    /// Instrument scope separates distinct markets even when their numeric fixture arrays match.
    /// Legacy unscoped reservations remain conservative global protection and cannot be weakened.
    pub fn reserve_instrument_holdout(
        &mut self,
        dataset_hash: &str,
        instrument: &str,
        period: &crate::validation::Interval,
        protocol_hash: &str,
        selected: &[String],
    ) -> Result<()> {
        period.validate()?;
        let mut selection = selected.to_vec();
        selection.sort();
        selection.dedup();
        if dataset_hash.is_empty()
            || !SYMBOLS.contains(&instrument)
            || protocol_hash.is_empty()
            || selection.is_empty()
            || selection.len() > 100
            || selection.iter().any(|s| s.is_empty() || s.len() > 256)
        {
            return Err("invalid instrument holdout selection".into());
        }
        let tx = writer_tx(&self.connection)?;
        let legacy: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM holdouts WHERE dataset_hash=?1)",
                [dataset_hash],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if legacy {
            return Err("legacy global holdout reservation cannot be narrowed".into());
        }
        let previous:Option<(i64,i64,String,String)>=tx.query_row("SELECT start_ms,end_ms,protocol_hash,selected_json FROM instrument_holdouts WHERE dataset_hash=?1 AND instrument=?2",params![dataset_hash,instrument],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_err)?;
        let json = serde_json::to_string(&selection).map_err(|e| e.to_string())?;
        if let Some((start, end, protocol, stored)) = previous {
            if start == period.start_ms
                && end == period.end_exclusive_ms
                && protocol == protocol_hash
                && stored == json
            {
                return Ok(());
            }
            return Err("instrument holdout definition is already immutable".into());
        }
        {
            let mut q=tx.prepare("SELECT key_json FROM experiments WHERE json_extract(key_json,'$.instrument')=?1").map_err(db_err)?;
            let rows = q
                .query_map([instrument], |r| r.get::<_, String>(0))
                .map_err(db_err)?;
            for row in rows {
                let key: ExperimentKey =
                    serde_json::from_str(&row.map_err(db_err)?).map_err(|e| e.to_string())?;
                if key.start_ms < period.end_exclusive_ms && key.end_exclusive_ms > period.start_ms
                {
                    return Err("instrument holdout already exposed to registered research".into());
                }
            }
        }
        let conflicting: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM instrument_holdouts WHERE instrument=?1 AND start_ms<?3 AND end_ms>?2 UNION ALL SELECT 1 FROM holdouts WHERE start_ms<?3 AND end_ms>?2 UNION ALL SELECT 1 FROM protocol_ranges WHERE instrument=?1 AND start_ms<?3 AND end_ms>?2)",
            params![instrument, period.start_ms, period.end_exclusive_ms], |r| r.get(0)
        ).map_err(db_err)?;
        if conflicting {
            return Err(
                "protected interval already reserved through another dataset/revision".into(),
            );
        }
        tx.execute("INSERT INTO instrument_holdouts(dataset_hash,instrument,start_ms,end_ms,protocol_hash,selected_json) VALUES (?1,?2,?3,?4,?5,?6)",params![dataset_hash,instrument,period.start_ms,period.end_exclusive_ms,protocol_hash,json]).map_err(db_err)?;
        tx.commit().map_err(db_err)
    }
    pub fn register(&mut self, key: &ExperimentKey) -> Result<String> {
        let id = key.identity()?;
        let json = serde_json::to_string(key).map_err(|e| e.to_string())?;
        let tx = writer_tx(&self.connection)?;
        let outside_protocol: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM protocol_ranges WHERE instrument=?1 AND start_ms<?4 AND end_ms>?3 AND (dataset_hash<>?2 OR protocol_hash<>?5))",
            params![key.instrument,key.dataset_hash,key.start_ms,key.end_exclusive_ms,key.protocol_hash],|r|r.get(0)
        ).map_err(db_err)?;
        if outside_protocol {
            return Err(
                "protected OOS time cannot feed another protocol or dataset revision".into(),
            );
        }
        // A different archive, adjusted quote history or parser revision cannot bypass
        // existing protected time. Legacy global reservations conservatively cover every symbol.
        let alias_overlap: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM instrument_holdouts WHERE instrument=?1 AND dataset_hash<>?2 AND start_ms<?4 AND end_ms>?3 UNION ALL SELECT 1 FROM holdouts WHERE dataset_hash<>?2 AND start_ms<?4 AND end_ms>?3)",
            params![key.instrument,key.dataset_hash,key.start_ms,key.end_exclusive_ms], |r| r.get(0)
        ).map_err(db_err)?;
        if alias_overlap {
            return Err(
                "protected interval cannot be exposed through a dataset alias/revision".into(),
            );
        }
        let reservation:Option<(i64,i64,String,String,bool)>=tx.query_row("SELECT start_ms,end_ms,protocol_hash,selected_json,0 FROM holdouts WHERE dataset_hash=?1 UNION ALL SELECT start_ms,end_ms,protocol_hash,selected_json,1 FROM instrument_holdouts WHERE dataset_hash=?1 AND instrument=?2",params![key.dataset_hash,key.instrument],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db_err)?;
        let instrument_scoped = reservation.as_ref().is_some_and(|r| r.4);
        if let Some((start, end, protocol, selected_json, _)) = reservation {
            let touches = key.start_ms < end && key.end_exclusive_ms > start;
            if key.stage == Stage::IndependentValidation {
                let selected: Vec<String> =
                    serde_json::from_str(&selected_json).map_err(|e| e.to_string())?;
                if key.start_ms != start
                    || key.end_exclusive_ms != end
                    || key.protocol_hash != protocol
                    || !selected.contains(&key.concept_hash)
                {
                    return Err(
                        "experiment inconsistent with frozen holdout selection/protocol".into(),
                    );
                }
            } else if touches {
                return Err(
                    "protected holdout cannot feed screening, training or diagnostics".into(),
                );
            }
        } else if key.stage == Stage::IndependentValidation {
            return Err("independent evaluation requires a pre-reserved holdout".into());
        }
        tx.execute(
            "INSERT OR IGNORE INTO experiments(id,key_json,state) VALUES (?1,?2,'queued')",
            params![id, json],
        )
        .map_err(db_err)?;
        let stored: String = tx
            .query_row("SELECT key_json FROM experiments WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .map_err(db_err)?;
        if stored != json {
            return Err("experiment identity collision".into());
        }
        if key.stage == Stage::IndependentValidation {
            let exposed: Option<String> = if instrument_scoped {
                tx.query_row("SELECT experiment_id FROM instrument_holdout_exposures WHERE dataset_hash=?1 AND instrument=?2 AND concept_hash=?3",params![key.dataset_hash,key.instrument,key.concept_hash],|r|r.get(0)).optional().map_err(db_err)?
            } else {
                tx.query_row("SELECT experiment_id FROM holdout_exposures WHERE dataset_hash=?1 AND concept_hash=?2",params![key.dataset_hash,key.concept_hash],|r|r.get(0)).optional().map_err(db_err)?
            };
            if exposed.is_some_and(|previous| previous != id) {
                return Err(
                    "final holdout concept already reserved for a different immutable experiment"
                        .into(),
                );
            }
            if instrument_scoped {
                tx.execute("INSERT OR IGNORE INTO instrument_holdout_exposures(dataset_hash,instrument,concept_hash,experiment_id) VALUES (?1,?2,?3,?4)",params![key.dataset_hash,key.instrument,key.concept_hash,id]).map_err(db_err)?;
            } else {
                tx.execute("INSERT OR IGNORE INTO holdout_exposures(dataset_hash,concept_hash,experiment_id) VALUES (?1,?2,?3)",params![key.dataset_hash,key.concept_hash,id]).map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(id)
    }
    pub fn claim(&mut self, id: &str, now_ms: i64, lease_ms: i64) -> Result<Claim> {
        if !(1..=86_400_000).contains(&lease_ms) {
            return Err("lease duration outside budget".into());
        }
        let expires = now_ms.checked_add(lease_ms).ok_or("lease time overflow")?;
        let tx = writer_tx(&self.connection)?;
        let (state, expiry, attempt): (String, Option<i64>, i64) = tx
            .query_row(
                "SELECT state,expires_ms,attempt FROM experiments WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_err)?;
        if state != "queued" && state != "running" {
            let result = read_result(&tx, id)?.ok_or("terminal experiment lacks result")?;
            return Ok(Claim::Reused(result));
        }
        if state == "running" && expiry.is_some_and(|e| e > now_ms) {
            return Ok(Claim::Busy);
        }
        let token: String = tx
            .query_row("SELECT lower(hex(randomblob(32)))", [], |r| r.get(0))
            .map_err(db_err)?;
        let attempt = attempt.checked_add(1).ok_or("attempt overflow")?;
        tx.execute(
            "UPDATE experiments SET state='running',token=?2,expires_ms=?3,attempt=?4 WHERE id=?1",
            params![id, token, expires, attempt],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,?2,'claimed',?3)",
            params![id, now_ms, attempt],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(Claim::Acquired(Lease {
            experiment_id: id.into(),
            token,
            expires_ms: expires,
            attempt,
        }))
    }
    /// Returns true only for the first durable completion. Retrying the same receipt is idempotent.
    pub fn finish(
        &mut self,
        lease: &Lease,
        now_ms: i64,
        outcome: Outcome,
        result: &serde_json::Value,
    ) -> Result<bool> {
        let bytes = crate::result_artifact::serialize(result, crate::result_artifact::MAX_BYTES)?;
        let result_hash = blake3::hash(&bytes).to_hex().to_string();
        let tx = writer_tx(&self.connection)?;
        if result["kind"] == "synthetic-corpus-replay-v1" {
            let control: Option<String> = tx
                .query_row(
                    "SELECT state FROM corpus_controls WHERE experiment_id=?1",
                    [&lease.experiment_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            if control.is_some_and(|s| s != "running") {
                return Err("corpus publication blocked by durable control".into());
            }
        }
        if result["proof_rejection"] == true && matches!(outcome, Outcome::Rejected) {
            crate::proof_screen::guard(&tx)?;
        }
        if !result["indexed_policy"].is_null()
            && matches!(outcome, Outcome::Completed | Outcome::Rejected)
        {
            crate::indexed_policy::guard(&tx)?;
        }
        let (state, token, expiry, old_hash, attempt): (
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            i64,
        ) = tx
            .query_row(
                "SELECT state,token,expires_ms,result_hash,attempt FROM experiments WHERE id=?1",
                [&lease.experiment_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(db_err)?;
        if token.as_deref() != Some(&lease.token) || attempt != lease.attempt {
            return Err("stale lease token".into());
        }
        if state == outcome.name() && old_hash.as_ref() == Some(&result_hash) {
            read_result(&tx, &lease.experiment_id)?.ok_or("terminal receipt missing")?;
            return Ok(false);
        }
        if state != "running" || expiry.is_none_or(|e| e <= now_ms) {
            return Err("lease expired or experiment already terminal".into());
        }
        let external = bytes.len() > crate::result_artifact::INLINE_BYTES;
        let json = if external {
            crate::result_artifact::publish(&tx, &result_hash, &bytes)?;
            serde_json::to_string(&crate::result_artifact::marker(
                result,
                &result_hash,
                bytes.len(),
            ))
            .map_err(|e| e.to_string())?
        } else {
            String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?
        };
        tx.execute(
            "UPDATE experiments SET state=?2,result_json=?3,result_hash=?4 WHERE id=?1",
            params![lease.experiment_id, outcome.name(), json, result_hash],
        )
        .map_err(db_err)?;
        if external {
            tx.execute(
                "INSERT INTO result_artifacts(experiment_id,hash,bytes) VALUES(?1,?2,?3)",
                params![lease.experiment_id, result_hash, bytes.len()],
            )
            .map_err(db_err)?;
        }
        tx.execute(
            "INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,?2,?3,?4)",
            params![lease.experiment_id, now_ms, outcome.name(), lease.attempt],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(true)
    }
    pub fn experiment_key(&self, id: &str) -> Result<Option<ExperimentKey>> {
        let json: Option<String> = self
            .connection
            .query_row("SELECT key_json FROM experiments WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_err)?;
        json.map(|json| {
            let key: ExperimentKey = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if key.identity()? != id {
                return Err("stored experiment identity corrupted".into());
            }
            Ok(key)
        })
        .transpose()
    }
    pub fn result(&self, id: &str) -> Result<Option<StoredResult>> {
        read_result(&self.connection, id)
    }
    pub fn counts(&self) -> Result<Counts> {
        self.connection.query_row("SELECT count(*),coalesce(sum(state='completed'),0),coalesce(sum(state='running'),0),coalesce(sum(attempt),0) FROM experiments",[],|r|Ok(Counts {registered:r.get(0)?,completed:r.get(1)?,running:r.get(2)?,attempts:r.get(3)?})).map_err(db_err)
    }
    pub fn integrity(&self) -> Result<()> {
        let verdict: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_err)?;
        if verdict != "ok" {
            return Err(format!("database integrity failure: {verdict}"));
        }
        let mut query = self
            .connection
            .prepare("SELECT id,key_json FROM experiments")
            .map_err(db_err)?;
        let rows = query
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        for row in rows {
            let (id, json) = row.map_err(db_err)?;
            let key: ExperimentKey = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if key.identity()? != id {
                return Err("stored experiment identity corrupted".into());
            }
            self.result(&id)?;
            self.replay_checkpoint(&id)?;
        }
        let mut protocols = self
            .connection
            .prepare("SELECT id,definition_json,definition_hash FROM scientific_protocols")
            .map_err(db_err)?;
        let rows = protocols
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(db_err)?;
        for row in rows {
            let (id, json, expected) = row.map_err(db_err)?;
            let definition: serde_json::Value =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if hash(&definition)? != expected || expected != id {
                return Err("scientific definition integrity mismatch".into());
            }
            crate::science::read_report(self, &id)?;
        }
        let mut query = self
            .connection
            .prepare("SELECT id FROM training_diagnoses")
            .map_err(db_err)?;
        let ids = query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        for id in ids {
            crate::improvement::read_report(self, &id.map_err(db_err)?)?;
        }
        let mut query = self
            .connection
            .prepare("SELECT id FROM training_reviews")
            .map_err(db_err)?;
        let ids = query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        for id in ids {
            crate::training_review::read_report(self, &id.map_err(db_err)?)?;
        }
        let inconsistent: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM improvement_jobs j LEFT JOIN research_steps s ON j.depth=0 AND s.run_id=j.run_id AND s.cursor=j.parent_cursor LEFT JOIN training_diagnoses d ON d.id=j.diagnosis_id LEFT JOIN training_reviews r ON r.id=j.review_id LEFT JOIN improvement_jobs pj ON j.depth>0 AND pj.run_id=j.run_id AND pj.parent_cursor=j.parent_cursor AND pj.depth=j.depth-1 LEFT JOIN training_reviews pr ON pr.id=pj.review_id WHERE (j.depth=0 AND s.experiment_id IS NOT j.parent_id) OR (j.depth>0 AND (j.accepted_edge_id IS NULL OR pr.id IS NULL OR NOT EXISTS(SELECT 1 FROM json_each(pr.report_json,'$.children') c WHERE json_extract(c.value,'$.proposal_edge_id')=j.accepted_edge_id AND json_extract(c.value,'$.evidence.experiment_id')=j.parent_id))) OR (j.depth=0 AND j.accepted_edge_id IS NOT NULL) OR (j.diagnosis_id IS NOT NULL AND d.parent_id IS NOT j.parent_id) OR (j.review_id IS NOT NULL AND r.diagnosis_id IS NOT j.diagnosis_id) OR (j.state='completed' AND (j.diagnosis_id IS NULL OR (json_array_length(d.report_json,'$.proposals')>0 AND j.review_id IS NULL))))",[],|r|r.get(0)).map_err(db_err)?;
        if inconsistent {
            return Err("improvement job lineage mismatch".into());
        }
        let dangling: bool = self
            .connection
            .prepare("PRAGMA foreign_key_check")
            .map_err(db_err)?
            .exists([])
            .map_err(db_err)?;
        if dangling {
            return Err("registry foreign key integrity mismatch".into());
        }
        crate::corpus_search::integrity(self)?;
        crate::corpus_fleet::integrity(self)?;
        crate::corpus_campaign::integrity(&self.connection)?;
        crate::campaign_rotation::integrity(&self.connection)?;
        crate::supervision_session::integrity(self)?;
        crate::owned_history::integrity(self)?;
        crate::fleet_control::integrity(&self.connection)?;
        crate::result_artifact::integrity(&self.connection)?;
        Ok(())
    }
    /// SQLite online backup includes committed WAL contents. Destination must be new.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        let mut artifact_name = destination
            .file_name()
            .ok_or("backup filename missing")?
            .to_os_string();
        artifact_name.push(".artifacts");
        if std::fs::symlink_metadata(destination.with_file_name(artifact_name)).is_ok() {
            return Err("backup artifact destination must be new".into());
        }
        let guard = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| e.to_string())?;
        drop(guard);
        let mut dest = Connection::open(destination).map_err(db_err)?;
        {
            let backup =
                rusqlite::backup::Backup::new(&self.connection, &mut dest).map_err(db_err)?;
            backup
                .run_to_completion(128, Duration::from_millis(5), None)
                .map_err(db_err)?;
        }
        crate::result_artifact::initialize(&dest)?;
        crate::result_artifact::copy_referenced(&self.connection, &mut dest)?;
        drop(dest);
        Registry::open(destination)?.integrity()
    }
}
fn read_result(connection: &Connection, id: &str) -> Result<Option<StoredResult>> {
    let row:Option<(String,String,String)>=connection.query_row("SELECT state,result_json,result_hash FROM experiments WHERE id=?1 AND result_json IS NOT NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_err)?;
    row.map(|(outcome, json, result_hash)| {
        let stored: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let result = crate::result_artifact::resolve(connection, id, stored, &result_hash)?;
        if hash(&result)? != result_hash {
            return Err("result content hash corrupted".into());
        }
        Ok(StoredResult {
            outcome,
            result,
            result_hash,
        })
    })
    .transpose()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCheckpoint {
    pub experiment_id: String,
    pub configuration_hash: String,
    pub cursor: i64,
    pub previous_hash: Option<String>,
    pub state: serde_json::Value,
}
// Global journal admission also bounds any one experiment's retained history.
const MAX_REPLAY_CHECKPOINTS: i64 = 10_000;
fn checkpoint_read(c: &Connection, id: &str) -> Result<Option<(ReplayCheckpoint, String)>> {
    let key_json: String = c
        .query_row("SELECT key_json FROM experiments WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(db_err)?;
    let key: ExperimentKey = serde_json::from_str(&key_json).map_err(|e| e.to_string())?;
    if key.identity()? != id {
        return Err("checkpoint experiment identity mismatch".into());
    }
    let mut q=c.prepare("SELECT cursor,CASE WHEN typeof(body)='blob' AND length(body)<=4194304 THEN body END,CASE WHEN length(hash)=64 THEN hash END FROM replay_checkpoints WHERE experiment_id=?1 ORDER BY cursor LIMIT ?2").map_err(db_err)?;
    let mut last: Option<(ReplayCheckpoint, String)> = None;
    let mut n = 0;
    for row in q
        .query_map(params![id, MAX_REPLAY_CHECKPOINTS + 1], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db_err)?
    {
        n += 1;
        if n > MAX_REPLAY_CHECKPOINTS {
            return Err("checkpoint history budget exceeded".into());
        }
        let (cursor, body, digest) = row.map_err(db_err)?;
        let record: ReplayCheckpoint = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        if record.experiment_id != id
            || record.configuration_hash != key.configuration_hash
            || record.cursor != cursor
            || record.previous_hash != last.as_ref().map(|(_, h)| h.clone())
            || hash(&record)? != digest
        {
            return Err("checkpoint source/configuration/history integrity mismatch".into());
        }
        last = Some((record, digest));
    }
    Ok(last)
}
impl Registry {
    /// Snapshot validation only. Every subsequent write still fences within its transaction.
    pub fn validate_lease(&self, lease: &Lease, now_ms: i64) -> Result<()> {
        let valid:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM experiments WHERE id=?1 AND state='running' AND token=?2 AND attempt=?3 AND expires_ms>?4)",params![lease.experiment_id,lease.token,lease.attempt,now_ms],|r|r.get(0)).map_err(db_err)?;
        if !valid {
            return Err("lease stale, expired or terminal".into());
        }
        Ok(())
    }
    /// Mechanically verified, frozen-experiment-bound checkpoint history. State
    /// semantics must additionally be validated by the account replay driver.
    pub fn replay_checkpoint(&self, id: &str) -> Result<Option<(ReplayCheckpoint, String)>> {
        checkpoint_read(&self.connection, id)
    }
    pub fn save_replay_checkpoint(
        &mut self,
        lease: &Lease,
        now_ms: i64,
        expected_cursor: i64,
        next_cursor: i64,
        state: serde_json::Value,
    ) -> Result<String> {
        if next_cursor <= expected_cursor || expected_cursor < 0 {
            return Err("checkpoint cursor must advance".into());
        }
        let tx = writer_tx(&self.connection)?;
        let (status, token, expiry, key_json, attempt): (
            String,
            Option<String>,
            Option<i64>,
            String,
            i64,
        ) = tx
            .query_row(
                "SELECT state,token,expires_ms,key_json,attempt FROM experiments WHERE id=?1",
                [&lease.experiment_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(db_err)?;
        if status != "running"
            || token.as_deref() != Some(&lease.token)
            || attempt != lease.attempt
            || expiry.is_none_or(|e| e <= now_ms)
        {
            return Err("checkpoint lease stale, expired or terminal".into());
        }
        let previous = checkpoint_read(&tx, &lease.experiment_id)?;
        if previous.as_ref().map_or(0, |(r, _)| r.cursor) != expected_cursor {
            return Err("checkpoint cursor changed".into());
        }
        let key: ExperimentKey = serde_json::from_str(&key_json).map_err(|e| e.to_string())?;
        let record = ReplayCheckpoint {
            experiment_id: lease.experiment_id.clone(),
            configuration_hash: key.configuration_hash,
            cursor: next_cursor,
            previous_hash: previous.map(|(_, h)| h),
            state,
        };
        let body = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        let (count, bytes): (i64, i64) = tx
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(body)),0) FROM replay_checkpoints",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)?;
        if count >= MAX_REPLAY_CHECKPOINTS
            || body.len() > 4194304
            || bytes + body.len() as i64 > 128 * 1024 * 1024
        {
            return Err("checkpoint metadata budget exceeded".into());
        }
        let digest = hash(&record)?;
        tx.execute(
            "INSERT INTO replay_checkpoints VALUES(?1,?2,?3,?4)",
            params![record.experiment_id, next_cursor, body, digest],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(digest)
    }
}

impl Registry {
    /// Yield after a committed checkpoint. Attempts and immutable evidence remain.
    pub fn yield_lease(&mut self, lease: &Lease, now_ms: i64) -> Result<()> {
        let tx = writer_tx(&self.connection)?;
        let n=tx.execute("UPDATE experiments SET state='queued',token=NULL,expires_ms=NULL WHERE id=?1 AND state='running' AND token=?2 AND attempt=?3 AND expires_ms>?4",params![lease.experiment_id,lease.token,lease.attempt,now_ms]).map_err(db_err)?;
        if n != 1 {
            return Err("cannot yield stale, expired or terminal lease".into());
        }
        tx.execute(
            "INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES(?1,?2,'yielded',?3)",
            params![lease.experiment_id, now_ms, lease.attempt],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }
}

impl Registry {
    pub fn corpus_control(&self, id: &str) -> Result<String> {
        let key = self
            .experiment_key(id)?
            .ok_or("corpus experiment missing")?;
        if key.protocol_hash != "synthetic-corpus-training-v1" {
            return Err("not a registered corpus job".into());
        }
        Ok(self
            .connection
            .query_row(
                "SELECT state FROM corpus_controls WHERE experiment_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or_else(|| "running".into()))
    }
    pub fn set_corpus_control(&mut self, id: &str, state: &str, now_ms: i64) -> Result<()> {
        if !["running", "paused", "stopped"].contains(&state) {
            return Err("invalid corpus control".into());
        }
        self.corpus_control(id)?;
        let tx = writer_tx(&self.connection)?;
        let (status,control):(String,Option<String>)=tx.query_row("SELECT e.state,c.state FROM experiments e LEFT JOIN corpus_controls c ON c.experiment_id=e.id WHERE e.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_err)?;
        if !["queued", "running"].contains(&status.as_str())
            || control.as_deref() == Some("stopped")
        {
            return Err("terminal corpus job/control cannot be changed".into());
        }
        tx.execute("INSERT INTO corpus_controls VALUES(?1,?2) ON CONFLICT(experiment_id) DO UPDATE SET state=excluded.state",params![id,state]).map_err(db_err)?;
        tx.execute(
            "INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES(?1,?2,?3,0)",
            params![id, now_ms, format!("corpus-control-{state}")],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }
}

impl Registry {
    /// Verified recovery metadata only; no lease tokens or claimed financial metrics.
    pub fn corpus_status(&self, id: &str) -> Result<serde_json::Value> {
        let tx = self.connection.unchecked_transaction().map_err(db_err)?;
        let control = self.corpus_control(id)?;
        let (state, attempt, expiry): (String, i64, Option<i64>) = tx
            .query_row(
                "SELECT state,attempt,expires_ms FROM experiments WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_err)?;
        let checkpoint = checkpoint_read(&tx, id)?;
        let value = serde_json::json!({"experiment_id":id,"state":state,"control":control,"attempt":attempt.to_string(),"expires_ms":expiry,"checkpoint_cursor":checkpoint.as_ref().map(|(r,_)|r.cursor),"checkpoint_hash":checkpoint.map(|(_,h)|h),"limitations":"Recovery metadata only; checkpoints do not establish qualification or completed replay."});
        tx.commit().map_err(db_err)?;
        Ok(value)
    }
}

#[cfg(test)]
mod writer_admission_tests {
    use super::*;
    use crate::corpus_failure::{Kind, classify_error};
    use std::sync::{Arc, Barrier};

    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("writer-admission-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Fill both admission slots: holder A owns the SQLite write lock with an
    /// open writer transaction (slot 1); holder B is admitted to slot 2 and
    /// parks in BEGIN IMMEDIATE's busy wait with a long busy timeout so it
    /// cannot time out inside the test window. WAL permits only ONE open
    /// writer per file, so "two slots held" means one writing plus one parked
    /// in BEGIN — never two open transactions. All registries are opened
    /// BEFORE the write lock is held (schema bootstrap writes would otherwise
    /// queue behind it), and A acquires the lock before B is spawned so the
    /// saturation is deterministic. Returns once both slots are in flight;
    /// the lock stays held until `release` fires.
    fn saturate_gate(db: &Path, release: Arc<Barrier>) -> Vec<std::thread::JoinHandle<()>> {
        let reg_a = Registry::open(db).unwrap();
        let reg_b = Registry::open(db).unwrap();
        reg_b
            .connection
            .busy_timeout(Duration::from_secs(60))
            .unwrap();
        let ready = Arc::new(Barrier::new(2));
        let ready2 = ready.clone();
        let a = std::thread::spawn(move || {
            let tx = writer_tx(&reg_a.connection).unwrap();
            ready2.wait();
            release.wait();
            tx.commit().unwrap();
        });
        ready.wait();
        let b = std::thread::spawn(move || {
            let tx = writer_tx(&reg_b.connection).unwrap();
            tx.commit().unwrap();
        });
        vec![a, b]
    }

    fn wait_in_flight(reg: &Registry, want: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while reg.writer_admission()["in_flight"] != want {
            assert!(
                std::time::Instant::now() < deadline,
                "gate never reached in_flight={want}: {}",
                reg.writer_admission()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn concurrent_writers_wait_within_budget_and_all_commit() {
        let dir = Temp::new("wait");
        let db = dir.0.join("registry.sqlite");
        let main_reg = Registry::open(&db).unwrap();
        {
            let tx = writer_tx(&main_reg.connection).unwrap();
            tx.execute_batch("CREATE TABLE pressure(n INTEGER)")
                .unwrap();
            tx.commit().unwrap();
        }
        let writer_regs: Vec<Registry> = (0..6).map(|_| Registry::open(&db).unwrap()).collect();
        let release = Arc::new(Barrier::new(2));
        let holders = saturate_gate(&db, release.clone());
        wait_in_flight(&main_reg, "2");
        let start = Arc::new(Barrier::new(7));
        let mut writers = Vec::new();
        for (t, reg) in writer_regs.into_iter().enumerate() {
            let start = start.clone();
            writers.push(std::thread::spawn(move || {
                start.wait();
                for i in 0..5 {
                    // Bounded wait generous enough for a loaded suite; the
                    // refusal budget itself is exercised separately.
                    let tx = writer_tx_with_policy(
                        &reg.connection,
                        WriterPolicy {
                            gate_wait_ms: 10_000,
                            lock_retries: WRITER_LOCK_RETRIES,
                        },
                    )
                    .unwrap();
                    tx.execute(
                        "INSERT INTO pressure VALUES(?1)",
                        params![(t * 5 + i) as i64],
                    )
                    .unwrap();
                    tx.commit().unwrap();
                }
            }));
        }
        start.wait();
        // Both slots stay held, so every writer must queue at the gate.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(main_reg.writer_admission()["in_flight"], "2");
        release.wait();
        for h in holders {
            h.join().unwrap();
        }
        for w in writers {
            w.join().unwrap();
        }
        let n: i64 = main_reg
            .connection
            .query_row("SELECT count(*) FROM pressure", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 30);
        let obs = main_reg.writer_admission();
        assert_eq!(obs["refused"], "0", "{obs}");
        assert_eq!(obs["in_flight"], "0", "{obs}");
        assert_eq!(obs["max_in_flight"], "2", "{obs}");
        let waited: u64 = obs["waited"].as_str().unwrap().parse().unwrap();
        assert!(waited >= 6, "every writer waited at least once: {obs}");
    }

    #[test]
    fn saturated_gate_refuses_typed_and_the_refused_thread_recovers() {
        let dir = Temp::new("refuse");
        let db = dir.0.join("registry.sqlite");
        let main_reg = Registry::open(&db).unwrap();
        let release = Arc::new(Barrier::new(2));
        let holders = saturate_gate(&db, release.clone());
        wait_in_flight(&main_reg, "2");
        let policy = WriterPolicy {
            gate_wait_ms: 50,
            lock_retries: 0,
        };
        let first = writer_tx_with_policy(&main_reg.connection, policy)
            .expect_err("saturated gate must refuse after the wait budget");
        assert_eq!(first.kind, Kind::RegistryAdmissionBusy);
        assert_eq!(
            classify_error(&first.to_string()),
            Kind::RegistryAdmissionBusy
        );
        // Regression: a refusal must not leave a stale reentrancy depth that
        // lets this same thread bypass admission afterwards.
        let second = writer_tx_with_policy(&main_reg.connection, policy)
            .expect_err("still saturated: the second attempt must also be refused");
        assert_eq!(
            classify_error(&second.to_string()),
            Kind::RegistryAdmissionBusy
        );
        assert_eq!(main_reg.writer_admission()["refused"], "2");
        assert_eq!(main_reg.writer_admission()["in_flight"], "2");
        release.wait();
        for h in holders {
            h.join().unwrap();
        }
        // The refused thread admits and commits normally once pressure clears.
        let tx = writer_tx(&main_reg.connection).unwrap();
        tx.execute_batch("CREATE TABLE recovered(n INTEGER)")
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(main_reg.writer_admission()["in_flight"], "0");
    }

    #[test]
    fn refusal_provenance_survives_successful_cleanup_writes() {
        let dir = Temp::new("provenance");
        let db = dir.0.join("registry.sqlite");
        let main_reg = Registry::open(&db).unwrap();
        let release = Arc::new(Barrier::new(2));
        let holders = saturate_gate(&db, release.clone());
        wait_in_flight(&main_reg, "2");
        let failure = writer_tx_with_policy(
            &main_reg.connection,
            WriterPolicy {
                gate_wait_ms: 50,
                lock_retries: 0,
            },
        )
        .expect_err("saturated gate must refuse");
        let message = failure.to_string();
        release.wait();
        for h in holders {
            h.join().unwrap();
        }
        // A successful cleanup write (e.g. lease release after a failed step)
        // must not erase the typed kind of the earlier refusal.
        let tx = writer_tx(&main_reg.connection).unwrap();
        tx.execute_batch("CREATE TABLE cleanup(n INTEGER)").unwrap();
        tx.commit().unwrap();
        assert_eq!(classify_error(&message), Kind::RegistryAdmissionBusy);
        // An unrelated error can never inherit the recorded busy kind.
        assert_eq!(
            classify_error("unrelated permanent refusal"),
            Kind::Unclassified
        );
    }

    #[test]
    fn reentrant_writer_neither_deadlocks_nor_consumes_a_slot() {
        let dir = Temp::new("reentrant");
        let db = dir.0.join("registry.sqlite");
        let outer_reg = Registry::open(&db).unwrap();
        let inner_reg = Registry::open(&db).unwrap();
        inner_reg
            .connection
            .busy_timeout(Duration::from_millis(10))
            .unwrap();
        // Open every handle before the write lock is held: schema bootstrap
        // writes would otherwise queue behind the outer transaction.
        let reg_b = Registry::open(&db).unwrap();
        reg_b
            .connection
            .busy_timeout(Duration::from_secs(60))
            .unwrap();
        // The outer writer on this thread owns slot 1 and the write lock.
        let outer = writer_tx(&outer_reg.connection).unwrap();
        // A second connection parks in BEGIN IMMEDIATE on slot 2.
        let parked = std::thread::spawn(move || {
            let tx = writer_tx(&reg_b.connection).unwrap();
            tx.commit().unwrap();
        });
        wait_in_flight(&outer_reg, "2");
        // Same thread, third connection on the same file: the gate must not
        // make the nested writer wait on its own slot. SQLite itself refuses
        // the nested BEGIN IMMEDIATE while the outer write lock is held.
        let started = std::time::Instant::now();
        let inner = writer_tx_with_policy(
            &inner_reg.connection,
            WriterPolicy {
                gate_wait_ms: 50,
                lock_retries: 0,
            },
        );
        let elapsed = started.elapsed();
        let inner = inner.expect_err("nested BEGIN is refused by SQLite, not by the gate");
        assert_eq!(inner.kind, Kind::RegistryAdmissionBusy);
        assert!(
            elapsed < Duration::from_millis(1000),
            "reentrant admission must not queue on the gate: {elapsed:?}"
        );
        assert_eq!(outer_reg.writer_admission()["in_flight"], "2");
        drop(outer);
        parked.join().unwrap();
        assert_eq!(outer_reg.writer_admission()["in_flight"], "0");
        // Depth fully released: the same thread admits normally afterwards.
        let tx = writer_tx(&inner_reg.connection).unwrap();
        tx.execute_batch("CREATE TABLE reentrant_ok(n INTEGER)")
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(outer_reg.writer_admission()["in_flight"], "0");
    }

    #[test]
    fn writer_policy_outside_declared_bounds_is_refused() {
        let dir = Temp::new("bounds");
        let db = dir.0.join("registry.sqlite");
        let reg = Registry::open(&db).unwrap();
        for policy in [
            WriterPolicy {
                gate_wait_ms: 60_001,
                lock_retries: 0,
            },
            WriterPolicy {
                gate_wait_ms: 0,
                lock_retries: 9,
            },
        ] {
            let failure = writer_tx_with_policy(&reg.connection, policy)
                .expect_err("policy outside declared bounds must be refused");
            assert_eq!(failure.kind, Kind::Unclassified);
            assert!(
                failure.message.contains("outside declared bounds"),
                "{}",
                failure.message
            );
        }
        assert_eq!(reg.writer_admission()["in_flight"], "0");
    }

    #[test]
    fn gates_are_per_file_and_observations_are_runtime_only() {
        let dir = Temp::new("identity");
        let db = dir.0.join("a.sqlite");
        let db2 = dir.0.join("b.sqlite");
        let reg1 = Registry::open(&db).unwrap();
        let reg2 = Registry::open(&db).unwrap();
        assert!(Arc::ptr_eq(&reg1.writer_gate, &reg2.writer_gate));
        let other = Registry::open(&db2).unwrap();
        assert!(!Arc::ptr_eq(&reg1.writer_gate, &other.writer_gate));
        let g1 = writer_gate_for(None);
        let g2 = writer_gate_for(None);
        assert!(!Arc::ptr_eq(&g1, &g2));
        {
            let tx = writer_tx(&reg1.connection).unwrap();
            tx.execute_batch("CREATE TABLE runtime_only(n INTEGER)")
                .unwrap();
            tx.commit().unwrap();
        }
        let version: u32 = reg1
            .connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(reg1);
        drop(reg2);
        // Counters are runtime observations, not persisted schema: a fresh
        // handle starts clean and the schema version is unchanged.
        let reopened = Registry::open(&db).unwrap();
        let obs = reopened.writer_admission();
        assert_eq!(obs["refused"], "0", "{obs}");
        assert_eq!(obs["in_flight"], "0", "{obs}");
        assert_eq!(obs["busy_retries"], "0", "{obs}");
        let version2: u32 = reopened
            .connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version2, SCHEMA_VERSION);
    }
}
