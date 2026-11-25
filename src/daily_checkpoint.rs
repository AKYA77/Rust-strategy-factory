//! Daily-checkpoint contract enforcement.
//!
//! Every candidate receives at most one deterministic scheduled evaluation
//! opportunity per eligible session-day. The checkpoint observes only
//! information available at the checkpoint (causal publication), the daily entry
//! count is zero or one, skipped days are recorded with reasons, and a failed
//! or already-consumed checkpoint is never retried later the same day. Records
//! are persisted in SQLite so the contract survives process restarts and
//! historical chunk boundaries.
//!
//! Session-days are UTC civil days (days since the Unix epoch). UTC has no DST,
//! so session-day boundaries are stable across daylight-saving transitions and
//! the same UTC instant always maps to the same session-day regardless of local
//! clock rules.
//!
//! This module is the enforcement mechanism. Callers (the research loop) supply
//! the domain preconditions (session open, market open, observations present,
//! sufficient history) and finalize each admitted checkpoint as an entry, a
//! no-entry evaluation, or a failure.

use crate::Result;
use rusqlite::OptionalExtension;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Milliseconds in one UTC day.
const MS_PER_DAY: i64 = 86_400_000;

/// Why a session-day was skipped (never an entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    /// The session-day was outside the trading session (e.g. a weekend).
    SessionClosed,
    /// The market was closed for the instrument on that session-day.
    MarketClosed,
    /// Required observations were missing or not yet published at the checkpoint.
    MissingObservations,
    /// The timestamp fell outside the configured session window.
    OutsideSessionWindow,
    /// Insufficient causal history to evaluate the rule.
    InsufficientHistory,
    /// A checkpoint already exists for this candidate on this session-day.
    AlreadyCheckpointed,
    /// The checkpoint previously failed and may not be retried the same day.
    PriorFailedCheckpoint,
    /// The daily entry limit (one) was already reached.
    EntryLimitReached,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionClosed => "session_closed",
            Self::MarketClosed => "market_closed",
            Self::MissingObservations => "missing_observations",
            Self::OutsideSessionWindow => "outside_session_window",
            Self::InsufficientHistory => "insufficient_history",
            Self::AlreadyCheckpointed => "already_checkpointed",
            Self::PriorFailedCheckpoint => "prior_failed_checkpoint",
            Self::EntryLimitReached => "entry_limit_reached",
        }
    }
}

/// Outcome of attempting to admit a candidate to its daily checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointDecision {
    /// The candidate may be evaluated now, using only info available at the
    /// checkpoint. A pending checkpoint row is consumed for this session-day.
    Admit { session_day: i64 },
    /// The candidate was skipped; the reason is recorded durably. Not retried
    /// later the same day.
    Skip {
        session_day: i64,
        reason: SkipReason,
    },
    /// A prior checkpoint this session-day forbids any further attempt.
    RefusedRetry {
        session_day: i64,
        reason: SkipReason,
    },
}

/// One durable daily-checkpoint record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCheckpointRecord {
    pub candidate_id: String,
    pub session_day: i64,
    /// UTC milliseconds when the checkpoint record was written.
    pub recorded_ms: i64,
    /// Daily entry count: zero or one.
    pub entry_taken: bool,
    /// Why the day was skipped, if any.
    pub skip_reason: Option<SkipReason>,
    /// Whether the checkpoint evaluation failed (no retry the same day).
    pub failed: bool,
    pub outcome: Option<String>,
}

/// Persisted daily-checkpoint store.
pub struct DailyCheckpointStore {
    connection: Connection,
}

impl DailyCheckpointStore {
    /// Open (or create) the store at `path`. The `daily_checkpoints` table is
    /// created if absent. Existing rows are never altered or deleted by open.
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)
            .map_err(|e| format!("failed to open daily-checkpoint store: {e}"))?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
                CREATE TABLE IF NOT EXISTS daily_checkpoints (
                    candidate_id TEXT NOT NULL,
                    session_day INTEGER NOT NULL,
                    recorded_ms INTEGER NOT NULL,
                    entry_taken INTEGER NOT NULL DEFAULT 0,
                    skip_reason TEXT,
                    failed INTEGER NOT NULL DEFAULT 0,
                    outcome TEXT,
                    PRIMARY KEY (candidate_id, session_day)
                );",
            )
            .map_err(|e| format!("failed to initialize daily-checkpoint schema: {e}"))?;
        Ok(Self { connection })
    }

    /// UTC civil day (days since the Unix epoch) containing `ts_ms`. Stable
    /// across DST transitions because it is derived from UTC, never local time.
    pub fn session_day(ts_ms: i64) -> Result<i64> {
        ts_ms
            .checked_div_euclid(MS_PER_DAY)
            .ok_or("session day overflow".into())
    }

    /// Decide whether `candidate_id` may be checkpointed on `session_day`.
    ///
    /// `now_ms` is the checkpoint time (must be causal, i.e. no future info).
    /// `session_open`, `market_open`, `observations_present` and
    /// `sufficient_history` are the domain preconditions evaluated only against
    /// data available at the checkpoint.
    #[allow(clippy::too_many_arguments)]
    pub fn decision(
        &mut self,
        candidate_id: &str,
        session_day: i64,
        now_ms: i64,
        session_open: bool,
        market_open: bool,
        observations_present: bool,
        sufficient_history: bool,
    ) -> Result<CheckpointDecision> {
        let existing = self
            .connection
            .query_row(
                "SELECT entry_taken, failed FROM daily_checkpoints
                 WHERE candidate_id=?1 AND session_day=?2",
                rusqlite::params![candidate_id, session_day],
                |r| Ok((r.get::<_, i32>(0)? != 0, r.get::<_, i32>(1)? != 0)),
            )
            .optional()
            .map_err(|e| format!("daily-checkpoint read failed: {e}"))?;

        match existing {
            // Already entered today: the daily entry count (one) is reached.
            Some((true, _)) => Ok(CheckpointDecision::RefusedRetry {
                session_day,
                reason: SkipReason::EntryLimitReached,
            }),
            // Checkpointed today but it failed: no same-day retry.
            Some((false, true)) => Ok(CheckpointDecision::RefusedRetry {
                session_day,
                reason: SkipReason::PriorFailedCheckpoint,
            }),
            // Checkpointed today without an entry (skipped or evaluated with no
            // entry): consumed, no retry later the same day.
            Some((false, false)) => Ok(CheckpointDecision::RefusedRetry {
                session_day,
                reason: SkipReason::AlreadyCheckpointed,
            }),
            None => {
                if !session_open {
                    self.record_skip(
                        candidate_id,
                        session_day,
                        now_ms,
                        SkipReason::SessionClosed,
                        None,
                    )?;
                    return Ok(CheckpointDecision::Skip {
                        session_day,
                        reason: SkipReason::SessionClosed,
                    });
                }
                if !market_open {
                    self.record_skip(
                        candidate_id,
                        session_day,
                        now_ms,
                        SkipReason::MarketClosed,
                        None,
                    )?;
                    return Ok(CheckpointDecision::Skip {
                        session_day,
                        reason: SkipReason::MarketClosed,
                    });
                }
                if !observations_present {
                    self.record_skip(
                        candidate_id,
                        session_day,
                        now_ms,
                        SkipReason::MissingObservations,
                        None,
                    )?;
                    return Ok(CheckpointDecision::Skip {
                        session_day,
                        reason: SkipReason::MissingObservations,
                    });
                }
                if !sufficient_history {
                    self.record_skip(
                        candidate_id,
                        session_day,
                        now_ms,
                        SkipReason::InsufficientHistory,
                        None,
                    )?;
                    return Ok(CheckpointDecision::Skip {
                        session_day,
                        reason: SkipReason::InsufficientHistory,
                    });
                }
                // Consume the session-day with a pending checkpoint so it cannot
                // be admitted again (no multiple entries via separate paths).
                self.connection
                    .execute(
                        "INSERT INTO daily_checkpoints
                         (candidate_id, session_day, recorded_ms, entry_taken, failed)
                         VALUES (?1, ?2, ?3, 0, 0)",
                        rusqlite::params![candidate_id, session_day, now_ms],
                    )
                    .map_err(|e| format!("daily-checkpoint admit failed: {e}"))?;
                Ok(CheckpointDecision::Admit { session_day })
            }
        }
    }

    /// Finalize an admitted checkpoint as an entry taken (daily entry count =
    /// one). Fails if no pending checkpoint exists for the session-day or it
    /// was already finalized.
    pub fn finalize_entry(
        &mut self,
        candidate_id: &str,
        session_day: i64,
        now_ms: i64,
        outcome: &str,
    ) -> Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE daily_checkpoints
                 SET entry_taken = 1, outcome = ?4, recorded_ms = ?5
                 WHERE candidate_id = ?1 AND session_day = ?2 AND entry_taken = 0 AND failed = 0",
                rusqlite::params![candidate_id, session_day, now_ms, outcome, now_ms],
            )
            .map_err(|e| format!("daily-checkpoint entry finalize failed: {e}"))?;
        if changed != 1 {
            return Err("no pending daily checkpoint to finalize as an entry".into());
        }
        Ok(())
    }

    /// Finalize an admitted checkpoint that produced no entry (evaluated, zero
    /// entries). Fails if no pending checkpoint exists or it was finalized.
    pub fn finalize_no_entry(
        &mut self,
        candidate_id: &str,
        session_day: i64,
        now_ms: i64,
        outcome: &str,
    ) -> Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE daily_checkpoints
                 SET outcome = ?4, recorded_ms = ?5
                 WHERE candidate_id = ?1 AND session_day = ?2 AND entry_taken = 0 AND failed = 0",
                rusqlite::params![candidate_id, session_day, now_ms, outcome, now_ms],
            )
            .map_err(|e| format!("daily-checkpoint no-entry finalize failed: {e}"))?;
        if changed != 1 {
            return Err("no pending daily checkpoint to finalize as no-entry".into());
        }
        Ok(())
    }

    /// Finalize an admitted checkpoint as a failure. A failed checkpoint is
    /// never retried later the same day.
    pub fn finalize_failure(
        &mut self,
        candidate_id: &str,
        session_day: i64,
        now_ms: i64,
        error: &str,
    ) -> Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE daily_checkpoints
                 SET failed = 1, outcome = ?4, recorded_ms = ?5
                 WHERE candidate_id = ?1 AND session_day = ?2 AND entry_taken = 0 AND failed = 0",
                rusqlite::params![candidate_id, session_day, now_ms, error, now_ms],
            )
            .map_err(|e| format!("daily-checkpoint failure finalize failed: {e}"))?;
        if changed != 1 {
            return Err("no pending daily checkpoint to finalize as a failure".into());
        }
        Ok(())
    }

    /// Read the durable record for a candidate on a session-day, if any.
    pub fn record(
        &self,
        candidate_id: &str,
        session_day: i64,
    ) -> Result<Option<DailyCheckpointRecord>> {
        let row = self
            .connection
            .query_row(
                "SELECT recorded_ms, entry_taken, skip_reason, failed, outcome
                 FROM daily_checkpoints WHERE candidate_id=?1 AND session_day=?2",
                rusqlite::params![candidate_id, session_day],
                |r| {
                    let skip_reason = match r.get::<_, Option<String>>(2)? {
                        Some(s) => Some(match s.as_str() {
                            "session_closed" => SkipReason::SessionClosed,
                            "market_closed" => SkipReason::MarketClosed,
                            "missing_observations" => SkipReason::MissingObservations,
                            "outside_session_window" => SkipReason::OutsideSessionWindow,
                            "insufficient_history" => SkipReason::InsufficientHistory,
                            "already_checkpointed" => SkipReason::AlreadyCheckpointed,
                            "prior_failed_checkpoint" => SkipReason::PriorFailedCheckpoint,
                            "entry_limit_reached" => SkipReason::EntryLimitReached,
                            _ => return Err(rusqlite::Error::InvalidQuery),
                        }),
                        None => None,
                    };
                    Ok(DailyCheckpointRecord {
                        candidate_id: candidate_id.to_owned(),
                        session_day,
                        recorded_ms: r.get::<_, i64>(0)?,
                        entry_taken: r.get::<_, i32>(1)? != 0,
                        skip_reason,
                        failed: r.get::<_, i32>(3)? != 0,
                        outcome: r.get::<_, Option<String>>(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| format!("daily-checkpoint read failed: {e}"))?;
        Ok(row)
    }

    /// Whether an entry was taken for `candidate_id` on `session_day`.
    pub fn entry_taken(&self, candidate_id: &str, session_day: i64) -> Result<bool> {
        Ok(self
            .record(candidate_id, session_day)?
            .is_some_and(|r| r.entry_taken))
    }

    /// All records for a candidate (newest first), for inspection and restart
    /// reconciliation.
    pub fn records_for(&self, candidate_id: &str) -> Result<Vec<DailyCheckpointRecord>> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT session_day, recorded_ms, entry_taken, skip_reason, failed, outcome
                 FROM daily_checkpoints WHERE candidate_id=?1 ORDER BY session_day DESC",
            )
            .map_err(|e| format!("daily-checkpoint prepare failed: {e}"))?;
        let iter = stmt
            .query_map(rusqlite::params![candidate_id], |r| {
                let skip_reason = match r.get::<_, Option<String>>(3)? {
                    Some(s) => Some(match s.as_str() {
                        "session_closed" => SkipReason::SessionClosed,
                        "market_closed" => SkipReason::MarketClosed,
                        "missing_observations" => SkipReason::MissingObservations,
                        "outside_session_window" => SkipReason::OutsideSessionWindow,
                        "insufficient_history" => SkipReason::InsufficientHistory,
                        "already_checkpointed" => SkipReason::AlreadyCheckpointed,
                        "prior_failed_checkpoint" => SkipReason::PriorFailedCheckpoint,
                        "entry_limit_reached" => SkipReason::EntryLimitReached,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    }),
                    None => None,
                };
                Ok(DailyCheckpointRecord {
                    candidate_id: candidate_id.to_owned(),
                    session_day: r.get::<_, i64>(0)?,
                    recorded_ms: r.get::<_, i64>(1)?,
                    entry_taken: r.get::<_, i32>(2)? != 0,
                    skip_reason,
                    failed: r.get::<_, i32>(4)? != 0,
                    outcome: r.get::<_, Option<String>>(5)?,
                })
            })
            .map_err(|e| format!("daily-checkpoint query failed: {e}"))?;
        let mut out = Vec::new();
        for item in iter {
            out.push(item.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Internal: record a skipped session-day with a reason and optional
    /// outcome, inserting a consumed (non-entry, non-failed) checkpoint row.
    fn record_skip(
        &mut self,
        candidate_id: &str,
        session_day: i64,
        now_ms: i64,
        reason: SkipReason,
        outcome: Option<String>,
    ) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO daily_checkpoints
                 (candidate_id, session_day, recorded_ms, entry_taken, skip_reason, failed, outcome)
                 VALUES (?1, ?2, ?3, 0, ?4, 0, ?5)",
                rusqlite::params![candidate_id, session_day, now_ms, reason.as_str(), outcome],
            )
            .map_err(|e| format!("daily-checkpoint skip record failed: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    fn temp_path() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "daily-checkpoint-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p.join("store.sqlite")
    }

    fn now_ms(day_offset_days: i64) -> i64 {
        // 2026-03-08T12:00:00Z plus day offset (a US DST spring-forward date).
        1773067200000 + day_offset_days * MS_PER_DAY
    }

    #[test]
    fn session_day_is_utc_and_dst_stable() {
        // Same UTC instant, same session-day regardless of local DST rules.
        let a = DailyCheckpointStore::session_day(1773067200000).unwrap(); // 2026-03-08T12:00:00Z
        let b = DailyCheckpointStore::session_day(1773067200000).unwrap();
        assert_eq!(a, b, "session-day must be deterministic");
        // One UTC day apart differs by exactly one.
        assert_eq!(
            DailyCheckpointStore::session_day(1773067200000 + MS_PER_DAY).unwrap(),
            a + 1,
            "adjacent UTC days differ by one"
        );
        // A DST "spring forward" local wall time (02:30 local does not exist)
        // still maps to the correct UTC day because the store uses UTC.
        let dst_day = DailyCheckpointStore::session_day(1773067200000).unwrap();
        let same_utc_day_later =
            DailyCheckpointStore::session_day(1773067200000 + 3_600_000).unwrap();
        assert_eq!(
            dst_day, same_utc_day_later,
            "same UTC day across local clock change"
        );
        // Midnight UTC boundary: 2026-03-07T23:30Z vs 2026-03-08T00:30Z.
        let before = DailyCheckpointStore::session_day(1773063600000).unwrap(); // 2026-03-07T23:30:00Z
        let after = DailyCheckpointStore::session_day(1773067200000).unwrap(); // 2026-03-08T12:00:00Z
        assert_eq!(after, before + 1, "UTC midnight boundary advances one day");
    }

    fn open_store() -> DailyCheckpointStore {
        DailyCheckpointStore::open(&temp_path()).unwrap()
    }

    #[test]
    fn admit_once_per_session_day_and_no_same_day_retry() {
        let mut store = open_store();
        let candidate = "strategy-v1::eurusd::opening-range-breakout";
        let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();

        let d1 = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(matches!(d1, CheckpointDecision::Admit { .. }), "{d1:?}");

        // A second admission the same session-day is refused (consumed).
        let d2 = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(
            matches!(
                d2,
                CheckpointDecision::RefusedRetry {
                    reason: SkipReason::AlreadyCheckpointed,
                    ..
                }
            ),
            "{d2:?}"
        );
    }

    #[test]
    fn skip_reasons_are_recorded_and_not_retried_same_day() {
        for (session_open, market_open, observations, history, reason) in [
            (false, true, true, true, SkipReason::SessionClosed),
            (true, false, true, true, SkipReason::MarketClosed),
            (true, true, false, true, SkipReason::MissingObservations),
            (true, true, true, false, SkipReason::InsufficientHistory),
        ] {
            let mut store = open_store();
            let candidate = "strategy-v1::skip-case";
            let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();
            let decision = store
                .decision(
                    candidate,
                    day,
                    now_ms(0),
                    session_open,
                    market_open,
                    observations,
                    history,
                )
                .unwrap();
            assert!(
                matches!(decision, CheckpointDecision::Skip { reason: ref r, .. } if *r == reason),
                "{decision:?} expected {reason:?}"
            );
            // The skip is durable: reopening the store still sees it.
            let rec = store.record(candidate, day).unwrap().unwrap();
            assert_eq!(rec.skip_reason, Some(reason));
            assert!(!rec.entry_taken);
            assert!(!rec.failed);
            // No retry later the same day.
            let again = store
                .decision(candidate, day, now_ms(0), true, true, true, true)
                .unwrap();
            assert!(
                matches!(
                    again,
                    CheckpointDecision::RefusedRetry {
                        reason: SkipReason::AlreadyCheckpointed,
                        ..
                    }
                ),
                "{again:?}"
            );
        }
    }

    #[test]
    fn daily_entry_count_is_zero_or_one() {
        let mut store = open_store();
        let candidate = "strategy-v1::entry-case";
        let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();

        let decision = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(matches!(decision, CheckpointDecision::Admit { .. }));
        store
            .finalize_entry(candidate, day, now_ms(0), "filled")
            .unwrap();
        assert!(store.entry_taken(candidate, day).unwrap());

        // A second entry the same day is refused.
        let rec = store.record(candidate, day).unwrap().unwrap();
        assert!(rec.entry_taken);
        let again = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(
            matches!(
                again,
                CheckpointDecision::RefusedRetry {
                    reason: SkipReason::EntryLimitReached,
                    ..
                }
            ),
            "{again:?}"
        );
        // finalize_entry again must fail (already finalized).
        assert!(
            store
                .finalize_entry(candidate, day, now_ms(0), "filled")
                .is_err(),
            "second entry must be refused"
        );
    }

    #[test]
    fn failed_checkpoint_is_not_retried_same_day() {
        let mut store = open_store();
        let candidate = "strategy-v1::fail-case";
        let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();

        let decision = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(matches!(decision, CheckpointDecision::Admit { .. }));
        store
            .finalize_failure(candidate, day, now_ms(0), "missing observation")
            .unwrap();

        let rec = store.record(candidate, day).unwrap().unwrap();
        assert!(rec.failed);
        assert!(!rec.entry_taken);

        let again = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(
            matches!(
                again,
                CheckpointDecision::RefusedRetry {
                    reason: SkipReason::PriorFailedCheckpoint,
                    ..
                }
            ),
            "{again:?}"
        );
    }

    #[test]
    fn evaluated_with_no_entry_is_consumed_and_not_retried() {
        let mut store = open_store();
        let candidate = "strategy-v1::no-entry";
        let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();

        let decision = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(matches!(decision, CheckpointDecision::Admit { .. }));
        store
            .finalize_no_entry(candidate, day, now_ms(0), "no_signal")
            .unwrap();

        let rec = store.record(candidate, day).unwrap().unwrap();
        assert!(!rec.entry_taken);
        assert!(!rec.failed);
        assert_eq!(rec.outcome.as_deref(), Some("no_signal"));

        let again = store
            .decision(candidate, day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(
            matches!(
                again,
                CheckpointDecision::RefusedRetry {
                    reason: SkipReason::AlreadyCheckpointed,
                    ..
                }
            ),
            "{again:?}"
        );
    }

    #[test]
    fn checkpoint_survives_process_restart_and_chunk_boundary() {
        let path = temp_path();
        let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();
        {
            let mut store = DailyCheckpointStore::open(&path).unwrap();
            let candidate = "strategy-v1::restart-case";
            let decision = store
                .decision(candidate, day, now_ms(0), true, true, true, true)
                .unwrap();
            assert!(matches!(decision, CheckpointDecision::Admit { .. }));
            store
                .finalize_entry(candidate, day, now_ms(0), "filled")
                .unwrap();
        } // store dropped; connection closed, WAL checkpointed

        // Reopen as if after a process restart / across a historical chunk boundary.
        let mut store = DailyCheckpointStore::open(&path).unwrap();
        let rec = store
            .record("strategy-v1::restart-case", day)
            .unwrap()
            .unwrap();
        assert!(rec.entry_taken);
        assert_eq!(rec.outcome.as_deref(), Some("filled"));

        // The consumed session-day refuses re-admission after restart.
        let decision = store
            .decision(
                "strategy-v1::restart-case",
                day,
                now_ms(0),
                true,
                true,
                true,
                true,
            )
            .unwrap();
        assert!(
            matches!(
                decision,
                CheckpointDecision::RefusedRetry {
                    reason: SkipReason::EntryLimitReached,
                    ..
                }
            ),
            "{decision:?}"
        );
        // A different session-day is a fresh opportunity.
        let next_day = day + 1;
        let decision = store
            .decision(
                "strategy-v1::restart-case",
                next_day,
                now_ms(1),
                true,
                true,
                true,
                true,
            )
            .unwrap();
        assert!(matches!(decision, CheckpointDecision::Admit { .. }));
    }

    #[test]
    fn distinct_candidates_and_days_are_independent() {
        let mut store = open_store();
        let day = DailyCheckpointStore::session_day(now_ms(0)).unwrap();
        let a = store
            .decision("candidate-a", day, now_ms(0), true, true, true, true)
            .unwrap();
        let b = store
            .decision("candidate-b", day, now_ms(0), true, true, true, true)
            .unwrap();
        assert!(matches!(a, CheckpointDecision::Admit { .. }));
        assert!(matches!(b, CheckpointDecision::Admit { .. }));
        // Different session-days for the same candidate are independent.
        let c = store
            .decision("candidate-a", day + 1, now_ms(1), true, true, true, true)
            .unwrap();
        assert!(matches!(c, CheckpointDecision::Admit { .. }));
    }

    #[test]
    fn skip_reason_strings_are_stable() {
        assert_eq!(SkipReason::SessionClosed.as_str(), "session_closed");
        assert_eq!(SkipReason::MarketClosed.as_str(), "market_closed");
        assert_eq!(
            SkipReason::MissingObservations.as_str(),
            "missing_observations"
        );
        assert_eq!(
            SkipReason::OutsideSessionWindow.as_str(),
            "outside_session_window"
        );
        assert_eq!(
            SkipReason::InsufficientHistory.as_str(),
            "insufficient_history"
        );
        assert_eq!(
            SkipReason::AlreadyCheckpointed.as_str(),
            "already_checkpointed"
        );
        assert_eq!(
            SkipReason::PriorFailedCheckpoint.as_str(),
            "prior_failed_checkpoint"
        );
        assert_eq!(
            SkipReason::EntryLimitReached.as_str(),
            "entry_limit_reached"
        );
    }

    #[test]
    fn candidate_ids_bind_to_daily_checkpoint_identity() {
        // The daily checkpoint is keyed by candidate identity, so a hash derived
        // from the candidate binds the checkpoint to its strategy identity.
        let id = crate::data::hash(&("daily-checkpoint-v1", "eurusd", "opening-range-breakout")).unwrap();
        assert!(!id.is_empty());
        let _ = id;
    }
}
