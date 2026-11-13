use rust_strategy_factory::indicators::Mask;
#[test]
fn seeded_packed_prefix_checks_match_an_independent_boolean_oracle() {
    let mut state = 42u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        state
    };
    let mut queries = 0;
    for length in [0, 1, 2, 63, 64, 65, 127, 128, 129, 255, 511, 1025] {
        for trial in 0..200 {
            let a = (0..length).map(|_| next() >> 63 != 0).collect::<Vec<_>>();
            let other_len = length + next() as usize % 70;
            let mut b = (0..other_len)
                .map(|_| next() >> 63 != 0)
                .collect::<Vec<_>>();
            let prefix = next() as usize % (length + 1);
            if trial % 3 == 0 {
                b[..prefix].copy_from_slice(&a[..prefix]);
            }
            let expected = a[..prefix] == b[..prefix];
            assert_eq!(
                Mask::from_bools(a)
                    .matches_prefix(&Mask::from_bools(b), prefix)
                    .unwrap(),
                expected
            );
            queries += 1;
        }
    }
    assert_eq!(queries, 2400);
}
#[test]
fn exact_words_partial_words_empty_bounds_and_malformed_storage_are_checked() {
    for end in [1, 63, 64, 65, 127, 128, 129, 130] {
        let a = Mask::from_bools(vec![false; 180]);
        let mut bits = vec![false; 180];
        bits[end - 1] = true;
        let b = Mask::from_bools(bits);
        assert!(a.matches_prefix(&b, end - 1).unwrap());
        assert!(!a.matches_prefix(&b, end).unwrap());
    }
    let a = Mask::from_bools(vec![false; 130]);
    let mut bits = vec![false; 130];
    bits[64] = true;
    let b = Mask::from_bools(bits);
    assert!(a.matches_prefix(&b, 64).unwrap());
    assert!(!a.matches_prefix(&b, 65).unwrap());
    let prefix = Mask::from_bools(vec![false; 64]);
    assert!(a.matches_prefix(&prefix, 64).unwrap());
    assert!(a.matches_prefix(&prefix, 65).is_err());
    let empty = Mask::from_bools([]);
    assert!(empty.is_empty());
    assert!(a.matches_prefix(&empty, 0).unwrap());
    assert!(empty.matches_prefix(&empty, 0).unwrap());
    assert!(empty.matches_prefix(&a, 1).is_err());
    for malformed in [
        serde_json::json!({"words":[],"len":65}),
        serde_json::json!({"words":[u64::MAX],"len":1}),
    ] {
        let bad: Mask = serde_json::from_value(malformed).unwrap();
        assert!(bad.matches_prefix(&a, 0).is_err());
        assert!(bad.combine(&a, true).is_err());
    }
}
