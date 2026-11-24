use super::super::model::SlopeModel;
fn model(scale: f64) -> SlopeModel {
    SlopeModel::new(
        std::array::from_fn(|i| (i + 1) as f64 * 24.0 / 21.0),
        std::array::from_fn(|r| (17 - r) as f64),
        [[scale; 8]; 18],
        0.0001,
    )
    .unwrap()
}
fn check(m: &SlopeModel, c: &[f64; 21]) {
    let (n, s) = m.evaluate(c);
    let mut t = [0.0; 240];
    assert_eq!(n.to_bits(), m.evaluate_into(c, &mut t).to_bits());
    assert_eq!(n.to_bits(), m.total_only(c).to_bits());
    assert_eq!(n.to_bits(), m.evaluate(c).0.to_bits());
    assert_eq!(s, t);
    let max = c
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    assert!(n.is_finite() && n >= 0.0);
    assert!(s.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= max));
}
#[test]
fn scales_zero_width_and_overflow_distance() {
    for slope in [f64::from_bits(1), 1e-300, 1.0, 1e100, 1e306, f64::MAX] {
        let m = model(slope);
        for high in [0.0, 1e-300, 1.0, 17.0, 100.0, 1e306] {
            let mut c = [0.0; 21];
            c[0] = high;
            check(&m, &c);
        }
    }
}
#[test]
fn threshold_ties_and_neighbors() {
    let m = model(0.75);
    for threshold in 1..=17 {
        let t = threshold as f64;
        for value in [
            f64::from_bits(t.to_bits() - 1),
            t,
            f64::from_bits(t.to_bits() + 1),
        ] {
            let mut c = [0.0; 21];
            c[0] = value;
            check(&m, &c);
            check(&m, &[value; 21]);
        }
    }
    check(&m, &[f64::from_bits(1); 21]);
    check(&m, &[0.0; 21]);
}
#[test]
fn invalid_and_above_domain_cores() {
    let m = model(1.0);
    let mut c = [0.0; 21];
    for v in [-1.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
        c[0] = v;
        assert_eq!(m.evaluate(&c), (0.0, [0.0; 240]));
    }
    c[0] = f64::MAX;
    let _ = m.evaluate(&c);
    let _ = m.total_only(&c);
}
#[test]
fn seeded_frames_all_paths() {
    let m = model(3.0);
    let mut state = 19_u64;
    for _ in 0..2000 {
        let c = std::array::from_fn(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 11) as f64 / (1_u64 << 53) as f64 * 50.0
        });
        check(&m, &c);
    }
}
