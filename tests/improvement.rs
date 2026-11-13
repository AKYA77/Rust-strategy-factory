use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, Profile, Side},
    data::{self, Tick},
    improvement::{self, Options},
    research::{self, Evaluation, Input},
    storage::{Registry, Stage},
    strategies::{Rule, Strategy},
    tick_engine::Costs,
    validation::Interval,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-improvement-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn input(last: i64) -> Input {
    let ticks = [
        (0, 100000),
        (60000, 100020),
        (120000, 100040),
        (120000, 100050),
        (121000, last),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (ms, bid))| Tick {
        timestamp_ms: 1735689600000 + ms,
        sequence: i as u64,
        bid,
        ask: bid + 10,
    })
    .collect();
    Input {
        dataset: data::dataset(
            Profile::synthetic("EURUSD").unwrap(),
            ticks,
            "synthetic improvement fixture",
            "generated".into(),
            true,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); 5],
    }
}
fn strategy() -> Strategy {
    Strategy {
        stop_policy: None,
        timeframe: Timeframe::M1,
        side: Side::Short,
        entry: None,
        stop_ticks: 50,
        rule: Rule::AboveMean { window: 2 },
    }
}
fn evaluate(r: &mut Registry, s: &Strategy, last: i64) -> research::Execution {
    research::evaluate(
        r,
        &input(last),
        s,
        &Costs::default(),
        "diagnostic-parent-build",
        10000,
    )
    .unwrap()
}
#[test]
fn comparable_children_preserve_explicit_shadow_policy_identity() {
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let input = input(100900);
    let revision = research::compute_revision().unwrap();
    let policy = rust_strategy_factory::candidate_screen::ShadowPolicy {
        threshold_bps: 7000,
        seed: 19,
        ..Default::default()
    };
    let parent = research::evaluate_configured(
        &mut r,
        &input,
        &strategy(),
        &Costs::default(),
        &revision,
        research::EvaluationOptions {
            feature_cells: 10000,
            evaluation: None,
            shadow_policy: Some(&policy),
            proof_policy: None,
        },
    )
    .unwrap();
    let diagnosis = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 1,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    assert_eq!(diagnosis.proposals.len(), 1);
    let report = rust_strategy_factory::training_review::execute(
        &mut r,
        &input,
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap()
    .report;
    assert_eq!(report.baseline.experiment_id, parent.experiment_id);
    for evidence in
        std::iter::once(&report.baseline).chain(report.children.iter().map(|c| &c.evidence))
    {
        let result = r.result(&evidence.experiment_id).unwrap().unwrap();
        assert_eq!(
            result.result["shadow_policy"],
            serde_json::to_value(&policy).unwrap()
        );
        assert_eq!(
            r.experiment_key(&evidence.experiment_id)
                .unwrap()
                .unwrap()
                .configuration_hash,
            diagnosis.parent_key.configuration_hash
        );
    }
    assert!(
        rust_strategy_factory::training_review::execute(
            &mut r,
            &input,
            &diagnosis.id,
            9999,
            256 * 1024 * 1024
        )
        .is_err()
    );
    assert!(
        rust_strategy_factory::training_review::read_report(&r, &report.id)
            .unwrap()
            .is_some()
    );
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn exact_proof_rejection_audits_and_comparable_children_preserve_real_evidence() {
    use rust_strategy_factory::{proof_screen, training_review};
    let d = dir();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let input = input(99800);
    let revision = research::compute_revision().unwrap();
    let mut s = strategy();
    s.stop_ticks = 30;
    let mut skipped = None;
    for seed in 0..16 {
        let policy = proof_screen::Policy {
            audit_bps: 1,
            seed,
            ..Default::default()
        };
        let execution = research::evaluate_configured(
            &mut registry,
            &input,
            &s,
            &Costs::default(),
            &revision,
            research::EvaluationOptions {
                feature_cells: 10000,
                evaluation: None,
                shadow_policy: None,
                proof_policy: Some(&policy),
            },
        )
        .unwrap();
        if execution.result["proof_audited"] == false {
            skipped = Some(execution);
            break;
        }
    }
    let skipped = skipped.expect("fixture must exercise an actual unaudited proof rejection");
    assert_eq!(skipped.outcome, "rejected");
    assert!(skipped.result["tick_replay"].is_null());
    assert!(skipped.result["screen"]["signals"].as_u64().unwrap() > 0);
    let key = registry
        .experiment_key(&skipped.experiment_id)
        .unwrap()
        .unwrap();
    assert!(
        proof_screen::verified_rejection(&skipped.result, &key)
            .unwrap()
            .is_some()
    );
    let audited_policy = proof_screen::Policy {
        audit_bps: 10000,
        ..Default::default()
    };
    let audited = research::evaluate_configured(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        &revision,
        research::EvaluationOptions {
            feature_cells: 10000,
            evaluation: None,
            shadow_policy: None,
            proof_policy: Some(&audited_policy),
        },
    )
    .unwrap();
    assert_eq!(audited.outcome, "rejected");
    assert_eq!(audited.result["proof_audited"], true);
    assert_eq!(
        audited.result["tick_replay"]["trades"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(
        !audited.result["tick_replay"]["rejections"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let default = research::evaluate(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        &revision,
        10000,
    )
    .unwrap();
    assert_eq!(default.outcome, "completed");
    assert_eq!(audited.result["tick_replay"], default.result["tick_replay"]);
    let diagnosis = improvement::diagnose(
        &mut registry,
        &skipped.experiment_id,
        &Options {
            max_children: 1,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    assert!(diagnosis.training_evidence.is_none());
    assert!(
        diagnosis
            .reasons
            .iter()
            .any(|r| r.contains("by_proof_not_observed"))
    );
    assert_eq!(
        diagnosis.proposals[0].operator,
        "proven-friction-minimum-stop"
    );
    let review = training_review::execute(
        &mut registry,
        &input,
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap()
    .report;
    assert_eq!(review.baseline.resolved_net_micro_usd, Some(0));
    assert!(
        review
            .baseline
            .net_basis
            .as_ref()
            .unwrap()
            .contains("no-entry proof")
    );
    assert_eq!(review.children[0].net_delta_micro_usd, Some(2_364_060));
    assert!(
        training_review::read_report(&registry, &review.id)
            .unwrap()
            .is_some()
    );
    let mut tampered = skipped.result.clone();
    tampered["market_stop_proof"]["fixed_stop_price_units"] = 31.into();
    assert!(proof_screen::verified_rejection(&tampered, &key).is_err());
    let mut pending_key = key.clone();
    pending_key.code_revision = "proof-publication-race-fixture".into();
    let pending_id = registry.register(&pending_key).unwrap();
    let rust_strategy_factory::storage::Claim::Acquired(pending_lease) =
        registry.claim(&pending_id, 0, 300000).unwrap()
    else {
        panic!("new fixture lease")
    };
    // Isolated durable failure injection, not fabricated market evidence.
    let connection = rusqlite::Connection::open(d.join("state.sqlite")).unwrap();
    connection.execute("INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,0,'proof-audit-contradiction-v1',1)",[&skipped.experiment_id]).unwrap();
    drop(connection);
    assert!(
        registry
            .finish(
                &pending_lease,
                1,
                rust_strategy_factory::storage::Outcome::Rejected,
                &skipped.result
            )
            .unwrap_err()
            .contains("disabled by recorded audit contradiction")
    );
    registry
        .finish(
            &pending_lease,
            1,
            rust_strategy_factory::storage::Outcome::Failed,
            &serde_json::json!({"error":"injected audit block"}),
        )
        .unwrap();
    assert!(
        research::evaluate_configured(
            &mut registry,
            &input,
            &s,
            &Costs::default(),
            &revision,
            research::EvaluationOptions {
                feature_cells: 10000,
                evaluation: None,
                shadow_policy: None,
                proof_policy: Some(&audited_policy)
            }
        )
        .unwrap_err()
        .contains("disabled by recorded audit contradiction")
    );
    assert!(
        research::evaluate(
            &mut registry,
            &input,
            &s,
            &Costs::default(),
            &revision,
            10000
        )
        .unwrap()
        .reused
    );
    let mut driver = research::Driver::open(
        &d.join("state.sqlite"),
        vec![Input {
            dataset: input.dataset.clone(),
            conversions: input.conversions.clone(),
        }],
        research::Config {
            proof_policy: Some(audited_policy.clone()),
            ..Default::default()
        },
        revision.clone(),
    )
    .unwrap();
    driver.acquire(0).unwrap();
    assert!(driver.step(1).unwrap());
    assert_eq!(
        research::status(&driver.registry, &driver.id, 2)
            .unwrap()
            .state,
        "paused"
    );
    assert!(!driver.step(2).unwrap());
    driver.release().unwrap();
    drop(driver);
    let mut tampered = skipped.result.clone();
    tampered["proof_audited"] = true.into();
    assert!(proof_screen::verified_rejection(&tampered, &key).is_err());
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn proof_screening_cannot_hide_bad_conversions_or_adapt_scientific_windows() {
    let d = dir();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let mut input = input(99800);
    let mut s = strategy();
    s.stop_ticks = 30;
    let policy = rust_strategy_factory::proof_screen::Policy {
        audit_bps: 1,
        ..Default::default()
    };
    input.conversions[0].as_of_ms = input.dataset.ticks[0].timestamp_ms + 1;
    let execution = research::evaluate_configured(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "proof-invalid-input",
        research::EvaluationOptions {
            feature_cells: 10000,
            evaluation: None,
            shadow_policy: None,
            proof_policy: Some(&policy),
        },
    )
    .unwrap();
    assert_eq!(execution.outcome, "failed");
    assert!(
        execution.result["error"]
            .as_str()
            .unwrap()
            .contains("conversion")
    );
    let training_window = Interval {
        start_ms: input.dataset.manifest.first_ms + 120000,
        end_exclusive_ms: input.dataset.manifest.last_ms + 1,
    };
    let training = Evaluation {
        protocol_hash: data::hash(&("training-window-v1", &training_window)).unwrap(),
        window: training_window,
        stage: Stage::TickVerification,
    };
    let scoped = research::evaluate_configured(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "proof-scope",
        research::EvaluationOptions {
            feature_cells: 10000,
            evaluation: Some(&training),
            shadow_policy: None,
            proof_policy: Some(&policy),
        },
    )
    .unwrap();
    assert_eq!(scoped.outcome, "rejected");
    assert_eq!(scoped.result["proof_rejection"], true);
    let evaluation = Evaluation {
        window: Interval {
            start_ms: input.dataset.manifest.first_ms,
            end_exclusive_ms: input.dataset.manifest.last_ms + 1,
        },
        stage: Stage::IndependentValidation,
        protocol_hash: "protected".into(),
    };
    assert!(
        research::evaluate_configured(
            &mut registry,
            &input,
            &s,
            &Costs::default(),
            "proof-science-refusal",
            research::EvaluationOptions {
                feature_cells: 10000,
                evaluation: Some(&evaluation),
                shadow_policy: None,
                proof_policy: Some(&policy)
            }
        )
        .is_err()
    );
    input.conversions[0] = Conversion::usd();
    let mut profile = input.dataset.profile.clone();
    profile.pip = 100000;
    input.dataset = data::dataset(
        profile,
        input.dataset.ticks.clone(),
        "SYNTHETIC_TEST_ONLY invalid entry cost",
        "generated".into(),
        true,
    )
    .unwrap();
    let invalid_entry = research::evaluate_configured(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "proof-entry-invalid",
        research::EvaluationOptions {
            feature_cells: 10000,
            evaluation: None,
            shadow_policy: None,
            proof_policy: Some(&policy),
        },
    )
    .unwrap();
    let reference = research::evaluate(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "proof-entry-reference",
        10000,
    )
    .unwrap();
    assert_eq!(invalid_entry.outcome, "failed");
    assert_eq!(reference.outcome, "failed");
    assert_eq!(invalid_entry.result["error"], reference.result["error"]);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn real_loss_proposals_are_bounded_immutable_and_survive_backup() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut r = Registry::open(&path).unwrap();
    let parent = evaluate(&mut r, &strategy(), 100300);
    assert_eq!(
        parent.result["tick_replay"]["trades"][0]["net_micro_usd"],
        -5_958_200i64
    );
    let before = serde_json::to_value(r.counts().unwrap()).unwrap();
    let options = Options {
        max_children: 3,
        ..Default::default()
    };
    let first = improvement::diagnose(&mut r, &parent.experiment_id, &options).unwrap();
    assert!(!first.reused);
    assert_eq!(first.report.proposals.len(), 3);
    assert!(first.report.reasons.contains(&"recorded_net_loss".into()));
    let evidence = first.report.training_evidence.as_ref().unwrap();
    assert_eq!(evidence.closed.resolved, 1);
    assert_eq!(evidence.closed.losses, 1);
    assert_eq!(evidence.closed.sum_net_micro_usd, -5_958_200);
    assert_eq!(evidence.maximum_consecutive_losses, 1);
    assert_eq!(evidence.closed_commission_micro_usd, 0);
    assert_eq!(evidence.exit_years[&2025].resolved, 1);
    assert_eq!(evidence.entry_utc_six_hour_bins[&0].resolved, 1);
    assert_eq!(evidence.recorded_realized_cash_micro_usd, 14_041_800);
    assert_eq!(evidence.status, "INSUFFICIENT_EVIDENCE");

    for p in &first.report.proposals {
        assert_eq!(p.status, "UNEVALUATED");
        assert_eq!(p.strategy.side, Side::Short);
        assert_eq!(p.strategy.timeframe, Timeframe::M1);
        assert_eq!(p.strategy.entry, strategy().entry);
        assert_eq!(p.concept_id, p.strategy.identity().unwrap());
    }
    let second = improvement::diagnose(&mut r, &parent.experiment_id, &options).unwrap();
    assert!(second.reused);
    assert_eq!(
        data::hash(&first.report).unwrap(),
        data::hash(&second.report).unwrap()
    );
    assert_eq!(before, serde_json::to_value(r.counts().unwrap()).unwrap());
    r.integrity().unwrap();
    r.backup(&d.join("backup.sqlite")).unwrap();
    drop(r);
    let backup = Registry::open(&d.join("backup.sqlite")).unwrap();
    assert_eq!(
        data::hash(
            &improvement::read_report(&backup, &first.report.id)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        data::hash(&first.report).unwrap()
    );
    drop(backup);
    let mut r = Registry::open(&path).unwrap();
    assert!(
        improvement::diagnose(&mut r, &parent.experiment_id, &options)
            .unwrap()
            .reused
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE training_diagnoses SET report_json=json_set(report_json,'$.status','FORGED')",
        [],
    )
    .unwrap();
    assert!(r.integrity().is_err());
    drop(r);
    drop(conn);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn scientific_stages_cannot_authorize_adaptation_but_explicit_training_can() {
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let data = input(100300);
    let window = Interval {
        start_ms: 1735689600000,
        end_exclusive_ms: 1735689721001,
    };
    for stage in [Stage::TickVerification, Stage::IndependentValidation] {
        let mut r = Registry::open(&d.join(format!("{stage:?}.sqlite"))).unwrap();
        if stage == Stage::IndependentValidation {
            r.reserve_instrument_holdout(
                &data.dataset.manifest.content_hash,
                "EURUSD",
                &window,
                "frozen-scientific-protocol",
                &[strategy().identity().unwrap()],
            )
            .unwrap();
        }
        let parent = research::evaluate_window(
            &mut r,
            &data,
            &strategy(),
            &Costs::default(),
            "test",
            10000,
            &Evaluation {
                window: window.clone(),
                stage,
                protocol_hash: "frozen-scientific-protocol".into(),
            },
        )
        .unwrap();
        assert!(
            improvement::diagnose(&mut r, &parent.experiment_id, &Options::default())
                .unwrap_err()
                .contains("explicit training")
        );
    }
    let parent = research::evaluate_window(
        &mut r,
        &data,
        &strategy(),
        &Costs::default(),
        "test",
        10000,
        &Evaluation {
            window: window.clone(),
            stage: Stage::TickVerification,
            protocol_hash: data::hash(&("training-window-v1", &window)).unwrap(),
        },
    )
    .unwrap();
    assert!(
        !improvement::diagnose(&mut r, &parent.experiment_id, &Options::default())
            .unwrap()
            .report
            .proposals
            .is_empty()
    );
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn no_signals_unresolved_and_failed_receipts_remain_explicit() {
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let mut s = strategy();
    s.rule = Rule::AboveMean { window: 200 };
    let parent = evaluate(&mut r, &s, 100300);
    let report = improvement::diagnose(&mut r, &parent.experiment_id, &Options::default())
        .unwrap()
        .report;
    assert_eq!(report.reasons, vec!["no_signals"]);
    assert!(!report.proposals.is_empty());
    let parent = evaluate(&mut r, &strategy(), 100050);
    let report = improvement::diagnose(&mut r, &parent.experiment_id, &Options::default())
        .unwrap()
        .report;
    assert!(report.proposals.is_empty());
    assert!(
        report
            .reasons
            .contains(&"unresolved_outcome_no_proposals".into())
    );
    let parent = research::evaluate(
        &mut r,
        &input(100300),
        &strategy(),
        &Costs::default(),
        "failed-budget",
        0,
    )
    .unwrap();
    assert_eq!(parent.outcome, "failed");
    let report = improvement::diagnose(&mut r, &parent.experiment_id, &Options::default())
        .unwrap()
        .report;
    assert_eq!(report.reasons, vec!["execution_failed_no_proposals"]);
    assert!(report.proposals.is_empty());
    assert!(
        improvement::diagnose(
            &mut r,
            &parent.experiment_id,
            &Options {
                max_children: 17,
                ..Default::default()
            }
        )
        .is_err()
    );
    r.integrity().unwrap();
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn competing_connections_publish_one_deterministic_report() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut r = Registry::open(&path).unwrap();
    let parent = evaluate(&mut r, &strategy(), 100300);
    drop(r);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let id = parent.experiment_id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut r = Registry::open(&path).unwrap();
                barrier.wait();
                improvement::diagnose(&mut r, &id, &Options::default()).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| !r.reused).count(), 1);
    assert_eq!(results[0].report.id, results[1].report.id);
    let r = Registry::open(&path).unwrap();
    r.integrity().unwrap();
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn comparable_training_review_replays_and_retains_exact_evaluation_edges() {
    use rust_strategy_factory::training_review;
    let d = dir();
    let path = d.join("state.sqlite");
    let mut r = Registry::open(&path).unwrap();
    let parent = evaluate(&mut r, &strategy(), 100300);
    let diagnosis = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 3,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    // Simulate a restart after the baseline and one child commit, before review publication.
    let revision = research::compute_revision().unwrap();
    research::evaluate(
        &mut r,
        &input(100300),
        &strategy(),
        &Costs::default(),
        &revision,
        10000,
    )
    .unwrap();
    research::evaluate(
        &mut r,
        &input(100300),
        &diagnosis.proposals[0].strategy,
        &Costs::default(),
        &revision,
        10000,
    )
    .unwrap();
    let before = r.counts().unwrap().attempts;
    drop(r);
    let mut r = Registry::open(&path).unwrap();
    let review = training_review::execute(
        &mut r,
        &input(100300),
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap();
    assert!(!review.reused);
    assert_eq!(review.report.children.len(), 3);
    assert_eq!(
        review.report.baseline.resolved_net_micro_usd,
        Some(-5_958_200)
    );
    assert_eq!(review.report.status, "DESCRIPTIVE_TRAINING_ONLY");
    assert_eq!(r.counts().unwrap().attempts, before + 2);
    for (child, proposal) in review.report.children.iter().zip(&diagnosis.proposals) {
        assert_eq!(child.proposal_edge_id, proposal.edge_id);
        let key = r
            .experiment_key(&child.evidence.experiment_id)
            .unwrap()
            .unwrap();
        assert_eq!(key.code_revision, revision);
        assert_eq!(key.concept_hash, proposal.concept_id);
        assert_eq!(key.cost_hash, diagnosis.parent_key.cost_hash);
        assert_eq!(
            key.configuration_hash,
            diagnosis.parent_key.configuration_hash
        );
        assert_eq!(key.start_ms, diagnosis.parent_key.start_ms);
        assert_eq!(key.end_exclusive_ms, diagnosis.parent_key.end_exclusive_ms);
        if let Some(net) = child.evidence.resolved_net_micro_usd {
            assert_eq!(child.net_delta_micro_usd, Some(net + 5_958_200));
        }
        assert_eq!(
            child.evidence.result_hash,
            r.result(&child.evidence.experiment_id)
                .unwrap()
                .unwrap()
                .result_hash
        );
    }
    let count = r.counts().unwrap().attempts;
    assert!(
        training_review::execute(
            &mut r,
            &input(100300),
            &diagnosis.id,
            10000,
            256 * 1024 * 1024
        )
        .unwrap()
        .reused
    );
    assert_eq!(r.counts().unwrap().attempts, count);
    r.integrity().unwrap();
    r.backup(&d.join("backup.sqlite")).unwrap();
    let backup = Registry::open(&d.join("backup.sqlite")).unwrap();
    assert!(
        training_review::read_report(&backup, &review.report.id)
            .unwrap()
            .is_some()
    );
    drop(backup);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE training_reviews SET report_json=json_set(report_json,'$.status','FORGED')",
        [],
    )
    .unwrap();
    assert!(r.integrity().is_err());
    drop(conn);
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn review_refuses_changed_context_before_registering_and_keeps_scoped_window() {
    use rust_strategy_factory::training_review;
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let data = input(100300);
    let window = Interval {
        start_ms: 1735689660000,
        end_exclusive_ms: 1735689721001,
    };
    let parent = research::evaluate_window(
        &mut r,
        &data,
        &strategy(),
        &Costs::default(),
        "old-build",
        10000,
        &Evaluation {
            window: window.clone(),
            stage: Stage::TickVerification,
            protocol_hash: data::hash(&("training-window-v1", &window)).unwrap(),
        },
    )
    .unwrap();
    let diagnosis = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 2,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    let before = r.counts().unwrap().attempts;
    assert!(
        training_review::execute(&mut r, &data, &diagnosis.id, 9999, 256 * 1024 * 1024).is_err()
    );
    assert!(
        training_review::execute(
            &mut r,
            &input(100400),
            &diagnosis.id,
            10000,
            256 * 1024 * 1024
        )
        .is_err()
    );
    let mut changed = input(100300);
    changed.dataset.manifest.source = "different provenance".into();
    assert!(
        training_review::execute(&mut r, &changed, &diagnosis.id, 10000, 256 * 1024 * 1024)
            .is_err()
    );
    let mut changed = input(100300);
    changed.conversions[0].as_of_ms = 1;
    assert!(
        training_review::execute(&mut r, &changed, &diagnosis.id, 10000, 256 * 1024 * 1024)
            .is_err()
    );
    assert_eq!(r.counts().unwrap().attempts, before);
    let review =
        training_review::execute(&mut r, &data, &diagnosis.id, 10000, 256 * 1024 * 1024).unwrap();
    for evidence in std::iter::once(&review.report.baseline)
        .chain(review.report.children.iter().map(|c| &c.evidence))
    {
        let key = r.experiment_key(&evidence.experiment_id).unwrap().unwrap();
        assert_eq!(key.start_ms, window.start_ms);
        assert_eq!(key.end_exclusive_ms, window.end_exclusive_ms);
    }
    r.integrity().unwrap();
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}

fn queued_driver(path: &std::path::Path, children: usize, budget: u64) -> research::Driver {
    research::Driver::open(
        path,
        vec![input(100300)],
        research::Config {
            seed: 0,
            feature_cells: 10000,
            improvement_children: children,
            state_budget_bytes: budget,
            ..Default::default()
        },
        research::compute_revision().unwrap(),
    )
    .unwrap()
}
#[test]
fn opt_in_queue_is_bounded_pauses_and_recovers_a_committed_review() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut driver = queued_driver(&path, 2, 256 * 1024 * 1024);
    driver.acquire(0).unwrap();
    assert!(driver.step(1).unwrap());
    let first = research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!(first.cursor, 1);
    assert_eq!(first.improvements.queued, 1);
    assert_eq!(driver.registry.counts().unwrap().attempts, 1);
    research::control(&driver.registry, &driver.id, "paused").unwrap();
    assert!(!driver.step(3).unwrap());
    assert_eq!(
        research::status(&driver.registry, &driver.id, 3)
            .unwrap()
            .improvements
            .queued,
        1
    );
    // Simulate evidence commits followed by a crash before the queue checkpoint.
    let diagnosis = improvement::diagnose(
        &mut driver.registry,
        first.latest.as_ref().unwrap(),
        &Options {
            seed: 0,
            max_children: 2,
            state_budget_bytes: 256 * 1024 * 1024,
        },
    )
    .unwrap()
    .report;
    let review = rust_strategy_factory::training_review::execute(
        &mut driver.registry,
        &input(100300),
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(review.report.children.len(), 2);
    let attempts = driver.registry.counts().unwrap().attempts;
    let run_id = driver.id.clone();
    drop(driver);
    let mut driver = queued_driver(&path, 2, 256 * 1024 * 1024);
    assert_eq!(driver.id, run_id);
    driver.acquire(4).unwrap();
    research::control(&driver.registry, &driver.id, "running").unwrap();
    assert!(driver.step(5).unwrap());
    let status = research::status(&driver.registry, &driver.id, 6).unwrap();
    assert_eq!(status.cursor, 1);
    assert_eq!(status.improvements.queued, 0);
    assert_eq!(status.improvements.completed, 1);
    assert_eq!(status.improvements.failed, 0);
    assert_eq!(status.improvements.child_receipt_links, 2);
    assert_eq!(status.executed, first.executed);
    assert_eq!(status.rejected, first.rejected);
    assert_eq!(driver.registry.counts().unwrap().attempts, attempts);
    assert!(driver.step(7).unwrap());
    assert_eq!(
        research::status(&driver.registry, &driver.id, 8)
            .unwrap()
            .improvements
            .queued,
        1
    );
    driver.registry.integrity().unwrap();
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn queued_work_obeys_fencing_and_state_pressure_without_losing_the_job() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut old = queued_driver(&path, 2, 32 * 1024 * 1024);
    old.acquire(0).unwrap();
    assert!(old.step(1).unwrap());
    let mut current = queued_driver(&path, 2, 32 * 1024 * 1024);
    current.acquire(300_002).unwrap();
    assert!(old.step(2).is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE pressure_fixture(bytes BLOB); INSERT INTO pressure_fixture VALUES(zeroblob(20000000));").unwrap();
    assert!(!current.step(300_003).unwrap());
    let status = research::status(&current.registry, &current.id, 300_004).unwrap();
    assert_eq!(status.state, "paused");
    assert_eq!(status.cursor, 1);
    assert_eq!(status.improvements.queued, 1);
    assert_eq!(status.improvements.failed, 0);
    assert_eq!(current.registry.counts().unwrap().attempts, 1);
    drop(conn);
    drop(old);
    drop(current);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn automatic_improvement_limits_and_build_identity_are_enforced() {
    let d = dir();
    assert!(
        research::Driver::open(
            &d.join("invalid.sqlite"),
            vec![input(100300)],
            research::Config {
                improvement_children: 5,
                ..Default::default()
            },
            research::compute_revision().unwrap()
        )
        .is_err()
    );
    assert!(
        research::Driver::open(
            &d.join("invalid.sqlite"),
            vec![input(100300)],
            research::Config {
                improvement_children: 1,
                ..Default::default()
            },
            "different-build".into()
        )
        .is_err()
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn incremental_review_yields_after_each_missing_experiment_and_matches_full_report() {
    use rust_strategy_factory::training_review;
    let d = dir();
    let path = d.join("state.sqlite");
    let mut r = Registry::open(&path).unwrap();
    let parent = evaluate(&mut r, &strategy(), 100300);
    let diagnosis = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 3,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    let mut before = r.counts().unwrap().attempts;
    for _ in 0..4 {
        assert!(
            training_review::advance(
                &mut r,
                &input(100300),
                &diagnosis.id,
                10000,
                256 * 1024 * 1024
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(r.counts().unwrap().attempts, before + 1);
        before += 1;
        r.integrity().unwrap();
        drop(r);
        r = Registry::open(&path).unwrap();
    }
    let incremental = training_review::advance(
        &mut r,
        &input(100300),
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap()
    .unwrap();
    assert_eq!(r.counts().unwrap().attempts, before);
    assert!(!incremental.reused);
    let complete = training_review::execute(
        &mut r,
        &input(100300),
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap();
    assert!(complete.reused);
    assert_eq!(
        data::hash(&incremental.report).unwrap(),
        data::hash(&complete.report).unwrap()
    );
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn worker_pause_and_restart_between_children_preserve_progress_and_counters() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut driver = queued_driver(&path, 2, 256 * 1024 * 1024);
    driver.acquire(0).unwrap();
    assert!(driver.step(1).unwrap());
    assert_eq!(driver.registry.counts().unwrap().attempts, 1);
    assert!(driver.step(2).unwrap());
    assert_eq!(driver.registry.counts().unwrap().attempts, 2);
    let status = research::status(&driver.registry, &driver.id, 3).unwrap();
    assert_eq!(status.cursor, 1);
    assert_eq!(status.improvements.queued, 1);
    assert_eq!(status.improvements.completed, 0);
    let jobs = research::improvement_jobs(&driver.registry, &driver.id).unwrap();
    assert!(jobs[0].diagnosis_id.is_some());
    assert!(jobs[0].review_id.is_none());
    research::control(&driver.registry, &driver.id, "paused").unwrap();
    assert!(!driver.step(4).unwrap());
    assert_eq!(driver.registry.counts().unwrap().attempts, 2);
    drop(driver);
    let mut driver = queued_driver(&path, 2, 256 * 1024 * 1024);
    driver.acquire(5).unwrap();
    research::control(&driver.registry, &driver.id, "running").unwrap();
    assert!(driver.step(6).unwrap());
    assert_eq!(driver.registry.counts().unwrap().attempts, 3);
    assert_eq!(
        research::status(&driver.registry, &driver.id, 7)
            .unwrap()
            .improvements
            .queued,
        1
    );
    assert!(driver.step(8).unwrap());
    assert_eq!(driver.registry.counts().unwrap().attempts, 3);
    let status = research::status(&driver.registry, &driver.id, 9).unwrap();
    assert_eq!(status.cursor, 1);
    assert_eq!(status.improvements.completed, 1);
    assert_eq!(status.improvements.child_receipt_links, 2);
    assert_eq!(status.rejected, 1);
    driver.registry.integrity().unwrap();
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn actual_v1_diagnosis_fixture_remains_hash_readable_after_optional_extension() {
    use rust_strategy_factory::storage::{Claim, ExperimentKey, Outcome};
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../docs/VERIFICATION/training-diagnosis-v1-fixture.json"
    ))
    .unwrap();
    let d = dir();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let key: ExperimentKey = serde_json::from_value(fixture["parent_key"].clone()).unwrap();
    let id = registry.register(&key).unwrap();
    let Claim::Acquired(lease) = registry.claim(&id, 0, 10000).unwrap() else {
        panic!("new fixture lease");
    };
    registry
        .finish(&lease, 1, Outcome::Rejected, &fixture["parent_result"])
        .unwrap();
    let raw = fixture["diagnosis_json"].as_str().unwrap();
    let report: improvement::Report = serde_json::from_str(raw).unwrap();
    assert!(report.training_evidence.is_none());
    let conn = rusqlite::Connection::open(d.join("state.sqlite")).unwrap();
    conn.execute(
        "INSERT INTO training_diagnoses(id,parent_id,report_json,report_hash) VALUES(?1,?2,?3,?4)",
        rusqlite::params![
            report.id,
            id,
            raw,
            fixture["diagnosis_hash"].as_str().unwrap()
        ],
    )
    .unwrap();
    assert_eq!(
        improvement::read_report(&registry, &report.id)
            .unwrap()
            .unwrap()
            .version,
        "training-proposals-v1"
    );
    registry.integrity().unwrap();
    drop(conn);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn rejected_parent_gets_declared_floor_probe_with_exact_child_tick_accounting() {
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let mut s = strategy();
    s.stop_ticks = 30;
    let parent = evaluate(&mut r, &s, 99800);
    assert_eq!(
        parent.result["market_stop_proof"]["version"],
        "fixed-market-stop-infeasible-v1"
    );
    assert_eq!(
        parent.result["market_stop_proof"]["required_stop_price_units"],
        41
    );
    assert_eq!(
        parent.result["tick_replay"]["trades"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(parent.result["tick_replay"]["rejections"].as_array().unwrap().iter().any(|x| x["reason"] == "initial protective stop is crossed or violates executable-side minimum distance"));
    let diagnosis = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 1,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    assert_eq!(diagnosis.proposals.len(), 1);
    let proposal = &diagnosis.proposals[0];
    assert_eq!(proposal.operator, "declared-friction-minimum-stop");
    assert_eq!(proposal.strategy.stop_ticks, 41); // 1 minimum + 20 spread + 20 entry slip.
    assert_eq!(proposal.status, "UNEVALUATED");
    let child = evaluate(&mut r, &proposal.strategy, 99800);
    let trade = &child.result["tick_replay"]["trades"][0];
    // Short effective entry=100050-20; target=entry-3*41; margin+mark caps quantity.
    assert_eq!(trade["entry"], 100030);
    assert_eq!(trade["stop"], 100071);
    assert_eq!(trade["target"], 99907);
    assert_eq!(trade["exit"], 99907);
    assert_eq!(trade["volume_micro_lots"], 19220);
    assert_eq!(trade["net_micro_usd"], 2_364_060);
    assert_eq!(child.result["cost_profile"], parent.result["cost_profile"]);
    assert_eq!(
        child.result["instrument_profile"],
        parent.result["instrument_profile"]
    );
    let review = rust_strategy_factory::training_review::execute(
        &mut r,
        &input(99800),
        &diagnosis.id,
        10000,
        256 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(review.report.baseline.resolved_net_micro_usd, Some(0));
    assert_eq!(
        review.report.children[0].evidence.resolved_net_micro_usd,
        Some(2_364_060)
    );
    assert_eq!(
        review.report.children[0].net_delta_micro_usd,
        Some(2_364_060)
    );
    assert_eq!(review.report.status, "DESCRIPTIVE_TRAINING_ONLY");
    r.integrity().unwrap();
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn declared_stop_probe_preserves_policies_and_refuses_noop_and_grammar_overflow() {
    use rust_strategy_factory::{stops::Policy, strategies::EntryPlan};
    let mut p = Profile::synthetic("EURUSD").unwrap();
    let mut s = strategy();
    s.stop_ticks = 1;
    s.stop_policy = Some(Policy::Atr {
        window: 2,
        multiple_bps: 15000,
    });
    s.entry = Some(EntryPlan::Limit {
        offset_ticks: 2,
        ttl_ms: 60000,
        cancel_on_signal_false: true,
    });
    let costs = Costs::default();
    let child = improvement::friction_stop_probe(&s, &costs, &p)
        .unwrap()
        .unwrap();
    assert_eq!(child.stop_ticks, 21); // Passive entry has no adverse entry slippage.
    let mut expected = s.clone();
    expected.stop_ticks = 21;
    assert_eq!(child.identity().unwrap(), expected.identity().unwrap());
    assert!(
        improvement::friction_stop_probe(&child, &costs, &p)
            .unwrap()
            .is_none()
    );
    p.min_stop_ticks = 1_000_000;
    assert!(
        improvement::friction_stop_probe(&s, &costs, &p)
            .unwrap()
            .is_none()
    );
    p.min_stop_ticks = i64::MAX;
    assert!(
        improvement::friction_stop_probe(&s, &costs, &p)
            .unwrap()
            .is_none()
    );
}

#[test]
fn declared_floor_is_not_a_wide_spread_guarantee() {
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let mut source = input(99800);
    let ticks = source
        .dataset
        .ticks
        .iter()
        .map(|t| Tick {
            ask: t.bid + 100,
            ..*t
        })
        .collect();
    source.dataset = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "synthetic wide-spread fixture",
        "generated".into(),
        true,
    )
    .unwrap();
    let mut s = strategy();
    s.stop_ticks = 30;
    for side in [Side::Long, Side::Short] {
        s.side = side;
        let child =
            improvement::friction_stop_probe(&s, &Costs::default(), &source.dataset.profile)
                .unwrap()
                .unwrap();
        let result = research::evaluate(
            &mut r,
            &source,
            &child,
            &Costs::default(),
            "wide-spread",
            10000,
        )
        .unwrap();
        assert!(
            result.result["tick_replay"]["trades"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(result.result["tick_replay"]["rejections"].as_array().unwrap().iter().any(|x| x["reason"] == "initial protective stop is crossed or violates executable-side minimum distance"));
    }
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn structural_dropout_executes_a_real_child_and_small_budgets_keep_mutation_classes() {
    let d = dir();
    let mut r = Registry::open(&d.join("state.sqlite")).unwrap();
    let mut s = strategy();
    s.rule = Rule::All(vec![s.rule, Rule::Falling { lookback: 1 }]);
    let parent = evaluate(&mut r, &s, 100300);
    assert_eq!(parent.result["screen"]["signals"], 0);
    let report = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 16,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    let child = report
        .proposals
        .iter()
        .find(|p| p.strategy.identity().unwrap() == strategy().identity().unwrap())
        .unwrap();
    assert!(child.operator.starts_with("structural-drop-term-"));
    assert_eq!(child.status, "UNEVALUATED");
    let result = evaluate(&mut r, &child.strategy, 100300);
    assert_eq!(
        result.result["tick_replay"]["trades"][0]["net_micro_usd"],
        -5_958_200i64
    );
    // A nonzero trade can lose; structure proposals are not accepted improvements.
    assert_eq!(report.status, "TRAINING_ONLY_UNEVALUATED");
    let small = improvement::diagnose(
        &mut r,
        &parent.experiment_id,
        &Options {
            max_children: 2,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    assert_eq!(small.proposals.len(), 2);
    assert!(!small.proposals[0].operator.starts_with("structural-"));
    assert!(small.proposals[1].operator.starts_with("structural-"));
    r.integrity().unwrap();
    drop(r);
    std::fs::remove_dir_all(d).unwrap();
}
