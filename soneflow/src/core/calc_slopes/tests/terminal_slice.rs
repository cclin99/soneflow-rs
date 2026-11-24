use super::super::model::SlopeModel;
fn model(first: f64, second: f64, slope: f64, extension: f64) -> SlopeModel {
    let mut e = std::array::from_fn(|i| (i + 1) as f64);
    e[0] = first;
    e[1] = second;
    e[20] = 24.0;
    SlopeModel::new(
        e,
        std::array::from_fn(|r| (17 - r) as f64),
        [[slope; 8]; 18],
        extension,
    )
    .unwrap()
}
#[test]
fn terminal_cut_sample_uses_computed_endpoint() {
    let m = model(0.09017991828253465, 0.1, 100101.0, 0.0);
    let mut c = [0.0; 21];
    c[0] = 1000.0;
    let (_, s) = m.evaluate(&c);
    let low: f64 = 1000.0 - 100101.0 * (0.1 - 0.09017991828253465);
    assert!(low > 17.0);
    assert_eq!(s[0].to_bits(), low.to_bits());
}
#[test]
fn terminal_piece_with_no_samples_does_not_move_cursor() {
    let mut e = std::array::from_fn(|i| (i + 1) as f64);
    e[0] = 0.01;
    e[1] = 0.02;
    e[2] = 0.1;
    e[3] = f64::from_bits(0.1_f64.to_bits() + 1);
    e[20] = 24.0;
    let m = SlopeModel::new(
        e,
        std::array::from_fn(|r| (17 - r) as f64),
        [[1.0; 8]; 18],
        0.0,
    )
    .unwrap();
    let mut c = [0.0; 21];
    c[0] = 20.0;
    let (_, s) = m.evaluate(&c);
    let at_second = 20.0 - (e[1] - e[0]);
    let at_third = at_second - (e[2] - e[1]);
    let at_fourth = at_third - (e[3] - e[2]);
    assert_eq!(s[0].to_bits(), at_third.to_bits());
    assert_eq!(s[1].to_bits(), (at_fourth - (0.2 - e[3])).to_bits());
}
#[test]
fn terminal_extension_keeps_last_sample_inside_piece() {
    let m = model(1.0, 2.0, 1.0, 0.001);
    let mut c = [0.0; 21];
    c[0] = 100.0;
    let (n, s) = m.evaluate(&c);
    assert!((s[239] - (100.0 - (24.0 - 1.001))).abs() < 1e-12);
    let mut into = [0.0; 240];
    assert_eq!(n.to_bits(), m.evaluate_into(&c, &mut into).to_bits());
    assert_eq!(n.to_bits(), m.total_only(&c).to_bits());
    assert_eq!(s, into);
}

#[test]
fn terminal_threshold_sample_uses_assigned_endpoint() {
    let m = model(0.05, 0.1, 20.0, 0.0);
    let mut c = [0.0; 21];
    c[0] = 18.0;
    let (_, s) = m.evaluate(&c);
    assert_eq!(s[0].to_bits(), 17.0_f64.to_bits());
}

#[test]
fn assigned_endpoint_differs_from_recomputed_value() {
    let first = 0.002;
    let high = 1000.0;
    let target = 0.1;
    let slope = (high - target) / (0.1 - first);
    let drop = slope * (0.1 - first);
    assert!(high - target <= drop);
    assert_eq!(first + (high - target) / slope, 0.1);
    assert!(high - drop > target);
    let mut e = std::array::from_fn(|i| (i + 1) as f64);
    e[0] = first;
    e[1] = 0.1;
    e[20] = 24.0;
    let t = std::array::from_fn(|r| target * (17 - r) as f64 / 17.0);
    let m = SlopeModel::new(e, t, [[slope; 8]; 18], 0.0).unwrap();
    let mut c = [0.0; 21];
    c[0] = high;
    let (_, s) = m.evaluate(&c);
    assert_eq!(s[0].to_bits(), target.to_bits());
}
