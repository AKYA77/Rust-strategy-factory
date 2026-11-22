use rust_strategy_factory::{
    candles::{Candle, Ohlc},
    indicators::Mask,
    sparse_mask::{SparseMask, Storage},
    strategies::{Features, Rule},
};

fn oracle(values: &[bool]) -> Mask {
    Mask::from_bools(values.iter().copied())
}

fn seeded_values(len: usize, density: u64, seed: &mut u64) -> Vec<bool> {
    (0..len)
        .map(|_| {
            *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            *seed % 100 < density
        })
        .collect()
}

#[test]
fn sparse_and_dense_backing_match_the_dense_oracle() {
    let mut seed = 73_u64;
    let mut checked = 0_usize;
    for len in [0_usize, 1, 2, 63, 64, 65, 127, 128, 129, 1000, 4097] {
        for density in [0_u64, 1, 2, 10, 100] {
            for _ in 0..20 {
                let values = seeded_values(len, density, &mut seed);
                let sparse = SparseMask::from_bools(values.iter().copied());
                let dense = oracle(&values);
                sparse.validate().unwrap();
                assert_eq!(sparse.len(), len);
                assert_eq!(
                    sparse.count(),
                    values.iter().filter(|&&value| value).count()
                );
                let expected: Vec<usize> = values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, &value)| value.then_some(index))
                    .collect();
                assert_eq!(sparse.set_indices().unwrap().collect::<Vec<_>>(), expected);
                let converted = SparseMask::from_mask(&dense).unwrap();
                assert!(sparse.equals_logical(&converted).unwrap());
                assert_eq!(sparse.to_mask().unwrap().count(), dense.count());
                assert_eq!(sparse.to_mask().unwrap().words(), dense.words());
                // Serialized identity is stable across a round trip.
                let json = serde_json::to_string(&sparse).unwrap();
                let back: SparseMask = serde_json::from_str(&json).unwrap();
                assert_eq!(back, sparse);
                assert_eq!(serde_json::to_string(&back).unwrap(), json);
                // Prefix traversal agrees at word boundaries and partial tails.
                for count in [0, len / 2, len] {
                    assert_eq!(
                        sparse
                            .set_indices_prefix(count)
                            .unwrap()
                            .collect::<Vec<_>>(),
                        expected
                            .iter()
                            .copied()
                            .take_while(|&index| index < count)
                            .collect::<Vec<_>>()
                    );
                }
                assert!(sparse.set_indices_prefix(len + 1).is_err());
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 1100);
}

#[test]
fn boolean_combinations_and_complement_match_the_dense_oracle() {
    let mut seed = 4_211_u64;
    for len in [1_usize, 64, 65, 129, 1000] {
        for _ in 0..25 {
            let left = seeded_values(len, 5, &mut seed);
            let right = seeded_values(len, 40, &mut seed);
            let a = SparseMask::from_bools(left.iter().copied());
            let b = SparseMask::from_bools(right.iter().copied());
            let (oa, ob) = (oracle(&left), oracle(&right));
            for and in [true, false] {
                let combined = a.combine(&b, and).unwrap();
                let expected = oa.combine(&ob, and).unwrap();
                assert_eq!(combined.len(), len);
                assert_eq!(combined.count(), expected.count());
                assert!(
                    combined
                        .equals_logical(&SparseMask::from_mask(&expected).unwrap())
                        .unwrap()
                );
            }
            let complement = a.complement().unwrap();
            let expected: Vec<bool> = left.iter().map(|&value| !value).collect();
            assert_eq!(
                complement.set_indices().unwrap().collect::<Vec<_>>(),
                expected
                    .iter()
                    .enumerate()
                    .filter_map(|(index, &value)| value.then_some(index))
                    .collect::<Vec<_>>()
            );
            assert!(
                complement
                    .combine(&a, true)
                    .unwrap()
                    .set_indices()
                    .unwrap()
                    .next()
                    .is_none()
            );
        }
    }
}

#[test]
fn prefix_comparison_detects_only_active_bit_differences() {
    let a = SparseMask::from_indices(129, [0, 63, 64, 128]).unwrap();
    let b = SparseMask::from_indices(129, [0, 63, 64, 128]).unwrap();
    let c = SparseMask::from_indices(129, [0, 63, 65, 128]).unwrap();
    assert!(a.matches_prefix(&b, 129).unwrap());
    assert!(a.matches_prefix(&b, 64).unwrap());
    assert!(!a.matches_prefix(&c, 129).unwrap());
    assert!(!a.matches_prefix(&c, 65).unwrap());
    assert!(a.matches_prefix(&c, 64).unwrap());
    assert!(
        a.matches_prefix(&SparseMask::from_indices(64, []).unwrap(), 129)
            .is_err()
    );
}

#[test]
fn representation_choice_is_density_boundaried_and_canonical() {
    assert_eq!(SparseMask::from_bools([]).storage(), None);
    assert_eq!(
        SparseMask::from_bools([true; 64]).storage(),
        Some(Storage::Dense)
    );
    assert_eq!(
        SparseMask::from_bools([true; 1_000_000]).storage(),
        Some(Storage::Dense)
    );
    let one_bit = SparseMask::from_bools((0..64).map(|index| index == 7));
    assert_eq!(one_bit.storage(), Some(Storage::Sparse));
    assert_eq!(one_bit.density_bps(), Some(156));
    let two_bits = SparseMask::from_bools((0..64).map(|index| index == 7 || index == 9));
    assert_eq!(two_bits.storage(), Some(Storage::Dense));
    let sparse_long = SparseMask::from_indices(1_000_000, [0, 999_999]).unwrap();
    assert_eq!(sparse_long.storage(), Some(Storage::Sparse));
    assert_eq!(sparse_long.count(), 2);
    assert_eq!(
        sparse_long.set_indices().unwrap().collect::<Vec<_>>(),
        vec![0, 999_999]
    );
    // Unsorted and duplicated indices normalize to one canonical mask.
    let shuffled = SparseMask::from_indices(100, [9, 3, 9, 40, 3]).unwrap();
    assert_eq!(
        shuffled.set_indices().unwrap().collect::<Vec<_>>(),
        vec![3, 9, 40]
    );
    assert_eq!(
        serde_json::to_string(&shuffled).unwrap(),
        serde_json::to_string(&SparseMask::from_indices(100, [3, 9, 40]).unwrap()).unwrap()
    );
    assert!(SparseMask::from_indices(4, [4]).is_err());
    assert!(SparseMask::from_indices(4, [1_000_000]).is_err());
    assert!(SparseMask::from_indices(0, []).unwrap().validate().is_ok());
}

#[test]
fn malformed_backing_and_padding_refuse_before_iteration() {
    for value in [
        serde_json::json!({"len":1}),
        serde_json::json!({"len":64,"words":[0],"indices":[1]}),
        serde_json::json!({"len":64,"words":[0,0]}),
        serde_json::json!({"len":65,"words":[0,2]}),
        serde_json::json!({"len":65,"words":[0,1]}),
        serde_json::json!({"len":4,"indices":[4]}),
        serde_json::json!({"len":4,"indices":[3,3]}),
        serde_json::json!({"len":4,"indices":[3,2]}),
        serde_json::json!({"len":64,"indices":[0,1]}),
        serde_json::json!({"len":1,"words":[1]}),
    ] {
        let mask: SparseMask = serde_json::from_value(value).unwrap();
        assert!(mask.validate().is_err());
        assert!(mask.set_indices().is_err());
    }
    assert!(
        serde_json::from_value::<SparseMask>(serde_json::json!({"len":64,"indices":[0],"extra":1}))
            .is_err()
    );
    // Zero-count nonzero masks are canonically dense, never sparse-empty.
    let zero = SparseMask::from_words(64, vec![0]).unwrap();
    assert_eq!(zero.storage(), Some(Storage::Dense));
    assert_eq!(zero.count(), 0);
    assert!(zero.validate().is_ok());
}

#[test]
fn shared_feature_cache_retains_sparse_session_masks_logically_equal() {
    // Monday 2025-01-06T00:00:00Z, one week of M1 candles.
    let base = 1_736_121_600_000_i64;
    let candles: Vec<Candle> = (0..10_080)
        .map(|index| Candle {
            start_ms: base + index * 60_000,
            available_ms: base + (index + 1) * 60_000,
            publication_sequence: index as u64 + 1,
            bid: Ohlc {
                open: 100_000,
                high: 100_010,
                low: 99_990,
                close: 100_000,
            },
            ask: Ohlc {
                open: 100_010,
                high: 100_020,
                low: 100_000,
                close: 100_010,
            },
            events: 1,
        })
        .collect();
    let rule = Rule::UtcSession {
        start_hour: 10,
        end_hour: 11,
        weekdays: 1,
    };
    let mut features = Features::new(&candles, 8_000_000).unwrap();
    let sparse = features.signal_sparse(&rule).unwrap();
    assert_eq!(sparse.len(), candles.len());
    assert_eq!(sparse.storage(), Some(Storage::Sparse));
    assert_eq!(sparse.count(), 60);
    assert_eq!(sparse.density_bps(), Some(59));
    let dense = features.signal(&rule).unwrap();
    assert_eq!(dense.len(), candles.len());
    assert_eq!(dense.count(), 60);
    assert!(
        sparse
            .equals_logical(&SparseMask::from_mask(&dense).unwrap())
            .unwrap()
    );
    let cached = features.signal_sparse(&rule).unwrap();
    assert_eq!(cached, sparse);
    assert!(features.cache_hits >= 1);
}
