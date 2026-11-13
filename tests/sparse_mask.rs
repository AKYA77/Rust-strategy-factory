use rust_strategy_factory::indicators::Mask;
#[test]
fn sparse_indices_match_scalar_oracle_for_boundaries_densities_and_seeded_masks() {
    let mut seed = 73u64;
    let mut checked = 0;
    for len in [0, 1, 2, 63, 64, 65, 127, 128, 129, 1000, 4097] {
        for density in [0, 1, 2, 10, 100] {
            for _ in 0..20 {
                let values = (0..len)
                    .map(|_| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        seed % 100 < density
                    })
                    .collect::<Vec<_>>();
                let mask = Mask::from_bools(values.iter().copied());
                let expected = values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &b)| b.then_some(i))
                    .collect::<Vec<_>>();
                let actual = mask.set_indices().unwrap().collect::<Vec<_>>();
                assert_eq!(actual, expected);
                assert_eq!(actual.len(), mask.count());
                for count in [0, len / 2, len] {
                    assert_eq!(
                        mask.set_indices_prefix(count).unwrap().collect::<Vec<_>>(),
                        expected
                            .iter()
                            .copied()
                            .take_while(|&i| i < count)
                            .collect::<Vec<_>>()
                    );
                }
                assert!(mask.set_indices_prefix(len + 1).is_err());
                assert!(actual.windows(2).all(|p| p[0] < p[1]));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 1100);
}
#[test]
fn malformed_sparse_backing_and_padding_refuse_before_iteration() {
    for value in [
        serde_json::json!({"words":[],"len":1}),
        serde_json::json!({"words":[2],"len":1}),
        serde_json::json!({"words":[0],"len":0}),
        serde_json::json!({"words":[1],"len":129}),
    ] {
        let mask: Mask = serde_json::from_value(value).unwrap();
        assert!(mask.set_indices().is_err());
    }
    let mask = Mask::from_bools([true]);
    let mut indices = mask.set_indices().unwrap();
    assert_eq!(indices.next(), Some(0));
    for _ in 0..10 {
        assert_eq!(indices.next(), None);
    }
}
