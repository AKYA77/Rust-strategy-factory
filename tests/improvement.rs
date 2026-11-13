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
