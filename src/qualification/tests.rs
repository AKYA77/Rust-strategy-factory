//! Disposable abstract/control-flow fixtures. Positive branches simulate the
//! approved-evidence path only; they assert no real market evidence, broker approval
//! or scientific qualification. Synthetic data must never qualify.
use super::*;
use crate::{
    candles::Timeframe,
    contracts::Side,
    storage::{ExperimentKey, Stage},
};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use std::fs;

const PASSWORD: &str = "fixture-only-qualification-reviewer";
const SUBMITTER: &str = "fixture-preparer";
const SOURCE: &str = "DISPOSABLE_TEST_SOURCE";

fn hex64(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}
fn strategy(window: usize) -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::Donchian {
            window,
            above: true,
        },
        entry: None,
        stop_policy: None,
    }
}
fn stats(
    resolved: usize,
    wins: usize,
    losses: usize,
    breakeven: usize,
    streak: usize,
) -> PeriodStats {
    PeriodStats {
        resolved,
        wins,
        losses,
        breakeven,
        max_losing_streak: streak,
    }
}
fn cutoff_2026() -> i64 {
    NaiveDate::from_ymd_opt(2026, 10, 31)
        .unwrap()
        .and_hms_opt(23, 59, 59)
        .unwrap()
        .and_utc()
        .timestamp_millis()
}
fn accounting() -> AccountingEvidence {
    AccountingEvidence {
        experiment_id: hex64('e'),
        result_hash: hex64('f'),
        classification: FULL_PERIOD_CLASSIFICATION.into(),
        cutoff_ms: cutoff_2026(),
        pooled: stats(200, 120, 60, 20, 3),
        year_2025: stats(100, 60, 30, 10, 3),
        year_2026: stats(100, 60, 30, 10, 3),
        net_micro_usd: 600_000_000,
        ending_micro_usd: 620_000_000,
        max_drawdown_micro_usd: 50_000_000,
        unresolved: 0,
    }
}
fn science_gate() -> evidence_gates::Assessment {
    evidence_gates::Assessment {
        method: evidence_gates::VERSION.into(),
        policy: evidence_gates::Policy {
            minimum_folds: 2,
            minimum_resolved_per_fold: 20,
            minimum_resolved_per_year: 50,
            minimum_net_win_rate_bps: 5000,
            maximum_consecutive_losses: 5,
            minimum_net_micro_usd: 1,
        },
        state: "NUMERICAL_EVIDENCE_ONLY".into(),
        criteria_passed: true,
        qualification_granted: false,
        decisions: Vec::new(),
        remaining_blockers: vec!["abstract fixture; no market evidence".into()],
    }
}
fn science() -> ScienceEvidence {
    ScienceEvidence {
        protocol_id: hex64('1'),
        report_hash: hex64('2'),
        report_data_class: DataClass::RealProfileApproved,
        gate: science_gate(),
        robustness_cases_passed: 4,
        robustness_cases_total: 4,
        uncertainty_present: true,
        block_uncertainty_present: true,
    }
}
fn bundle() -> EvidenceBundle {
    let s = strategy(20);
    let costs = Costs::default();
    let policy = Policy::owner_selected().unwrap();
    EvidenceBundle {
        instrument: "EURUSD".into(),
        concept_id: s.identity().unwrap(),
        strategy: s,
        declared_class: DataClass::RealProfileApproved,
        profile_research_only: false,
        dataset_identity: hex64('a'),
        dataset_hash: hex64('b'),
        source: SOURCE.into(),
        submitter: SUBMITTER.into(),
        profile_hash: hex64('c'),
        cost_hash: data::hash(&costs).unwrap(),
        costs,
        execution: ExecutionIdentity {
            engine: crate::ENGINE_VERSION.into(),
            indicators: crate::INDICATOR_VERSION.into(),
            signals: crate::SIGNAL_VERSION.into(),
            compute_revision: hex64('3'),
        },
        data_approval: DataApprovalEvidence {
            catalog_identity: hex64('6'),
            machine_id: hex64('5'),
            state: "DATA_APPROVED_FOR_SCOPE".into(),
            reasons: Vec::new(),
            review_head: Some(hex64('4')),
            live_machine_revalidated: true,
            scope: Interval {
                start_ms: policy.required_coverage_start_ms,
                end_exclusive_ms: policy.required_coverage_end_exclusive_ms,
            },
        },
        coverage: CoverageEvidence {
            receipt_hash: hex64('7'),
            catalog_identity: hex64('6'),
            start_ms: policy.required_coverage_start_ms,
            end_exclusive_ms: policy.required_coverage_end_exclusive_ms,
            months: policy.required_months().unwrap(),
            complete: true,
            events: 1_000_000,
        },
        accounting: accounting(),
        science: science(),
    }
}
fn reviewer(_areas: &[Area]) -> crate::review_authority::Reviewer {
    let salt = SaltString::encode_b64(b"fixture-only-salt").unwrap();
    crate::review_authority::Reviewer {
        name: "independent-reviewer".into(),
        password_hash: Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &salt)
            .unwrap()
            .to_string(),
        grants: vec![
            crate::review_authority::Grant {
                symbol: "EURUSD".into(),
                source: SOURCE.into(),
                submitter: SUBMITTER.into(),
                areas: vec![
                    Area::Source,
                    Area::Profile,
                    Area::Calendar,
                    Area::Quality,
                    Area::Coverage,
                ],
                expires_ms: chrono::Utc::now().timestamp_millis() + 86_400_000,
            },
            crate::review_authority::Grant {
                symbol: "EURUSD".into(),
                source: SOURCE.into(),
                submitter: SUBMITTER.into(),
                areas: vec![Area::ScientificQualification],
                expires_ms: chrono::Utc::now().timestamp_millis() + 86_400_000,
            },
        ],
    }
}
struct Temp {
    root: std::path::PathBuf,
}
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "qualification-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.root.join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn authority_fixture(temp: &Temp, name: &str, areas: Vec<Area>) -> Authority {
    let path = temp.path(name);
    Authority::initialize(&path, vec![reviewer(&areas)]).unwrap();
    Authority::load(&path).unwrap()
}
fn request(kind: DecisionKind, head: Option<String>) -> Request {
    Request {
        decision: kind,
        expected_head: head,
        submitter: SUBMITTER.into(),
        explanation: "TEST CONTROL FLOW ONLY; not market evidence".into(),
        evidence_text: "Disposable qualification fixture decision".into(),
    }
}

// === Policy and gate boundaries ===

#[test]
fn policy_floors_and_owner_window_are_exact() {
    let p = Policy::owner_selected().unwrap();
    p.validate().unwrap();
    let months = p.required_months().unwrap();
    assert_eq!(months.len(), 22);
    assert_eq!(months[0], "2025-01");
    assert_eq!(months[21], "2026-10");
    for bad in [
        Policy {
            minimum_net_win_rate_bps: 4999,
            ..p.clone()
        },
        Policy {
            minimum_resolved_per_year: 49,
            ..p.clone()
        },
        Policy {
            maximum_consecutive_losses: 6,
            ..p.clone()
        },
        Policy {
            minimum_ending_equity_micro_usd: 499_999_999,
            ..p.clone()
        },
        Policy {
            maximum_per_instrument: 6,
            ..p.clone()
        },
        Policy {
            maximum_per_diversity_class: 0,
            ..p.clone()
        },
        Policy {
            required_coverage_start_ms: p.required_coverage_start_ms + 1,
            ..p.clone()
        },
        Policy {
            required_coverage_end_exclusive_ms: p.required_coverage_end_exclusive_ms - 1,
            ..p.clone()
        },
    ] {
        assert!(bad.validate().is_err());
    }
    // Stricter policies remain valid.
    Policy {
        minimum_net_win_rate_bps: 6000,
        minimum_resolved_per_year: 80,
        ..p.clone()
    }
    .validate()
    .unwrap();
}
#[test]
fn complete_abstract_evidence_passes_gates_but_never_grants_qualification() {
    let b = bundle();
    let p = Policy::owner_selected().unwrap();
    let a = assess(&b, &p).unwrap();
    assert_eq!(a.state, "QUALIFICATION_EVIDENCE_COMPLETE");
    assert!(!a.qualification_granted);
    assert!(a.reasons.is_empty());
    assert_eq!(a.gates.len(), 10);
    assert!(a.gates.iter().all(|g| g.state == GateState::Passed));
}
#[test]
fn exact_numerical_boundaries_pass_and_one_notch_below_fails() {
    let p = Policy::owner_selected().unwrap();
    // Exactly 50.00% passes; break-even trades count in the denominator only.
    let mut b = bundle();
    b.accounting.pooled = stats(200, 100, 80, 20, 3);
    b.accounting.year_2025 = stats(100, 50, 40, 10, 3);
    b.accounting.year_2026 = stats(100, 50, 40, 10, 3);
    assert_eq!(
        assess(&b, &p).unwrap().state,
        "QUALIFICATION_EVIDENCE_COMPLETE"
    );
    b.accounting.pooled = stats(200, 99, 81, 20, 3);
    b.accounting.year_2025 = stats(100, 50, 40, 10, 3);
    b.accounting.year_2026 = stats(100, 49, 41, 10, 3);
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    // Exactly 50 resolved per year passes; 49 fails.
    let mut b = bundle();
    b.accounting.pooled = stats(100, 60, 30, 10, 3);
    b.accounting.year_2025 = stats(50, 30, 15, 5, 3);
    b.accounting.year_2026 = stats(50, 30, 15, 5, 3);
    assert_eq!(
        assess(&b, &p).unwrap().state,
        "QUALIFICATION_EVIDENCE_COMPLETE"
    );
    b.accounting.year_2026 = stats(49, 30, 14, 5, 3);
    b.accounting.pooled = stats(99, 60, 29, 10, 3);
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    // Losing streak exactly five passes; six fails; break-even does not erase a streak.
    let mut b = bundle();
    b.accounting.pooled.max_losing_streak = 5;
    b.accounting.year_2025.max_losing_streak = 5;
    assert_eq!(
        assess(&b, &p).unwrap().state,
        "QUALIFICATION_EVIDENCE_COMPLETE"
    );
    b.accounting.pooled.max_losing_streak = 6;
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    // Equity exactly at the $500 objective passes; one micro-dollar below fails.
    let mut b = bundle();
    b.accounting.net_micro_usd = 480_000_000;
    b.accounting.ending_micro_usd = 500_000_000;
    assert_eq!(
        assess(&b, &p).unwrap().state,
        "QUALIFICATION_EVIDENCE_COMPLETE"
    );
    b.accounting.ending_micro_usd = 499_999_999;
    b.accounting.net_micro_usd = 479_999_999;
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    // Unresolved positions can never be silently excluded.
    let mut b = bundle();
    b.accounting.unresolved = 1;
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
}
#[test]
fn synthetic_diagnostic_and_structurally_valid_only_evidence_never_completes() {
    let p = Policy::owner_selected().unwrap();
    let mut b = bundle();
    b.declared_class = DataClass::SyntheticTestOnly;
    b.science.report_data_class = DataClass::SyntheticTestOnly;
    let a = assess(&b, &p).unwrap();
    assert_eq!(a.state, "REJECTED");
    assert!(!a.qualification_granted);
    assert!(a.reasons.iter().any(|r| r.contains("never qualifies")));
    let mut b = bundle();
    b.accounting.classification = "diagnostic_simulation".into();
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    let mut b = bundle();
    b.declared_class = DataClass::RealStructurallyValid;
    b.science.report_data_class = DataClass::RealStructurallyValid;
    b.data_approval.state = "DATA_BLOCKED".into();
    b.data_approval.reasons = vec!["approvals absent".into()];
    b.data_approval.review_head = None;
    let a = assess(&b, &p).unwrap();
    assert_eq!(a.state, "HELD");
    assert!(!a.qualification_granted);
    let mut b = bundle();
    b.declared_class = DataClass::Quarantined;
    b.science.report_data_class = DataClass::Quarantined;
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
}
#[test]
fn each_missing_evidence_requirement_holds_or_rejects_independently() {
    let p = Policy::owner_selected().unwrap();
    let mut b = bundle();
    b.data_approval.state = "DATA_BLOCKED".into();
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.coverage.complete = false;
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.coverage.months.pop();
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.coverage.events = 0;
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.science.gate.criteria_passed = false;
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.science.robustness_cases_passed = 3;
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.execution.engine = "stale-engine".into();
    assert_eq!(assess(&b, &p).unwrap().state, "HELD");
    let mut b = bundle();
    b.profile_research_only = true;
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    let mut b = bundle();
    b.cost_hash = hex64('9');
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    let mut b = bundle();
    b.costs.spread_floor_pips = 1;
    b.cost_hash = data::hash(&b.costs).unwrap();
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    let mut b = bundle();
    b.concept_id = hex64('0');
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    let mut b = bundle();
    b.accounting.year_2025.resolved += 1;
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
    let mut b = bundle();
    b.accounting.cutoff_ms = 1_700_000_000_000; // 2023: outside the approved YTD contract
    assert_eq!(assess(&b, &p).unwrap().state, "REJECTED");
}
#[test]
fn diversity_class_groups_parameter_tweaks_and_ranking_is_exact() {
    let a = strategy(20);
    let b = strategy(30);
    assert_eq!(diversity_class(&a), diversity_class(&b));
    let c = Strategy {
        rule: Rule::RsiCross {
            window: 14,
            basis_points: 5000,
            above: true,
        },
        ..a.clone()
    };
    assert_ne!(diversity_class(&a), diversity_class(&c));
    let base = bundle();
    let s1 = score(&base).unwrap();
    let s2 = score(&base).unwrap();
    assert_eq!(s1, s2);
    assert_eq!(s1.method, RANKING_VERSION);
    assert!(s1.total >= 0 && s1.total <= 100);
    assert_eq!(s1.total, s1.components.iter().map(|(_, v)| v).sum::<i128>());
    let mut better = base.clone();
    better.accounting.pooled.wins = 160;
    better.accounting.pooled.losses = 20;
    better.accounting.year_2025.wins = 80;
    better.accounting.year_2025.losses = 10;
    better.accounting.year_2026.wins = 80;
    better.accounting.year_2026.losses = 10;
    assert!(score(&better).unwrap().total > s1.total);
    let mut empty = base;
    empty.accounting.pooled.resolved = 0;
    assert!(score(&empty).is_err());
}

// === Store: immutability, fencing, boards, archive ===

#[test]
fn decisions_are_immutable_and_head_fenced() {
    let temp = Temp::new("immutable");
    let authority = authority_fixture(
        &temp,
        "authority.secret",
        vec![Area::ScientificQualification],
    );
    let mut store = Store::open(&temp.path("store.sqlite")).unwrap();
    let b = bundle();
    let p = Policy::owner_selected().unwrap();
    let principal = authority
        .authenticate("independent-reviewer", PASSWORD)
        .unwrap();
    let e = store
        .decide(
            &authority,
            &principal,
            request(DecisionKind::Reject, None),
            &b,
            &p,
            None,
        )
        .unwrap();
    // Direct mutation is refused by the schema.
    assert!(
        store
            .connection
            .execute(
                "UPDATE qualification_decisions SET authenticator=?1 WHERE sequence=1",
                [hex64('0')]
            )
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM qualification_decisions WHERE sequence=1", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute(
                "UPDATE qualification_boards SET body=X'00' WHERE sequence=1",
                []
            )
            .is_err()
    );
    // Stale head refuses; the exact head succeeds.
    assert!(
        store
            .decide(
                &authority,
                &principal,
                request(DecisionKind::Reject, Some(hex64('0'))),
                &b,
                &p,
                None
            )
            .is_err()
    );
    let e2 = store
        .decide(
            &authority,
            &principal,
            request(DecisionKind::Reject, Some(e.hash.clone())),
            &b,
            &p,
            None,
        )
        .unwrap();
    assert_eq!(e2.record.sequence, 2);
    assert_eq!(e2.record.request.expected_head, Some(e.hash));
    // Self-review and wrong submitter scopes refuse.
    let mut bad = request(DecisionKind::Reject, Some(e2.hash.clone()));
    bad.submitter = "someone-else".into();
    assert!(
        store
            .decide(&authority, &principal, bad, &b, &p, None)
            .is_err()
    );
    // A tampered record body fails content-hash integrity on decode.
    let mut tampered = e2.record.clone();
    tampered.request.explanation = "rewritten history".into();
    let body = serde_json::to_vec(&tampered).unwrap();
    assert!(
        decode(
            e2.record.sequence,
            e2.hash.clone(),
            body,
            e2.authenticator.clone()
        )
        .is_err()
    );
    let state = store
        .scope_state("EURUSD", &b.concept_id, &authority)
        .unwrap();
    assert_eq!(state.events.len(), 2);
    assert_eq!(state.current_state, "REJECTED");
}
#[test]
fn qualify_refuses_without_complete_gates_or_live_revalidation() {
    let temp = Temp::new("refuse");
    let authority = authority_fixture(
        &temp,
        "authority.secret",
        vec![Area::ScientificQualification],
    );
    let mut store = Store::open(&temp.path("store.sqlite")).unwrap();
    let p = Policy::owner_selected().unwrap();
    let principal = authority
        .authenticate("independent-reviewer", PASSWORD)
        .unwrap();
    let mut synthetic = bundle();
    synthetic.declared_class = DataClass::SyntheticTestOnly;
    synthetic.science.report_data_class = DataClass::SyntheticTestOnly;
    assert!(
        store
            .decide(
                &authority,
                &principal,
                request(DecisionKind::Qualify, None),
                &synthetic,
                &p,
                None
            )
            .is_err()
    );
    let b = bundle();
    assert!(
        store
            .decide(
                &authority,
                &principal,
                request(DecisionKind::Qualify, None),
                &b,
                &p,
                None
            )
            .is_err()
    );
    // Nothing was published.
    let board = store.board("EURUSD", &authority).unwrap();
    assert!(board.entries.is_empty());
    assert!(!board.empty_reasons.is_empty());
    assert!(store.latest_board_snapshot("EURUSD").unwrap().is_none());
}
/// Disposable live artifacts: a catalog with the simulated five-area approval branch and a
/// registry holding one completed full-period experiment plus one frozen protocol report.
/// These fixtures exercise control flow only; they are not acquired market evidence.
struct LiveFixture {
    temp: Temp,
    catalog: crate::catalog::Catalog,
    registry: Registry,
    authority: Authority,
    archive: std::path::PathBuf,
    dataset: data::Dataset,
    bundle: EvidenceBundle,
}
impl LiveFixture {
    fn new(name: &str) -> Self {
        let temp = Temp::new(name);
        let archive = temp.path("source.csv");
        fs::write(
            &archive,
            b"timestamp_ms,sequence,bid,ask\n0,1,1.00000,1.00010\n1000,2,1.00020,1.00030\n",
        )
        .unwrap();
        let mut profile = crate::contracts::Profile::synthetic("EURUSD").unwrap();
        profile.research_only = false;
        profile.version = "DISPOSABLE_TEST_BRANCH".into();
        profile.provenance =
            "fabricated profile solely to test qualification control flow, not broker evidence"
                .into();
        let dataset = data::import_csv(&archive, profile, SOURCE, false, 1 << 20, 10).unwrap();
        let mut catalog = crate::catalog::Catalog::open(&temp.path("catalog.sqlite")).unwrap();
        let entry = catalog.register(&dataset).unwrap().entry;
        let identity = entry.receipt.facts.dataset_identity.clone();
        let calendar = crate::calendar_evidence::Declaration {
            reference: "unapproved fixture calendar".into(),
            scope: Interval {
                start_ms: 0,
                end_exclusive_ms: 1001,
            },
            open: vec![Interval {
                start_ms: 0,
                end_exclusive_ms: 1001,
            }],
        };
        let claim = crate::machine_evidence::Claim {
            catalog_hash: entry.hash.clone(),
            archive_report_hash: crate::archive_evidence::assess(&archive, &dataset, &entry)
                .unwrap()
                .hash,
            calendar_report_hash: crate::calendar_evidence::assess(
                &dataset,
                &entry,
                calendar.clone(),
            )
            .unwrap()
            .hash,
        };
        let machine = catalog
            .admit_machine(&archive, &dataset, &identity, calendar, &claim)
            .unwrap()
            .id;
        let areas = vec![
            Area::Source,
            Area::Profile,
            Area::Calendar,
            Area::Quality,
            Area::Coverage,
            Area::ScientificQualification,
        ];
        let secret = temp.path("authority.secret");
        Authority::initialize(&secret, vec![reviewer(&areas)]).unwrap();
        let authority = Authority::load(&secret).unwrap();
        let principal = authority
            .authenticate("independent-reviewer", PASSWORD)
            .unwrap();
        for area in areas[..5].iter() {
            let head = catalog.review_state(&identity, &authority).unwrap().head;
            catalog
                .decide(
                    &authority,
                    &principal,
                    crate::data_approval::Request {
                        machine_id: machine.clone(),
                        expected_head: head,
                        area: *area,
                        decision: crate::data_approval::Decision::Approve,
                        explanation: "TEST CONTROL FLOW ONLY".into(),
                        evidence_text: "disposable fixture approval".into(),
                    },
                )
                .unwrap();
        }
        let eligibility = catalog
            .data_eligibility(&authority, &machine, &archive, &dataset)
            .unwrap();
        assert_eq!(eligibility.state, "DATA_APPROVED_FOR_SCOPE");
        let mut b = bundle();
        b.data_approval = DataApprovalEvidence::from(&eligibility);
        b.dataset_identity = identity;
        b.dataset_hash = dataset.manifest.content_hash.clone();
        b.profile_hash = dataset.manifest.profile_hash.clone();
        let registry = Registry::open(&temp.path("registry.sqlite")).unwrap();
        Self {
            temp,
            catalog,
            registry,
            authority,
            archive,
            dataset,
            bundle: b,
        }
    }
    fn register_accounting_and_report(&mut self, b: &EvidenceBundle) {
        let policy = Policy::owner_selected().unwrap();
        let key = ExperimentKey {
            concept_hash: b.concept_id.clone(),
            instrument: b.instrument.clone(),
            dataset_hash: b.dataset_hash.clone(),
            start_ms: policy.required_coverage_start_ms,
            end_exclusive_ms: policy.required_coverage_end_exclusive_ms,
            signal_version: crate::SIGNAL_VERSION.into(),
            indicator_version: crate::INDICATOR_VERSION.into(),
            cost_hash: b.cost_hash.clone(),
            fill_policy: "L1-full-fill-fixture".into(),
            profile_hash: b.profile_hash.clone(),
            engine_version: crate::ENGINE_VERSION.into(),
            protocol_hash: b.science.protocol_id.clone(),
            code_revision: b.execution.compute_revision.clone(),
            configuration_hash: hex64('8'),
            stage: Stage::TickVerification,
        };
        let id = key.identity().unwrap();
        let result =
            serde_json::json!({"fixture": "disposable full-period receipt", "trades": 200});
        self.registry
            .connection
            .execute(
                "INSERT INTO experiments(id,key_json,state,result_json,result_hash) VALUES(?1,?2,'completed',?3,?4)",
                params![
                    id,
                    serde_json::to_string(&key).unwrap(),
                    serde_json::to_string(&result).unwrap(),
                    data::hash(&result).unwrap()
                ],
            )
            .unwrap();
        let mut report = crate::science::Report {
            protocol_id: b.science.protocol_id.clone(),
            dataset_hash: b.dataset_hash.clone(),
            dataset_identity: b.dataset_identity.clone(),
            compute_revision: b.execution.compute_revision.clone(),
            status: "INSUFFICIENT_EVIDENCE".into(),
            data_class: b.declared_class,
            trial_budget: 1000,
            candidates: Vec::new(),
            missing_requirements: vec!["disposable fixture".into()],
        };
        let candidate = crate::science::CandidateEvidence {
            concept_id: b.concept_id.clone(),
            experiments: vec![crate::science::Evidence {
                phase: "protected-final".into(),
                experiment_id: id.clone(),
                outcome: "completed".into(),
                reused: false,
            }],
            pooled_oos_reset_fold_stats: crate::science::DescriptiveStats {
                closed: 200,
                wins: 120,
                losses: 60,
                breakeven: 20,
                maximum_losing_streak: 3,
                sum_net_micro_usd: 600_000_000,
            },
            unresolved_oos_positions: 0,
            failed_windows: 0,
            uncertainty: Some(crate::uncertainty::Diagnostic {
                method: crate::uncertainty::VERSION.into(),
                observations: 200,
                net_wins: 120,
                declared_trials: 1000,
                family_error_probability: 0.05,
                empirical_net_win_rate: Some(0.6),
                conditional_lower_net_win_rate: Some(0.5),
                assumptions_verified: false,
                warnings: Vec::new(),
            }),
            block_uncertainty: None,
            robustness: Vec::new(),
            robustness_assessment: Some(crate::robustness::ProbeAssessment {
                method: crate::robustness::ASSESSMENT_VERSION.into(),
                policy: crate::robustness::ProbePolicy {
                    minimum_resolved_per_probe: 20,
                    minimum_net_win_rate_bps: 5000,
                    minimum_sum_net_micro_usd: 1,
                    maximum_consecutive_losses: 5,
                },
                state: "PASS_DECLARED_PROBES".into(),
                cases: (0..4)
                    .map(|i| crate::robustness::ProbeDecision {
                        case_name: format!("fixture-case-{i}"),
                        state: "PASS_DECLARED_PROBES".into(),
                        reasons: Vec::new(),
                    })
                    .collect(),
                qualification_granted: false,
                limitations: "abstract fixture".into(),
            }),
            breakdown: None,
            win_evidence: None,
            evidence_gate: Some(b.science.gate.clone()),
        };
        report.candidates.push(candidate);
        let report_hash = data::hash(&report).unwrap();
        let definition = serde_json::json!({"fixture": "disposable frozen definition"});
        self.registry
            .connection
            .execute(
                "INSERT INTO scientific_protocols(id,definition_json,definition_hash,report_json,report_hash) VALUES(?1,?2,?3,?4,?5)",
                params![
                    b.science.protocol_id,
                    serde_json::to_string(&definition).unwrap(),
                    data::hash(&definition).unwrap(),
                    serde_json::to_string(&report).unwrap(),
                    report_hash
                ],
            )
            .unwrap();
        // Bind the exact fixture identities into the presented bundle.
        let mut updated = b.clone();
        updated.accounting.experiment_id = id;
        updated.accounting.result_hash = data::hash(&result).unwrap();
        updated.science.report_hash = report_hash;
        updated.science.block_uncertainty_present = false;
        self.bundle = updated;
    }
    fn live(&self) -> LiveEvidence<'_> {
        LiveEvidence {
            registry: &self.registry,
            catalog: &self.catalog,
            authority: &self.authority,
            archive: &self.archive,
            dataset: &self.dataset,
        }
    }
}
#[test]
fn full_positive_flow_qualifies_ranks_revokes_and_retains_history() {
    let mut f = LiveFixture::new("positive");
    f.register_accounting_and_report(&f.bundle.clone());
    let b = f.bundle.clone();
    let p = Policy::owner_selected().unwrap();
    let mut store = Store::open(&f.temp.path("store.sqlite")).unwrap();
    let principal = f
        .authority
        .authenticate("independent-reviewer", PASSWORD)
        .unwrap();
    let event = store
        .decide(
            &f.authority,
            &principal,
            request(DecisionKind::Qualify, None),
            &b,
            &p,
            Some(&f.live()),
        )
        .unwrap();
    assert_eq!(event.record.request.decision, DecisionKind::Qualify);
    assert_eq!(
        event.record.assessment.state,
        "QUALIFICATION_EVIDENCE_COMPLETE"
    );
    assert!(!event.record.assessment.qualification_granted);
    let board = store.board("EURUSD", &f.authority).unwrap();
    assert_eq!(board.entries.len(), 1);
    assert_eq!(board.entries[0].concept_id, b.concept_id);
    assert_eq!(board.entries[0].rank, 1);
    assert_eq!(board.entries[0].win_rate_bps, 6000);
    assert!(board.empty_reasons.is_empty());
    assert_eq!(board.policy_hash, Some(data::hash(&p).unwrap()));
    let snapshot = store.latest_board_snapshot("EURUSD").unwrap().unwrap();
    assert_eq!(snapshot.decision_hash, event.hash);
    assert_eq!(snapshot.entries.len(), 1);
    assert!(snapshot.previous.is_none());
    let state = store
        .scope_state("EURUSD", &b.concept_id, &f.authority)
        .unwrap();
    assert_eq!(state.current_state, "QUALIFIED");
    // Revoke removes the position but retains the complete history.
    let revoke = store
        .decide(
            &f.authority,
            &principal,
            request(DecisionKind::Revoke, Some(event.hash.clone())),
            &b,
            &p,
            None,
        )
        .unwrap();
    let board = store.board("EURUSD", &f.authority).unwrap();
    assert!(board.entries.is_empty());
    assert!(board.empty_reasons.iter().any(|r| r.contains("1 revoked")));
    let state = store
        .scope_state("EURUSD", &b.concept_id, &f.authority)
        .unwrap();
    assert_eq!(state.current_state, "REVOKED");
    assert_eq!(state.events.len(), 2);
    let snapshot2 = store.latest_board_snapshot("EURUSD").unwrap().unwrap();
    assert_eq!(snapshot2.previous, Some(snapshot_hash(&store, "EURUSD", 1)));
    assert_eq!(snapshot2.decision_hash, revoke.hash);
    // Requalification requires the new head and passes live checks again.
    let requalify = store
        .decide(
            &f.authority,
            &principal,
            request(DecisionKind::Qualify, Some(revoke.hash)),
            &b,
            &p,
            Some(&f.live()),
        )
        .unwrap();
    assert_eq!(
        store.board("EURUSD", &f.authority).unwrap().entries.len(),
        1
    );
    let (leaders, next) = store.leaders("EURUSD", None, 10).unwrap();
    assert_eq!(leaders.len(), 3);
    assert!(next.is_none());
    assert_eq!(leaders[2].decision_hash, requalify.hash);
    // Backup preserves the journal and refuses overwrite.
    let backup = f.temp.path("backup.sqlite");
    store.backup(&backup).unwrap();
    assert!(store.backup(&backup).is_err());
    let restored = Store::existing(&backup).unwrap();
    restored.integrity().unwrap();
    assert_eq!(
        restored
            .board("EURUSD", &f.authority)
            .unwrap()
            .entries
            .len(),
        1
    );
    assert_eq!(
        restored
            .scope_state("EURUSD", &b.concept_id, &f.authority)
            .unwrap()
            .events
            .len(),
        3
    );
    // Live evidence drift refuses a fresh qualification: revoke a data approval.
    let head = f
        .catalog
        .review_state(&b.data_approval.catalog_identity, &f.authority)
        .unwrap()
        .head;
    f.catalog
        .decide(
            &f.authority,
            &principal,
            crate::data_approval::Request {
                machine_id: b.data_approval.machine_id.clone(),
                expected_head: head,
                area: Area::Source,
                decision: crate::data_approval::Decision::Revoke,
                explanation: "fixture drift".into(),
                evidence_text: "fixture drift".into(),
            },
        )
        .unwrap();
    let mut drifted = b.clone();
    drifted.data_approval = DataApprovalEvidence::from(
        &f.catalog
            .data_eligibility(
                &f.authority,
                &b.data_approval.machine_id,
                &f.archive,
                &f.dataset,
            )
            .unwrap(),
    );
    assert!(
        store
            .decide(
                &f.authority,
                &principal,
                request(DecisionKind::Qualify, Some(requalify.hash)),
                &drifted,
                &p,
                Some(&f.live())
            )
            .is_err()
    );
}
fn snapshot_hash(store: &Store, instrument: &str, sequence: i64) -> String {
    store
        .connection
        .query_row(
            "SELECT id FROM qualification_boards WHERE instrument=?1 AND sequence=?2",
            params![instrument, sequence],
            |r| r.get(0),
        )
        .unwrap()
}
#[test]
fn live_binding_mismatches_refuse_qualification() {
    let mut f = LiveFixture::new("binding");
    f.register_accounting_and_report(&f.bundle.clone());
    let p = Policy::owner_selected().unwrap();
    let mut store = Store::open(&f.temp.path("store.sqlite")).unwrap();
    let principal = f
        .authority
        .authenticate("independent-reviewer", PASSWORD)
        .unwrap();
    // Forged accounting receipt hash.
    let mut b = f.bundle.clone();
    b.accounting.result_hash = hex64('0');
    assert!(
        store
            .decide(
                &f.authority,
                &principal,
                request(DecisionKind::Qualify, None),
                &b,
                &p,
                Some(&f.live())
            )
            .is_err()
    );
    // Missing experiment.
    let mut b = f.bundle.clone();
    b.accounting.experiment_id = hex64('0');
    assert!(
        store
            .decide(
                &f.authority,
                &principal,
                request(DecisionKind::Qualify, None),
                &b,
                &p,
                Some(&f.live())
            )
            .is_err()
    );
    // Forged report hash.
    let mut b = f.bundle.clone();
    b.science.report_hash = hex64('0');
    assert!(
        store
            .decide(
                &f.authority,
                &principal,
                request(DecisionKind::Qualify, None),
                &b,
                &p,
                Some(&f.live())
            )
            .is_err()
    );
    // Tampered stored result refuses on read.
    f.registry
        .connection
        .execute(
            "UPDATE experiments SET result_json='{\"fixture\":\"tampered\"}' WHERE id=?1",
            [&f.bundle.accounting.experiment_id],
        )
        .unwrap();
    let b = f.bundle.clone();
    assert!(
        store
            .decide(
                &f.authority,
                &principal,
                request(DecisionKind::Qualify, None),
                &b,
                &p,
                Some(&f.live())
            )
            .is_err()
    );
}
#[test]
fn boards_enforce_capacity_diversity_and_never_fill_with_unqualified() {
    let mut f = LiveFixture::new("boards");
    let p = Policy::owner_selected().unwrap();
    let mut store = Store::open(&f.temp.path("store.sqlite")).unwrap();
    let principal = f
        .authority
        .authenticate("independent-reviewer", PASSWORD)
        .unwrap();
    // Seven distinct concepts: five structural classes, two extra Donchian tweaks.
    let rules: Vec<Rule> = vec![
        Rule::Donchian {
            window: 20,
            above: true,
        },
        Rule::Donchian {
            window: 30,
            above: true,
        },
        Rule::Donchian {
            window: 40,
            above: true,
        },
        Rule::RsiCross {
            window: 14,
            basis_points: 5000,
            above: true,
        },
        Rule::ChannelEdge {
            window: 20,
            basis_points: 5000,
            upper: true,
        },
        Rule::MacdCross {
            fast: 12,
            slow: 26,
            signal_window: 9,
            above: true,
        },
        Rule::BollingerCross {
            window: 20,
            multiple_bps: 20000,
            upper: true,
        },
    ];
    let mut heads: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (i, rule) in rules.into_iter().enumerate() {
        let mut b = f.bundle.clone();
        b.strategy = Strategy {
            rule,
            ..b.strategy.clone()
        };
        b.concept_id = b.strategy.identity().unwrap();
        // Distinct frozen protocols and accounting receipts per concept keep bindings exact.
        b.science.protocol_id = format!("{:064x}", i + 1);
        f.register_accounting_and_report(&b);
        let b = f.bundle.clone();
        let head = heads.get(&b.concept_id).cloned();
        let event = store
            .decide(
                &f.authority,
                &principal,
                request(DecisionKind::Qualify, head),
                &b,
                &p,
                Some(&f.live()),
            )
            .unwrap_or_else(|e| panic!("concept {i}: {e}"));
        heads.insert(b.concept_id.clone(), event.hash);
    }
    let board = store.board("EURUSD", &f.authority).unwrap();
    assert_eq!(board.entries.len(), 5);
    // Diversity cap: at most two Donchian-class entries.
    let donchian = board
        .entries
        .iter()
        .filter(|e| e.diversity_class.contains("Donchian"))
        .count();
    assert_eq!(donchian, 2);
    assert_eq!(board.excluded.len(), 2);
    assert!(
        board
            .excluded
            .iter()
            .any(|e| e.reason.contains("diversity cap"))
    );
    assert!(
        board
            .excluded
            .iter()
            .any(|e| e.reason.contains("board full"))
    );
    // Ranks are contiguous and scores nonincreasing.
    for (i, e) in board.entries.iter().enumerate() {
        assert_eq!(e.rank, i + 1);
        if i > 0 {
            assert!(board.entries[i - 1].score.total >= e.score.total);
        }
    }
    // Other instruments remain honestly empty with exact reasons.
    let others = store.boards(&f.authority).unwrap();
    assert_eq!(others.len(), 7);
    for view in others.iter().filter(|v| v.instrument != "EURUSD") {
        assert!(view.entries.is_empty());
        assert!(
            view.empty_reasons
                .iter()
                .any(|r| r.contains("No independently qualified strategy"))
        );
    }
    // Rejected candidates never enter the board and stay in the archive.
    let mut rejected = f.bundle.clone();
    rejected.strategy = Strategy {
        rule: Rule::Rising { lookback: 5 },
        ..rejected.strategy.clone()
    };
    rejected.concept_id = rejected.strategy.identity().unwrap();
    let event = store
        .decide(
            &f.authority,
            &principal,
            request(DecisionKind::Reject, None),
            &rejected,
            &p,
            None,
        )
        .unwrap();
    let board = store.board("EURUSD", &f.authority).unwrap();
    assert!(
        board
            .entries
            .iter()
            .all(|e| e.concept_id != rejected.concept_id)
    );
    let page = store
        .history(&HistoryFilter {
            decision: Some(DecisionKind::Reject),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].hash, event.hash);
    // Filters: diversity class, timeframe, instrument, date range.
    let class = diversity_class(&strategy(20));
    let page = store
        .history(&HistoryFilter {
            diversity_class: Some(class.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.events.len(), 3);
    let page = store
        .history(&HistoryFilter {
            timeframe: Some("M1".into()),
            instrument: Some("EURUSD".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.events.len(), 8);
    let page = store
        .history(&HistoryFilter {
            instrument: Some("USDJPY".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(page.events.is_empty());
    let page = store
        .history(&HistoryFilter {
            from_ms: Some(chrono::Utc::now().timestamp_millis() + 1000),
            ..Default::default()
        })
        .unwrap();
    assert!(page.events.is_empty());
    // Authority rotation de-authenticates retained decisions without deleting them.
    let rotated = authority_fixture(
        &f.temp,
        "rotated.secret",
        vec![Area::ScientificQualification],
    );
    let board = store.board("EURUSD", &rotated).unwrap();
    assert!(board.entries.is_empty());
    assert_eq!(board.excluded.len(), 7);
    assert!(
        board
            .excluded
            .iter()
            .all(|e| e.reason.contains("not authenticated"))
    );
    let state = store
        .scope_state("EURUSD", &f.bundle.concept_id, &rotated)
        .unwrap();
    assert_eq!(state.current_state, "REVOKED");
    assert!(!state.current_authority_authenticated);
}
