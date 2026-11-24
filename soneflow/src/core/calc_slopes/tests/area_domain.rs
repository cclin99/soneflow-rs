use super::super::model::SlopeModel;
fn model(slope: f64) -> SlopeModel {
    let mut edges = std::array::from_fn(|i| (i + 1) as f64);
    edges[20] = 24.0;
    SlopeModel::new(
        edges,
        std::array::from_fn(|r| (17 - r) as f64),
        [[slope; 8]; 18],
        0.001,
    )
    .unwrap()
}
fn check(m: &SlopeModel, core: &[f64; 21]) {
    let (n, s) = m.evaluate(core);
    let mut into = [0.0; 240];
    assert!(n.is_finite() && (0.0..=25.0e306).contains(&n));
    let max = core.iter().copied().fold(0.0, f64::max);
    assert!(s.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= max));
    assert_eq!(n.to_bits(), m.evaluate_into(core, &mut into).to_bits());
    assert_eq!(n.to_bits(), m.total_only(core).to_bits());
    assert_eq!(s, into);
}
#[test]
fn maximum_valid_endpoints_and_area_stay_finite() {
    let high = 1e306_f64;
    let near = f64::from_bits(high.to_bits() - 1);
    for slope in [f64::from_bits(1), high * 0.25, f64::MAX] {
        let m = model(slope);
        check(&m, &[high; 21]);
        check(&m, &[near; 21]);
        check(
            &m,
            &std::array::from_fn(|i| if i % 2 == 0 { high } else { near }),
        );
        check(
            &m,
            &std::array::from_fn(|i| if i % 2 == 0 { high } else { 0.0 }),
        );
    }
}
#[test]
fn adjacent_subnormal_endpoints_remain_bounded() {
    for bits in [1_u64, 2, 3, 4, 0x000f_ffff_ffff_fffe] {
        let low = f64::from_bits(bits);
        let high = f64::from_bits(bits + 1);
        for slope in [f64::from_bits(1), 1.0, f64::MAX] {
            let m = model(slope);
            check(&m, &[low; 21]);
            check(
                &m,
                &std::array::from_fn(|i| if i % 2 == 0 { high } else { low }),
            );
        }
    }
}
#[test]
fn zero_width_pieces_preserve_valid_finite_area() {
    let m = model(f64::MAX);
    for high in [1.0, 17.0, 1000.0, 1e306] {
        let mut core = [0.0; 21];
        core[0] = high;
        check(&m, &core);
        let (_, s) = m.evaluate(&core);
        assert_eq!(s[9], high);
        assert_eq!(s[10], 0.0);
    }
}
