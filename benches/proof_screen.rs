//! Full current evaluation comparison on a deliberately proven no-entry cohort.
use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    proof_screen,
    research::{self, EvaluationOptions, Input},
    storage::Registry,
    strategies::{Rule, Strategy},
    tick_engine::Costs,
};
use serde_json::json;
use std::{fs, time::Instant};
fn main() {
    let directory =
        std::env::temp_dir().join(format!("factory-proof-benchmark-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let inputs = SYMBOLS
        .iter()
        .map(|symbol| {
            let p = Profile::synthetic(symbol).unwrap();
            let base = if *symbol == "USDJPY" {
                150 * p.price_scale
            } else if *symbol == "XAUUSD" {
                2500 * p.price_scale
            } else {
                p.price_scale
            };
            let ticks = (0..12000)
                .map(|i| {
                    let n = i % 120;
                    let wave = if n < 60 { n } else { 120 - n };
                    let bid = base + wave * p.pip * 5;
                    Tick {
                        timestamp_ms: 1735689600000 + i * 10000,
                        sequence: i as u64,
                        bid,
                        ask: bid + p.pip,
                    }
                })
                .collect();
            let dataset = data::dataset(
                p,
                ticks,
                "SYNTHETIC_TEST_ONLY proven-no-entry benchmark",
                "generated".into(),
                true,
            )
            .unwrap();
            let conversions = dataset
                .ticks
                .iter()
                .map(|t| {
                    if dataset.profile.quote == "USD" {
                        Conversion::usd()
                    } else {
                        Conversion {
                            usd_num: 1,
                            quote_den: if *symbol == "USDJPY" { 150 } else { 1 },
                            as_of_ms: t.timestamp_ms,
                        }
                    }
                })
                .collect();
            Input {
                dataset,
                conversions,
            }
        })
        .collect::<Vec<_>>();
    let policy = proof_screen::Policy::default();
    let costs = Costs::default();
    let revision = research::compute_revision().unwrap();
    let mut measurements = Vec::new();
    let mut reference = std::collections::BTreeMap::new();
    for repetition in 0..5 {
        let order = if repetition % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for optimized in order {
            let path = directory.join(format!("{repetition}-{optimized}.sqlite"));
            let mut registry = Registry::open(&path).unwrap();
            let mut identities = Vec::new();
            let mut audit_count = 0;
            let mut skipped_count = 0;
            let start = Instant::now();
            for input in &inputs {
                for side in [Side::Long, Side::Short] {
                    let p = &input.dataset.profile;
                    let strategy = Strategy {
                        timeframe: Timeframe::M1,
                        side,
                        stop_ticks: p.min_stop_ticks + 4 * p.pip / p.tick - 1,
                        rule: Rule::AboveMean { window: 5 },
                        entry: None,
                        stop_policy: None,
                    };
                    let e = research::evaluate_configured(
                        &mut registry,
                        input,
                        &strategy,
                        &costs,
                        &revision,
                        EvaluationOptions {
                            feature_cells: 1_000_000,
                            evaluation: None,
                            shadow_policy: None,
                            proof_policy: optimized.then_some(&policy),
                            indexed_policy: None,
                        },
                    )
                    .unwrap();
                    assert!(!e.reused);
                    assert!(e.result["screen"]["signals"].as_u64().unwrap() > 0);
                    let proof = screen_proof(&e.result);
                    let concept = strategy.identity().unwrap();
                    if !optimized {
                        assert_eq!(e.outcome, "completed");
                        let replay = &e.result["tick_replay"];
                        assert!(replay["trades"].as_array().unwrap().is_empty());
                        assert_eq!(replay["realized_micro_usd"], 20_000_000);
                        assert!(replay["unresolved"].is_null());
                        assert!(replay["pending_entry"].is_null());
                        let hash = data::hash(replay).unwrap();
                        if let Some(prior) =
                            reference.insert((p.symbol.clone(), concept.clone()), hash.clone())
                        {
                            assert_eq!(prior, hash);
                        }
                    } else {
                        assert_eq!(e.outcome, "rejected");
                        let key = registry.experiment_key(&e.experiment_id).unwrap().unwrap();
                        assert!(
                            proof_screen::verified_rejection(&e.result, &key)
                                .unwrap()
                                .is_some()
                        );
                        if e.result["proof_audited"] == true {
                            audit_count += 1;
                        } else {
                            skipped_count += 1;
                        }
                    }
                    identities.push((
                        e.experiment_id,
                        p.symbol.clone(),
                        concept,
                        proof,
                        e.result["tick_replay"].clone(),
                    ));
                }
            }
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(registry.counts().unwrap().attempts, 16);
            let start = Instant::now();
            for (id, _, _, _, _) in &identities {
                assert!(matches!(
                    registry
                        .claim(id, chrono::Utc::now().timestamp_millis(), 300000)
                        .unwrap(),
                    rust_strategy_factory::storage::Claim::Reused(_)
                ));
            }
            let cached_ns = start.elapsed().as_nanos();
            assert_eq!(registry.counts().unwrap().attempts, 16);
            // Default always runs first in repetition0, establishing the exact replay oracle.
            for (_, symbol, concept, _, replay) in &identities {
                if optimized && !replay.is_null() {
                    assert_eq!(
                        data::hash(replay).unwrap(),
                        reference[&(symbol.clone(), concept.clone())]
                    );
                }
            }
            measurements.push(json!({"repetition":repetition,"mode":if optimized{"proof-policy"}else{"current-default-reference"},
                "new_evaluation_ns":elapsed,"cached_reads_ns":cached_ns,"new_attempts":16,"cached_reused":16,
                "audited_rejections":audit_count,"skipped_replays":skipped_count,"identities":identities.iter().map(|(id,s,c,p,_)|json!({"id":id,"symbol":s,"concept":c,"proof_hash":p})).collect::<Vec<_>>()}));
        }
    }
    fs::remove_dir_all(&directory).unwrap();
    println!(
        "{}",
        json!({"schema":1,"compute_revision":revision,"engine":rust_strategy_factory::ENGINE_VERSION,
        "class":"SYNTHETIC_PROVEN_NO_ENTRY_CURRENT_EVALUATION_COMPARISON",
        "workload":"8 profiles x 2 sides x12000 ticks; fixed distance deliberately below executable minimum+cost floor; all16 have signals and proven no possible entry",
        "measurement_scope":"Fresh SQLite per mode/batch; new evaluation timers include validation/features/diagnostics/replay where applicable/persistence/result checks; registry setup/corpus/cache-read clocks separate. Whole-child resources include all setup and checks.",
        "limitations":"No closed trades by construction, so not nonzero-trade research throughput or qualification. Default reference includes its existing M1 diagnostic. Repeated fixed identities are not independent audit calibration. No real data, full-history or VPS claim.","measurements":measurements})
    );
}
fn screen_proof(v: &serde_json::Value) -> String {
    assert!(!v["market_stop_proof"].is_null());
    data::hash(&v["market_stop_proof"]).unwrap()
}
