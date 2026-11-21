//! Scientific validation over labelled execution evidence.
//!
//! Every fixture here is synthetic and exists only to exercise the evaluator. The evaluator is
//! fail-closed: diagnostic or ineligible evidence is rejected, an eligible-looking report with
//! unknown continuous accounting stays `INSUFFICIENT_EVIDENCE`, and `qualification_granted` is
//! always false.

use rust_strategy_factory::historical_corpus::ExecutionReport;
use rust_strategy_factory::scientific_validation::{self, METHOD, PLANNED_RR_MILLI};

struct MonthOutcome {
    year: u32,
    month: u32,
    resolved: usize,
    wins: usize,
    losses: usize,
    breakevens: usize,
    streak: usize,
    gross_win: i128,
    gross_loss: i128,
    net: i128,
    unresolved: usize,
}

fn month_json(item: &MonthOutcome) -> serde_json::Value {
    serde_json::json!({
        "year": item.year,
        "month": item.month,
        "classification": "diagnostic_simulation",
        "eligibility": "none",
        "corpus_identity": "a".repeat(64),
        "source_dataset_hash": "b".repeat(64),
        "replay_dataset_id": "c".repeat(64),
        "declared_class": "RealStructurallyValid",
        "profile_id": "d".repeat(64),
        "conversion_policy": "identity_usd_v1",
        "initial_micro_usd": 20_000_000_i128,
        "realized_micro_usd": 20_000_000_i128,
        "final_marked_micro_usd": 20_000_000_i128,
        "maximum_drawdown_micro_usd": 0_i128,
        "trades": item.resolved,
        "rejections": 0,
        "receipt_hash": "e".repeat(64),
        "outcomes": {
            "resolved": item.resolved,
            "wins": item.wins,
            "losses": item.losses,
            "breakevens": item.breakevens,
            "max_losing_streak": item.streak,
            "gross_win_micro_usd": item.gross_win,
            "gross_loss_micro_usd": item.gross_loss,
            "net_micro_usd": item.net,
            "unresolved_positions": item.unresolved,
        }
    })
}

fn totals_json(months: &[MonthOutcome], pnl_micro_usd: i128) -> serde_json::Value {
    let (resolved, wins, losses, breakevens, streak, gross_win, gross_loss, net, unresolved) =
        months.iter().fold(
            (
                0usize, 0usize, 0usize, 0usize, 0usize, 0_i128, 0_i128, 0_i128, 0usize,
            ),
            |acc, item| {
                (
                    acc.0 + item.resolved,
                    acc.1 + item.wins,
                    acc.2 + item.losses,
                    acc.3 + item.breakevens,
                    acc.4.max(item.streak),
                    acc.5 + item.gross_win,
                    acc.6 + item.gross_loss,
                    acc.7 + item.net,
                    acc.8 + item.unresolved,
                )
            },
        );
    serde_json::json!({
        "months_executed": months.len(),
        "months_refused": 0,
        "trades": resolved,
        "rejections": 0,
        "initial_micro_usd": 20_000_000_i128 * months.len() as i128,
        "pnl_micro_usd": pnl_micro_usd,
        "maximum_drawdown_micro_usd": 0_i128,
        "wins": wins,
        "losses": losses,
        "breakevens": breakevens,
        "max_losing_streak": streak,
        "gross_win_micro_usd": gross_win,
        "gross_loss_micro_usd": gross_loss,
        "net_micro_usd": net,
        "unresolved_positions": unresolved,
    })
}

fn report(
    classification: &str,
    eligibility: &str,
    months: &[MonthOutcome],
    pnl_micro_usd: i128,
) -> ExecutionReport {
    serde_json::from_value(serde_json::json!({
        "instrument": "EURUSD",
        "identity": "a".repeat(64),
        "strategy_identity": "b".repeat(64),
        "classification": classification,
        "eligibility": eligibility,
        "conversion_policy": "identity_usd_v1",
        "months": months.iter().map(month_json).collect::<Vec<_>>(),
        "refusals": [],
        "totals": totals_json(months, pnl_micro_usd),
        "cache_hits": 0,
        "cache_misses": months.len(),
        "limitations": "synthetic fixture",
    }))
    .unwrap()
}

fn passing_months() -> Vec<MonthOutcome> {
    vec![
        MonthOutcome {
            year: 2025,
            month: 1,
            resolved: 60,
            wins: 40,
            losses: 15,
            breakevens: 5,
            streak: 3,
            gross_win: 120_000_000,
            gross_loss: 30_000_000,
            net: 90_000_000,
            unresolved: 0,
        },
        MonthOutcome {
            year: 2026,
            month: 2,
            resolved: 55,
            wins: 30,
            losses: 20,
            breakevens: 5,
            streak: 2,
            gross_win: 60_000_000,
            gross_loss: 40_000_000,
            net: 20_000_000,
            unresolved: 1,
        },
    ]
}

#[test]
fn summarize_computes_owner_criteria_descriptively() {
    let validation = scientific_validation::summarize(&report(
        "diagnostic_simulation",
        "none",
        &passing_months(),
        500_000_000,
    ))
    .unwrap();
    assert_eq!(validation.resolved, 115);
    assert_eq!(
        (validation.wins, validation.losses, validation.breakevens),
        (70, 35, 10)
    );
    assert_eq!(validation.net_win_rate_bps, 6_086);
    assert_eq!(validation.max_losing_streak, 3);
    assert_eq!(validation.gross_win_micro_usd, 180_000_000);
    assert_eq!(validation.gross_loss_micro_usd, 70_000_000);
    assert_eq!(validation.achieved_rr_milli, Some(2_571));
    assert_eq!(validation.planned_rr_milli, PLANNED_RR_MILLI);
    assert_eq!(
        validation.additive_ending_micro_usd,
        20_000_000 + 500_000_000
    );
    assert_eq!(validation.year_2025.resolved, 60);
    assert_eq!(validation.year_2026.resolved, 55);
    assert_eq!(validation.unresolved_positions, 1);
    // The numerical criteria pass, but the declared class is diagnostic: rejected, never
    // qualified.
    assert!(validation.criteria.numeric_gates_passed);
    assert_eq!(validation.criteria.status, "REJECTED");
    assert!(!validation.criteria.qualification_granted);
    assert_eq!(validation.criteria.scope, scientific_validation::SCOPE);
}

#[test]
fn eligible_class_with_unknown_continuous_accounting_is_never_qualified() {
    let validation = scientific_validation::summarize(&report(
        "registered_full_period_replay",
        "eligible",
        &passing_months(),
        500_000_000,
    ))
    .unwrap();
    assert!(validation.criteria.numeric_gates_passed);
    assert_eq!(validation.criteria.status, "INSUFFICIENT_EVIDENCE");
    assert!(!validation.criteria.qualification_granted);
    let continuous = validation
        .criteria
        .criteria
        .iter()
        .find(|item| item.name == "continuous_full_period_accounting")
        .unwrap();
    assert_eq!(continuous.state, "unknown");
}

#[test]
fn numeric_failure_rejects_even_with_an_eligible_class() {
    let mut months = passing_months();
    months[0].wins = 10;
    months[0].losses = 45;
    let validation = scientific_validation::summarize(&report(
        "registered_full_period_replay",
        "eligible",
        &months,
        500_000_000,
    ))
    .unwrap();
    assert!(!validation.criteria.numeric_gates_passed);
    assert_eq!(validation.criteria.status, "REJECTED");
    assert!(!validation.criteria.qualification_granted);
    assert!(
        validation
            .criteria
            .reasons
            .iter()
            .any(|reason| reason.contains("net_win_rate failed")),
        "{:?}",
        validation.criteria.reasons
    );
}

#[test]
fn totals_that_do_not_reconcile_with_months_are_refused() {
    let mut parsed = report("diagnostic_simulation", "none", &passing_months(), 0);
    parsed.totals.trades += 1;
    let error = scientific_validation::summarize(&parsed)
        .expect_err("a totals/months mismatch must be refused");
    assert!(error.contains("reconcile"), "{error}");
}

#[test]
fn months_outside_the_approved_years_are_refused() {
    let mut months = passing_months();
    months[0].year = 2024;
    let error =
        scientific_validation::summarize(&report("diagnostic_simulation", "none", &months, 0))
            .expect_err("a 2024 month must be refused");
    assert!(error.contains("2025/2026"), "{error}");
}

#[test]
fn summarize_all_aggregates_and_never_qualifies() {
    let reports = vec![
        report(
            "diagnostic_simulation",
            "none",
            &passing_months(),
            500_000_000,
        ),
        report(
            "diagnostic_simulation",
            "none",
            &passing_months(),
            500_000_000,
        ),
    ];
    let validation = scientific_validation::summarize_all(&reports).unwrap();
    assert_eq!(validation.method, METHOD);
    assert_eq!(validation.totals.instruments, 2);
    assert_eq!(validation.totals.resolved, 230);
    assert_eq!(validation.totals.wins, 140);
    assert_eq!(validation.totals.net_win_rate_bps, 6_086);
    assert_eq!(
        validation.totals.additive_ending_micro_usd,
        2 * (20_000_000 + 500_000_000)
    );
    assert_eq!(validation.classification, "diagnostic_simulation");
    assert_eq!(validation.eligibility, "none");
    assert!(!validation.qualification_granted);
    assert!(validation.limitations.contains("Reset-fold"));
}

#[test]
fn win_rate_is_zero_when_nothing_resolved() {
    let months = vec![MonthOutcome {
        year: 2025,
        month: 1,
        resolved: 0,
        wins: 0,
        losses: 0,
        breakevens: 0,
        streak: 0,
        gross_win: 0,
        gross_loss: 0,
        net: 0,
        unresolved: 1,
    }];
    let validation =
        scientific_validation::summarize(&report("diagnostic_simulation", "none", &months, 0))
            .unwrap();
    assert_eq!(validation.net_win_rate_bps, 0);
    assert_eq!(validation.achieved_rr_milli, None);
    assert!(!validation.criteria.numeric_gates_passed);
}
