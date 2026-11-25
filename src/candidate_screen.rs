//! Bounded, explicitly hypothetical M1 cash model. Never an authoritative fill receipt.
//! Uncertain paths use adverse bar extrema and reserve the whole exit bar. No candidate
//! rejection or scientific eligibility is inferred from this approximate trajectory.
use crate::{
    Result,
    candles::{self, Candle, Timeframe},
    contracts::{self, Conversion, Side},
    data::Dataset,
    indicators::Mask,
    strategies::Strategy,
    tick_engine::{Costs, Replay},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const VERSION: &str = "m1-hypothetical-cash-v1";
pub const MAX_BAR_VISITS: usize = 25_000;

/// Configurable library-level shadow policy. This version never skips replay.
/// A later execution policy must bind configuration into experiment identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowPolicy {
    pub version: String,
    pub threshold_bps: usize,
    pub minimum_closed_trades: usize,
    pub promote_uncertain: bool,
    pub audit_sample_bps: u16,
    pub seed: u64,
}
impl Default for ShadowPolicy {
    fn default() -> Self {
        Self {
            version: "m1-shadow-promotion-v1".into(),
            // Stage-1 screening floor: exactly 40.00% is inclusive.
            threshold_bps: 4000,
            minimum_closed_trades: 20,
            promote_uncertain: true,
            audit_sample_bps: 1000,
            seed: 42,
        }
    }
}
impl ShadowPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.version != "m1-shadow-promotion-v1"
            || self.threshold_bps > 10000
            || !(1..=10000).contains(&self.minimum_closed_trades)
            || !(1..=10000).contains(&self.audit_sample_bps)
        {
            return Err("invalid shadow promotion/audit policy".into());
        }
        Ok(())
    }
    pub fn decision(&self, e: &Estimate, experiment_id: &str) -> Result<serde_json::Value> {
        self.validate()?;
        if e.version != VERSION || e.estimated_net_wins > e.estimated_closed_trades {
            return Err("invalid/version-mismatched candle estimate".into());
        }
        let would_reject = e.status == "estimated"
            && e.estimated_closed_trades >= self.minimum_closed_trades
            && (!self.promote_uncertain || e.uncertainties.is_empty())
            && e.estimated_net_wins as u128 * 10000
                < self.threshold_bps as u128 * e.estimated_closed_trades as u128;
        // Conservative safe-failure bound: even the most favorable resolution
        // allowed by the candle model cannot reach the threshold. It holds even
        // under model uncertainty, so it never gates on promote_uncertain.
        let bound_proven_failure = e.status == "estimated"
            && e.estimated_closed_trades >= self.minimum_closed_trades
            && e.net_bounds.as_ref().is_some_and(|b| {
                b.optimistic_win_rate_bps
                    .is_some_and(|r| r < self.threshold_bps)
            });
        let classification =
            if e.status != "estimated" || e.estimated_closed_trades < self.minimum_closed_trades {
                ScreenClassification::NotScreened
            } else if bound_proven_failure {
                ScreenClassification::ProvenScreenFailure
            } else {
                ScreenClassification::ScreenUncertain
            };
        let identity = serde_json::to_vec(&(self, experiment_id)).map_err(|e| e.to_string())?;
        let digest = blake3::hash(&identity);
        let bucket = u64::from_le_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .map_err(|_| "audit digest width")?,
        ) % 10000;
        Ok(
            serde_json::json!({"version":self.version,"policy":self,"would_reject":would_reject,
            "bound_proven_failure":bound_proven_failure,
            "screen_classification":classification,
            "selected_for_sampled_audit":would_reject && bucket < u64::from(self.audit_sample_bps),
            "execution":"shadow-only-all-candidates-still-replayed", "proven_failure":false}),
        )
    }

    /// Explicit Stage-1 decision against the 40.00% screening floor.
    ///
    /// - [`Stage1Decision::Pass`]: estimate is at or above the floor (>= 40%,
    ///   inclusive) and carries no unresolved ambiguity.
    /// - [`Stage1Decision::Reject`]: estimate is below the floor with sufficient
    ///   eligible signals and no ambiguity that could flip the result; or the
    ///   conservative optimistic bound is itself below the floor (proven).
    /// - [`Stage1Decision::Hold`]: the estimate is ambiguous, unsupported, or the
    ///   sample is too thin. A hold is never a pass and never a proven reject.
    ///
    /// This is a screening diagnostic; it does not grant qualification and all
    /// candidates still escalate to exact replay.
    pub fn stage1_decision(&self, e: &Estimate) -> Result<Stage1Decision> {
        self.validate()?;
        if e.version != VERSION || e.estimated_net_wins > e.estimated_closed_trades {
            return Err("invalid/version-mismatched candle estimate".into());
        }
        let ambiguous = !e.uncertainties.is_empty();
        let sufficient =
            e.status == "estimated" && e.estimated_closed_trades >= self.minimum_closed_trades;
        // Proven failure: the most favourable resolution allowed by the candle
        // model cannot reach the floor. Sound for the whole reference execution.
        let proven_bound_failure = sufficient
            && e.net_bounds.as_ref().is_some_and(|b| {
                b.optimistic_win_rate_bps
                    .is_some_and(|r| r < self.threshold_bps)
            });
        if proven_bound_failure {
            return Ok(Stage1Decision::Reject);
        }
        if !sufficient {
            return Ok(Stage1Decision::Hold);
        }
        let below_floor = e.estimated_net_wins as u128 * 10000
            < self.threshold_bps as u128 * e.estimated_closed_trades as u128;
        if ambiguous {
            // The estimate can be wrong when both barriers occur inside one bar
            // or execution is ambiguous: hold, do not pass and do not prove fail.
            return Ok(Stage1Decision::Hold);
        }
        Ok(if below_floor {
            Stage1Decision::Reject
        } else {
            Stage1Decision::Pass
        })
    }
}

/// Stage-1 outcome relative to the 40.00% screening floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage1Decision {
    /// At or above the floor (>= 40%, inclusive) with no unresolved ambiguity.
    Pass,
    /// Below the floor with a sound case, or proven by the optimistic bound.
    Reject,
    /// Ambiguous/unsupported/thin: held for exact replay, never a pass.
    Hold,
}
impl Stage1Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Reject => "REJECT",
            Self::Hold => "HOLD",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Estimate {
    pub version: String,
    pub interpretation: String,
    pub status: String,
    pub estimated_closed_trades: usize,
    pub estimated_net_wins: usize,
    pub estimated_net_micro_usd: i128,
    pub estimated_net_win_rate_bps: Option<usize>,
    pub estimated_sizing_rejections: usize,
    pub unavailable_stop_signals: usize,
    pub overlapping_signals_skipped: usize,
    pub bar_visits: usize,
    pub uncertainties: BTreeSet<String>,
    pub promotes_to_tick: bool,
    /// Conservative pessimistic/optimistic net-outcome bounds across the
    /// estimate's resolved trades, derived from the explicit uncertainty set
    /// (adverse stop proxy, SL/TP collision order, partial deadline bar).
    /// `None` when no trade resolved. Diagnostic only: not acceptance.
    pub net_bounds: Option<NetBounds>,
    #[serde(skip)]
    optimistic_net_micro_usd: i128,
    #[serde(skip)]
    optimistic_wins: usize,
}
impl Estimate {
    fn new() -> Self {
        Self {
            version: VERSION.into(),
            interpretation: "Hypothetical sequential USD cash model; actual next-quote market entry, adverse OHLC stop proxy, exact requested target, first real deadline quote; whole exit bar reserved. Totals may be partial. Pessimistic/optimistic net bounds are computed from the explicit uncertainty set; all signaled candidates still escalate to tick replay.".into(),
            status: "estimated".into(), estimated_closed_trades: 0, estimated_net_wins: 0,
            estimated_net_micro_usd: 0, estimated_net_win_rate_bps: None,
            estimated_sizing_rejections: 0, unavailable_stop_signals: 0,
            overlapping_signals_skipped: 0, bar_visits: 0, uncertainties: BTreeSet::new(),
            promotes_to_tick: true, net_bounds: None, optimistic_net_micro_usd: 0,
            optimistic_wins: 0,
        }
    }
    fn flag(&mut self, reason: &str) {
        self.uncertainties.insert(reason.into());
    }
}

/// Inputs and masks must come from the same frozen compilation as replay. M1 bars
/// are built from actual quotes only; no final unfinished bucket is manufactured.
pub fn estimate(
    data: &Dataset,
    primary: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    window: Option<&crate::validation::Interval>,
) -> Result<Estimate> {
    data.verify()?;
    costs.validate()?;
    mask.validate()?;
    let strategy = strategy.canonical()?;
    if mask.len() != primary.len() {
        return Err("estimate mask length mismatch".into());
    }
    if let Some(w) = window {
        w.validate()?;
    }
    let mut out = Estimate::new();
    if data.profile.quote != "USD" || strategy.entry.is_some() {
        out.status = "unsupported".into();
        out.flag("pending-entry-or-non-USD-conversion-requires-tick-replay");
        return Ok(out);
    }
    let p = &data.profile;
    let bars = candles::build(data, Timeframe::M1)?;
    let visible = window.map_or(primary.len(), |w| {
        primary.partition_point(|c| c.available_ms < w.end_exclusive_ms)
    });
    let distances = crate::stops::prepare(&strategy, p, &primary[..visible])?;
    let slip = costs
        .slippage_pips
        .checked_mul(p.pip)
        .ok_or("estimate slippage overflow")?;
    let mut equity = contracts::INITIAL_EQUITY;
    let mut busy_through: Option<u64> = None;
    for i in mask.set_indices_prefix(visible)? {
        let signal = &primary[i];
        if window.is_some_and(|w| !w.contains(signal.available_ms)) {
            continue;
        }
        let entry_i = data
            .ticks
            .partition_point(|t| t.sequence <= signal.publication_sequence);
        let Some(t) = data
            .ticks
            .get(entry_i)
            .filter(|t| window.is_none_or(|w| w.contains(t.timestamp_ms)))
        else {
            out.flag("no-eligible-entry-quote-in-scope");
            continue;
        };
        if busy_through.is_some_and(|s| t.sequence <= s) {
            out.overlapping_signals_skipped += 1;
            continue;
        }
        let crate::stops::Distance::Ready(distance) = distances.at(i) else {
            out.unavailable_stop_signals += 1;
            continue;
        };
        let (bid, ask) = costs.effective(p, *t)?;
        let entry = crate::orders::market_from_effective(p, costs, bid, ask, strategy.side)?;
        let request = crate::risk::Request {
            equity,
            entry,
            mark: if strategy.side == Side::Long {
                bid
            } else {
                ask
            },
            side: strategy.side,
            stop_with_exit_slip: distance.checked_add(slip).ok_or("estimate risk overflow")?,
            conversion: Conversion::usd(),
        };
        let Ok(reservation) = crate::risk::size(p, costs, request) else {
            out.estimated_sizing_rejections += 1;
            continue;
        };
        let (stop, target) = crate::tick_engine::protective(entry, strategy.side, distance)?;
        let deadline = t
            .timestamp_ms
            .checked_add(contracts::MAX_HOLD_MS)
            .ok_or("estimate deadline overflow")?;
        let deadline_i = data.ticks.partition_point(|q| q.timestamp_ms < deadline);
        let deadline_quote = data
            .ticks
            .get(deadline_i)
            .filter(|q| window.is_none_or(|w| w.contains(q.timestamp_ms)));
        let first = bars.partition_point(|c| c.start_ms.saturating_add(60000) <= t.timestamp_ms);
        let mut exit = None;
        for c in &bars[first..] {
            if window.is_some_and(|w| c.available_ms >= w.end_exclusive_ms) {
                break;
            }
            if c.start_ms >= deadline {
                break;
            }
            if out.bar_visits == MAX_BAR_VISITS {
                out.status = "budget-exhausted-partial".into();
                out.flag("bar-visit-budget");
                return Ok(finish(out));
            }
            out.bar_visits += 1;
            let bounds = crate::screen::barrier_bounds(
                p,
                costs,
                c,
                crate::screen::Position {
                    side: strategy.side,
                    entry_ms: t.timestamp_ms,
                    stop,
                    target,
                    deadline_ms: deadline,
                },
            )?;
            if c.available_ms > deadline {
                out.flag("deadline-partial-bar-prior-barrier-order-unknown");
                if let Some(q) = deadline_quote {
                    let (b, a) = costs.effective(p, *q)?;
                    exit = Some((
                        crate::orders::market_from_effective(
                            p,
                            costs,
                            b,
                            a,
                            if strategy.side == Side::Long {
                                Side::Short
                            } else {
                                Side::Long
                            },
                        )?,
                        q.sequence,
                    ));
                }
                break;
            }
            if bounds.sl_tp_collision {
                out.flag("SL-TP-order-unknown-stop-proxy-selected");
            }
            if bounds.entry_or_deadline_uncertain {
                out.flag("entry-or-deadline-partial-bar");
            }
            let stop_possible = bounds
                .possible
                .contains(&crate::screen::Possible::StopMarket);
            let target_possible = bounds
                .possible
                .contains(&crate::screen::Possible::TargetLimit);
            if !stop_possible && !target_possible {
                continue;
            }
            let price = if stop_possible {
                out.flag("stop-first-quote-unknown-adverse-extremum-proxy");
                if strategy.side == Side::Long {
                    c.bid
                        .low
                        .checked_sub(slip)
                        .ok_or("estimate stop overflow")?
                } else {
                    c.ask
                        .high
                        .max(
                            c.bid
                                .high
                                .checked_add(
                                    costs
                                        .spread_floor_pips
                                        .checked_mul(p.pip)
                                        .ok_or("estimate spread overflow")?,
                                )
                                .ok_or("estimate spread overflow")?,
                        )
                        .checked_add(slip)
                        .ok_or("estimate stop overflow")?
                }
            } else {
                target
            };
            if strategy.side == Side::Short {
                out.flag("ask-extrema-correlation-unknown");
            }
            out.flag("whole-exit-bar-reserved-not-exact-overlap");
            let net_best = optimistic_exit_net(
                p,
                costs,
                entry,
                reservation.volume,
                strategy.side,
                if target_possible { target } else { stop },
            )?;
            out.optimistic_net_micro_usd = out
                .optimistic_net_micro_usd
                .checked_add(net_best)
                .ok_or("estimate sum overflow")?;
            out.optimistic_wins += usize::from(net_best > 0);
            exit = Some((
                price,
                c.publication_sequence
                    .checked_sub(1)
                    .ok_or("estimate sequence underflow")?,
            ));
            break;
        }
        if exit.is_none() {
            // Use a deadline quote only when every pre-deadline bucket was closed in scope.
            // An unfinished bucket could contain a prior barrier hit and cannot be skipped.
            if let Some(q) = deadline_quote.filter(|q| {
                bars.binary_search_by_key(&q.sequence, |c| c.publication_sequence)
                    .is_ok()
            }) {
                let (b, a) = costs.effective(p, *q)?;
                let price = crate::orders::market_from_effective(
                    p,
                    costs,
                    b,
                    a,
                    if strategy.side == Side::Long {
                        Side::Short
                    } else {
                        Side::Long
                    },
                )?;
                let net_best =
                    optimistic_exit_net(p, costs, entry, reservation.volume, strategy.side, price)?;
                out.optimistic_net_micro_usd = out
                    .optimistic_net_micro_usd
                    .checked_add(net_best)
                    .ok_or("estimate sum overflow")?;
                out.optimistic_wins += usize::from(net_best > 0);
                exit = Some((price, q.sequence));
            }
        }
        let Some((price, through)) = exit else {
            out.flag("unresolved-position-or-unclosed-coverage");
            out.status = "unresolved-partial".into();
            break;
        };
        if price <= 0 {
            return Err("estimate nonpositive exit under costs".into());
        }
        let movement = price
            .checked_sub(entry)
            .and_then(|v| v.checked_mul(strategy.side.sign()))
            .ok_or("estimate movement overflow")?;
        let net = contracts::pnl(p, movement, reservation.volume, Conversion::usd())?
            .checked_sub(
                costs
                    .commission(reservation.volume)
                    .checked_mul(2)
                    .ok_or("estimate fee overflow")?,
            )
            .ok_or("estimate net overflow")?;
        out.estimated_closed_trades += 1;
        out.estimated_net_wins += usize::from(net > 0);
        out.estimated_net_micro_usd = out
            .estimated_net_micro_usd
            .checked_add(net)
            .ok_or("estimate sum overflow")?;
        equity = equity.checked_add(net).ok_or("estimate equity overflow")?;
        busy_through = Some(through);
    }
    Ok(finish(out))
}
fn finish(mut out: Estimate) -> Estimate {
    out.estimated_net_win_rate_bps =
        (out.estimated_net_wins * 10000).checked_div(out.estimated_closed_trades);
    let closed = out.estimated_closed_trades;
    out.net_bounds = (closed > 0).then(|| NetBounds {
        version: "candidate-net-bounds-v1".into(),
        closed_trades: closed,
        pessimistic_net_micro_usd: out.estimated_net_micro_usd,
        optimistic_net_micro_usd: out.optimistic_net_micro_usd,
        pessimistic_wins: out.estimated_net_wins,
        optimistic_wins: out.optimistic_wins,
        pessimistic_win_rate_bps: (out.estimated_net_wins * 10000).checked_div(closed),
        optimistic_win_rate_bps: (out.optimistic_wins * 10000).checked_div(closed),
    });
    out
}

/// Best-case net for a resolved exit consistent with the barrier bounds: the
/// stop or target level itself, with no adverse excursion beyond it. Deadline
/// exits pass the actual quote, so best == adverse there.
fn optimistic_exit_net(
    p: &crate::contracts::Profile,
    costs: &Costs,
    entry: i64,
    volume: i64,
    side: Side,
    best_price: i64,
) -> Result<i128> {
    let best_movement = best_price
        .checked_sub(entry)
        .and_then(|v| v.checked_mul(side.sign()))
        .ok_or("estimate movement overflow")?;
    let net_best = contracts::pnl(p, best_movement, volume, Conversion::usd())?
        .checked_sub(
            costs
                .commission(volume)
                .checked_mul(2)
                .ok_or("estimate fee overflow")?,
        )
        .ok_or("estimate net overflow")?;
    Ok(net_best)
}

/// Conservative pessimistic/optimistic net-outcome bounds for one screened
/// candidate, derived from the candle estimate's explicit uncertainty set.
/// Diagnostic only: not candidate acceptance, not scientific evidence, and
/// never by itself a basis for skipping tick replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetBounds {
    pub version: String,
    pub closed_trades: usize,
    pub pessimistic_net_micro_usd: i128,
    pub optimistic_net_micro_usd: i128,
    pub pessimistic_wins: usize,
    pub optimistic_wins: usize,
    pub pessimistic_win_rate_bps: Option<usize>,
    pub optimistic_win_rate_bps: Option<usize>,
}

/// Where a candle-screened candidate lands relative to the promotion threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenClassification {
    /// Even the optimistic bound cannot reach the threshold: failure proven
    /// from the candle model alone, safe to reject without tick replay.
    ProvenScreenFailure,
    /// The bound set straddles the threshold or the sample is too thin:
    /// uncertainty preserved, escalate to tick verification.
    ScreenUncertain,
    /// Not estimable (unsupported, unresolved, or below the minimum count):
    /// not screened by the candle model.
    NotScreened,
}

/// Aggregate screened-candidate audit coverage across a batch or campaign.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AuditCoverage {
    pub candidates: usize,
    pub bound_rejections: usize,
    pub proven_failures: usize,
    pub promoted_to_tick: usize,
    pub sampled_for_audit: usize,
    pub audited: usize,
    pub false_negatives: usize,
    pub false_negative_rate_bps: Option<u32>,
    pub coverage_bps: u32,
}

/// Aggregate per-candidate candle-screen audits (the `candidate_screen_audit`
/// value persisted with each experiment result) into batch coverage.
/// `candidates` counts audits carrying a shadow decision; rejections are the
/// `would_reject` set; `audited` counts rejections with a resolved replay
/// outcome; false negatives are rejected candidates the replayed gate clears.
pub fn audit_coverage(audits: &[serde_json::Value]) -> AuditCoverage {
    let mut out = AuditCoverage {
        candidates: audits.len(),
        ..Default::default()
    };
    for audit in audits {
        let Some(decision) = audit
            .get("shadow_decision")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        let would_reject = decision.get("would_reject").and_then(|v| v.as_bool()) == Some(true);
        if !would_reject {
            out.promoted_to_tick += 1;
            continue;
        }
        out.bound_rejections += 1;
        if decision
            .get("bound_proven_failure")
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            out.proven_failures += 1;
        }
        if decision
            .get("selected_for_sampled_audit")
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            out.sampled_for_audit += 1;
        }
        let resolved = audit.get("actual_unresolved").and_then(|v| v.as_bool()) != Some(true);
        if resolved {
            out.audited += 1;
            if audit.get("shadow_false_negative").and_then(|v| v.as_bool()) == Some(true) {
                out.false_negatives += 1;
            }
        }
    }
    out.false_negative_rate_bps = if out.audited > 0 {
        out.false_negatives
            .checked_mul(10000)
            .and_then(|v| v.checked_div(out.audited))
            .map(|v| v as u32)
    } else {
        None
    };
    out.coverage_bps = if out.bound_rejections > 0 {
        out.audited
            .checked_mul(10000)
            .and_then(|v| v.checked_div(out.bound_rejections))
            .map(|v| v as u32)
            .unwrap_or(0)
    } else {
        0
    };
    out
}

/// Maximum persisted `candidate_screen_audit` markers aggregated by one registry
/// coverage read. The bound keeps the inspection explicit: a larger registry is
/// reported as truncated rather than silently sampled.
pub const REGISTRY_COVERAGE_SCAN_LIMIT: usize = 10_000;

/// Registry-wide aggregate of persisted candidate-screen audit markers.
/// `results_without_marker` is reported explicitly and is never coverage: a
/// result that never carried an audit marker is not counted as audited, so
/// marker absence cannot inflate coverage.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RegistryAuditCoverage {
    pub version: &'static str,
    /// Completed experiments with a stored result.
    pub results_total: usize,
    /// Results carrying a `candidate_screen_audit` marker.
    pub results_with_marker: usize,
    /// Results without the marker. Never coverage.
    pub results_without_marker: usize,
    /// Marker values actually aggregated (<= `scan_limit`).
    pub scanned_audits: usize,
    pub scan_limit: usize,
    /// True when more markers exist than `scan_limit`.
    pub truncated: bool,
    /// Aggregate over the scanned markers.
    pub coverage: AuditCoverage,
    pub limitations: &'static str,
}

/// Aggregate every persisted candidate-screen audit marker in the registry.
/// Request-time `Registry::integrity` refusal protects against a corrupt
/// registry and each result is hash-verified by `Registry::result`. Read-only
/// and diagnostic: no execution, no acceptance, no hard OS/worker enforcement.
pub fn registry_coverage(
    registry: &crate::storage::Registry,
    scan_limit: usize,
) -> Result<RegistryAuditCoverage> {
    if scan_limit == 0 || scan_limit > REGISTRY_COVERAGE_SCAN_LIMIT {
        return Err("registry audit coverage scan limit outside 1..=10000".into());
    }
    registry.integrity()?;
    let mut statement = registry
        .connection
        .prepare("SELECT id FROM experiments WHERE result_json IS NOT NULL ORDER BY rowid")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut results_total = 0_usize;
    let mut results_with_marker = 0_usize;
    let mut audits: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;
    for row in rows {
        let id = row.map_err(|e| e.to_string())?;
        let Some(stored) = registry.result(&id)? else {
            continue;
        };
        results_total += 1;
        let Some(audit) = stored.result.get("candidate_screen_audit") else {
            continue;
        };
        results_with_marker += 1;
        if audits.len() < scan_limit {
            audits.push(audit.clone());
        } else {
            truncated = true;
        }
    }
    let coverage = audit_coverage(&audits);
    Ok(RegistryAuditCoverage {
        version: "registry-audit-coverage-v1",
        results_total,
        results_with_marker,
        results_without_marker: results_total - results_with_marker,
        scanned_audits: audits.len(),
        scan_limit,
        truncated,
        coverage,
        limitations: "Registry-local persisted candidate-screen audit markers only. Results without a marker are reported, never treated as coverage. coverage_bps is the share of bound rejections with a resolved replay audit; it is diagnostic, not acceptance, not a hard OS/worker control and not qualification evidence. Marker absence does not establish audit coverage, completeness or correctness.",
    })
}

pub fn compare(
    estimate: &Estimate,
    replay: &Replay,
    experiment_id: &str,
    policy: &ShadowPolicy,
) -> Result<serde_json::Value> {
    let decision = policy.decision(estimate, experiment_id)?;
    // Explicit Stage-1 award against the versioned 40.00% floor: exactly 40.00%
    // passes, below never passes, and ambiguous/unsupported estimates hold.
    let stage1 = policy.stage1_decision(estimate)?;
    let wins = replay.trades.iter().filter(|t| t.net_micro_usd > 0).count();
    let actual_unresolved = replay.unresolved.is_some() || replay.pending_entry.is_some();
    let false_negative = if decision["would_reject"] == true && !actual_unresolved {
        Some(
            replay.trades.len() >= policy.minimum_closed_trades
                && wins * 10000 >= policy.threshold_bps * replay.trades.len(),
        )
    } else {
        None
    };
    Ok(serde_json::json!({
        "version":"m1-estimate-vs-tick-v1", "estimated":estimate,
        "stage1_decision":stage1.as_str(),
        "actual_closed_trades":replay.trades.len(), "actual_net_wins":wins,
        "actual_net_win_rate_bps":if replay.trades.is_empty(){None}else{Some(wins * 10000 / replay.trades.len())},
        "actual_unresolved":actual_unresolved, "shadow_decision":decision,
        "shadow_false_negative":false_negative,
        "candidate_rejected":false, "false_negative_audit":"all-candidates-replayed-sample-selection-diagnostic-only",
        "qualification_evidence":false
    }))
}

#[cfg(test)]
mod stage1_policy_tests {
    use super::*;

    fn estimate(wins: usize, trades: usize, uncertainties: &[&str]) -> Estimate {
        Estimate {
            version: VERSION.into(),
            interpretation: String::new(),
            status: "estimated".into(),
            estimated_closed_trades: trades,
            estimated_net_wins: wins,
            estimated_net_micro_usd: 0,
            estimated_net_win_rate_bps: Some((wins * 10000) / trades),
            estimated_sizing_rejections: 0,
            unavailable_stop_signals: 0,
            overlapping_signals_skipped: 0,
            bar_visits: trades,
            uncertainties: uncertainties.iter().map(|s| (*s).into()).collect(),
            promotes_to_tick: true,
            net_bounds: None,
            optimistic_net_micro_usd: 0,
            optimistic_wins: wins,
        }
    }

    #[test]
    fn stage1_floor_is_exactly_40_percent_inclusive() {
        let policy = ShadowPolicy::default();
        assert_eq!(policy.threshold_bps, 4000);
        // Exactly 40% passes.
        assert_eq!(
            policy.stage1_decision(&estimate(40, 100, &[])).unwrap(),
            Stage1Decision::Pass
        );
        // Just below the floor rejects.
        assert_eq!(
            policy.stage1_decision(&estimate(39, 100, &[])).unwrap(),
            Stage1Decision::Reject
        );
    }

    #[test]
    fn ambiguous_and_thin_estimates_hold_not_pass() {
        let policy = ShadowPolicy::default();
        // Ambiguity holds even when the point estimate is above the floor.
        assert_eq!(
            policy
                .stage1_decision(&estimate(
                    60,
                    100,
                    &["SL-TP-order-unknown-stop-proxy-selected"]
                ))
                .unwrap(),
            Stage1Decision::Hold
        );
        // A thin sample holds rather than passing or rejecting.
        assert_eq!(
            policy.stage1_decision(&estimate(1, 1, &[])).unwrap(),
            Stage1Decision::Hold
        );
        // Unsupported status holds.
        let mut unsupported = estimate(40, 100, &[]);
        unsupported.status = "unsupported".into();
        assert_eq!(
            policy.stage1_decision(&unsupported).unwrap(),
            Stage1Decision::Hold
        );
    }

    #[test]
    fn proven_optimistic_bound_below_floor_rejects() {
        let policy = ShadowPolicy::default();
        let mut e = estimate(50, 100, &[]);
        e.net_bounds = Some(NetBounds {
            version: "candidate-net-bounds-v1".into(),
            closed_trades: 100,
            pessimistic_net_micro_usd: 0,
            optimistic_net_micro_usd: 0,
            pessimistic_wins: 30,
            optimistic_wins: 30,
            pessimistic_win_rate_bps: Some(3000),
            optimistic_win_rate_bps: Some(3000),
        });
        assert_eq!(policy.stage1_decision(&e).unwrap(), Stage1Decision::Reject);
    }
}
