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
pub struct Registry {
    pub(crate) connection: Connection,
}
fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}
impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path).map_err(db_err)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_err)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
            )
            .map_err(db_err)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_err)?;
        if version > 9 {
            return Err("database schema newer than supported".into());
        }
        if version == 0 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
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
            PRAGMA user_version=1;").map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        if version < 2 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS holdouts (
                dataset_hash TEXT PRIMARY KEY, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                protocol_hash TEXT NOT NULL, selected_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS holdout_exposures (
                dataset_hash TEXT NOT NULL REFERENCES holdouts(dataset_hash), concept_hash TEXT NOT NULL,
                experiment_id TEXT NOT NULL REFERENCES experiments(id),
                PRIMARY KEY(dataset_hash,concept_hash), UNIQUE(experiment_id)
            ); PRAGMA user_version=2;").map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        if version < 3 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
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
            ); PRAGMA user_version=3;").map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        if version < 4 {
            connection.execute_batch("CREATE TABLE IF NOT EXISTS scientific_protocols (
                id TEXT PRIMARY KEY, definition_json TEXT NOT NULL, definition_hash TEXT NOT NULL,
                report_json TEXT, report_hash TEXT,
                CHECK ((report_json IS NULL AND report_hash IS NULL) OR (report_json IS NOT NULL AND report_hash IS NOT NULL))
            ); PRAGMA user_version=4;").map_err(db_err)?;
        }
        if version < 5 {
            connection.execute_batch("CREATE TABLE IF NOT EXISTS instrument_holdouts (
                dataset_hash TEXT NOT NULL, instrument TEXT NOT NULL, start_ms INTEGER NOT NULL,end_ms INTEGER NOT NULL,
                protocol_hash TEXT NOT NULL,selected_json TEXT NOT NULL,PRIMARY KEY(dataset_hash,instrument)
            );CREATE TABLE IF NOT EXISTS instrument_holdout_exposures (
                dataset_hash TEXT NOT NULL,instrument TEXT NOT NULL,concept_hash TEXT NOT NULL,experiment_id TEXT NOT NULL REFERENCES experiments(id),
                PRIMARY KEY(dataset_hash,instrument,concept_hash),UNIQUE(experiment_id),
                FOREIGN KEY(dataset_hash,instrument) REFERENCES instrument_holdouts(dataset_hash,instrument)
            );PRAGMA user_version=5;").map_err(db_err)?;
        }
        if version < 6 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS protocol_ranges (
                instrument TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                dataset_hash TEXT NOT NULL, protocol_hash TEXT NOT NULL,
                CHECK(start_ms<end_ms), PRIMARY KEY(instrument,start_ms,end_ms)
            );CREATE INDEX IF NOT EXISTS experiment_instrument ON experiments(json_extract(key_json,'$.instrument'));
            PRAGMA user_version=6;").map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        if version < 7 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS training_diagnoses (
                id TEXT PRIMARY KEY, parent_id TEXT NOT NULL REFERENCES experiments(id),
                report_json TEXT NOT NULL, report_hash TEXT NOT NULL
            ); PRAGMA user_version=7;",
            )
            .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        if version < 8 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS training_reviews (
                id TEXT PRIMARY KEY, diagnosis_id TEXT NOT NULL REFERENCES training_diagnoses(id),
                report_json TEXT NOT NULL, report_hash TEXT NOT NULL
            ); PRAGMA user_version=8;",
            )
            .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        if version < 9 {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_err)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS improvement_jobs (
                run_id TEXT NOT NULL REFERENCES research_runs(id), parent_cursor INTEGER NOT NULL,
                input_index INTEGER NOT NULL CHECK(input_index>=0), parent_id TEXT NOT NULL REFERENCES experiments(id),
                state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','completed','failed')),
                diagnosis_id TEXT REFERENCES training_diagnoses(id), review_id TEXT REFERENCES training_reviews(id), error TEXT,
                PRIMARY KEY(run_id,parent_cursor), FOREIGN KEY(run_id,parent_cursor) REFERENCES research_steps(run_id,cursor)
            ); CREATE UNIQUE INDEX IF NOT EXISTS one_pending_improvement ON improvement_jobs(run_id) WHERE state='queued';
            PRAGMA user_version=9;").map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        Ok(Self { connection })
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
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
        let json = serde_json::to_string(result).map_err(|e| e.to_string())?;
        if json.len() > 8 * 1024 * 1024 {
            return Err("result exceeds metadata budget; use an immutable artifact".into());
        }
        let result_hash = hash(result)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let (state, token, expiry, old_hash): (
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
        ) = tx
            .query_row(
                "SELECT state,token,expires_ms,result_hash FROM experiments WHERE id=?1",
                [&lease.experiment_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(db_err)?;
        if token.as_deref() != Some(&lease.token) {
            return Err("stale lease token".into());
        }
        if state == outcome.name() && old_hash.as_ref() == Some(&result_hash) {
            return Ok(false);
        }
        if state != "running" || expiry.is_none_or(|e| e <= now_ms) {
            return Err("lease expired or experiment already terminal".into());
        }
        tx.execute(
            "UPDATE experiments SET state=?2,result_json=?3,result_hash=?4 WHERE id=?1",
            params![lease.experiment_id, outcome.name(), json, result_hash],
        )
        .map_err(db_err)?;
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
        let inconsistent: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM improvement_jobs j JOIN research_steps s ON s.run_id=j.run_id AND s.cursor=j.parent_cursor LEFT JOIN training_diagnoses d ON d.id=j.diagnosis_id LEFT JOIN training_reviews r ON r.id=j.review_id WHERE s.experiment_id IS NOT j.parent_id OR (j.diagnosis_id IS NOT NULL AND d.parent_id IS NOT j.parent_id) OR (j.review_id IS NOT NULL AND r.diagnosis_id IS NOT j.diagnosis_id) OR (j.state='completed' AND (j.diagnosis_id IS NULL OR (json_array_length(d.report_json,'$.proposals')>0 AND j.review_id IS NULL))))",[],|r|r.get(0)).map_err(db_err)?;
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
        Ok(())
    }
    /// SQLite online backup includes committed WAL contents. Destination must be new.
    pub fn backup(&self, destination: &Path) -> Result<()> {
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
        drop(dest);
        Registry::open(destination)?.integrity()
    }
}
fn read_result(connection: &Connection, id: &str) -> Result<Option<StoredResult>> {
    let row:Option<(String,String,String)>=connection.query_row("SELECT state,result_json,result_hash FROM experiments WHERE id=?1 AND result_json IS NOT NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_err)?;
    row.map(|(outcome, json, result_hash)| {
        let result: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
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
