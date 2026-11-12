use rust_strategy_factory::{
    contracts::{Profile, Side},
    data::{self, Tick},
    tick_engine::{Costs, ExitReason},
    tick_index::{Hit, Index},
};
#[test]
fn seeded_barrier_queries_match_sequential_on_effective_quotes() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs::default();
    let mut rng = 42_u64;
    let mut next = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        rng
    };
    let ticks: Vec<_> = (0..4096)
        .map(|i| Tick {
            timestamp_ms: (i / 3) * 1000,
            sequence: i as u64,
            bid: 100000 + (next() % 1000) as i64,
            ask: 101000 + (next() % 100) as i64,
        })
        .collect();
    let d = data::dataset(
        p,
        ticks,
        "synthetic randomized ties",
        "generated".into(),
        true,
    )
    .unwrap();
    for block in [8, 64, 256, 1024] {
        let idx = Index::build(&d, &costs, block, 4096).unwrap();
        idx.validate_profile(&d.profile).unwrap();
        for _ in 0..500 {
            let start = next() as usize % 4096;
            let deadline = d.ticks[start].timestamp_ms + (next() % 20000) as i64;
            let side = if next() % 2 == 0 {
                Side::Long
            } else {
                Side::Short
            };
            let (stop, target) = if side == Side::Long {
                (
                    100010 + (next() % 300) as i64,
                    100700 + (next() % 1000) as i64,
                )
            } else {
                (
                    101090 + (next() % 500) as i64,
                    100500 + (next() % 500) as i64,
                )
            };
            let sequential = (start..d.ticks.len()).find_map(|i| {
                let (bid, ask) = costs.effective(&d.profile, d.ticks[i]).unwrap();
                let reason = if d.ticks[i].timestamp_ms >= deadline {
                    Some(ExitReason::DeadlineMarket)
                } else if side == Side::Long && bid <= stop || side == Side::Short && ask >= stop {
                    Some(ExitReason::StopMarket)
                } else if side == Side::Long && bid >= target
                    || side == Side::Short && ask <= target
                {
                    Some(ExitReason::TargetLimit)
                } else {
                    None
                };
                reason.map(|reason| Hit {
                    event_index: i,
                    reason,
                })
            });
            assert_eq!(
                idx.first_hit(start, side, stop, target, deadline).unwrap(),
                sequential
            );
        }
        assert_eq!(
            idx.first_hit(4096, Side::Long, 100, 200, i64::MAX).unwrap(),
            None
        );
        assert!(idx.first_hit(0, Side::Long, 200, 100, i64::MAX).is_err());
    }
}
