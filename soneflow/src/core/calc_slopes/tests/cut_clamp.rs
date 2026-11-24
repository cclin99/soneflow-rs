use super::super::model::SlopeModel;
fn check(n: f64, core: f64, slope: f64, end: f64) {
    let mut e = std::array::from_fn(|i| (i + 1) as f64);
    e[1] = end;
    e[20] = 24.0;
    let m = SlopeModel::new(
        e,
        std::array::from_fn(|r| (17 - r) as f64),
        [[slope; 8]; 18],
        0.0,
    )
    .unwrap();
    let mut c = [core; 21];
    c[0] = n;
    let (total, s) = m.evaluate(&c);
    let mut out = [0.0; 240];
    assert_eq!(total.to_bits(), m.total_only(&c).to_bits());
    assert_eq!(total.to_bits(), m.evaluate_into(&c, &mut out).to_bits());
    for (a, b) in s.iter().zip(out) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    assert!(s.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= n));
    if n <= 1e306 {
        assert!(total.is_finite() && total >= 0.0);
    }
    if end == 2.0 {
        let target = core.max(0.0);
        let expected = if n - target <= slope {
            if 1.0 + (n - target) / slope < end {
                core
            } else {
                target
            }
        } else {
            n - slope
        };
        assert_eq!(s[19].to_bits(), expected.to_bits());
    }
}
#[test]
fn band_cut_predicate_neighbors() {
    for (n, core) in [(100.0, 20.0), (1e306, 1e305), (f64::MAX, f64::MAX * 0.5)] {
        let d: f64 = n - core;
        for bits in [d.to_bits() - 1, d.to_bits(), d.to_bits() + 1] {
            let slope = f64::from_bits(bits);
            if slope.is_finite() {
                check(n, core, slope, 2.0);
            }
        }
    }
}
#[test]
fn subnormal_cut_and_signed_zero_targets() {
    for core in [-0.0, 0.0, f64::from_bits(1)] {
        for bits in 1..=5 {
            check(f64::from_bits(4), core, f64::from_bits(bits), 2.0);
        }
    }
}
#[test]
fn drop_overflow_stays_on_unchanged_reach_branch() {
    check(1e306, 0.0, f64::MAX, 2.9);
    check(f64::MAX, 0.0, f64::MAX, 2.9);
}
