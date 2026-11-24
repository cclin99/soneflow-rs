use super::super::model::SlopeModel;
fn model(edges: [f64; 21], slopes: [[f64; 8]; 18]) -> SlopeModel {
    SlopeModel::new(edges, std::array::from_fn(|r| (17 - r) as f64), slopes, 0.0).unwrap()
}
fn edges() -> [f64; 21] {
    let mut e = std::array::from_fn(|i| (i + 1) as f64);
    e[20] = 24.0;
    e
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-11, "{a} != {b}");
}
#[test]
fn all_ranges_inside_band_and_sample_endpoint() {
    let m = model(edges(), [[40.0; 8]; 18]);
    let mut c = [0.0; 21];
    c[0] = 20.0;
    let (n, s) = m.evaluate(&c);
    close(n, 25.0);
    close(s[9], 20.0);
    close(s[10], 16.0);
    close(s[14], 0.0);
}
#[test]
fn last_tenth_and_many_pieces_between_samples() {
    let mut e = edges();
    e[0] = 0.96;
    let m = model(e, [[1000.0; 8]; 18]);
    let mut c = [0.0; 21];
    c[0] = 20.0;
    let (n, s) = m.evaluate(&c);
    close(n, 19.4);
    close(s[8], 20.0);
    close(s[9], 0.0);
}
#[test]
fn exact_shared_edge_and_upward_jump() {
    let mut e = edges();
    e[0] = 0.1;
    e[1] = 0.2;
    let m = model(e, [[1.0; 8]; 18]);
    let mut c = [3.0; 21];
    c[0] = 1.0;
    c[1] = 2.0;
    let (_, s) = m.evaluate(&c);
    assert_eq!(&s[0..3], &[1.0, 2.0, 3.0]);
}
#[test]
fn range_uses_below_threshold_slope() {
    let mut slopes = [[1.0; 8]; 18];
    slopes[1] = [2.0; 8];
    let m = model(edges(), slopes);
    let mut c = [0.0; 21];
    c[0] = 17.0;
    let (_, s) = m.evaluate(&c);
    close(s[10], 16.8);
    close(s[14], 16.0);
    close(s[15], 15.9);
}
#[test]
fn nonfinite_models_and_collapsed_edges() {
    let e = edges();
    let t = std::array::from_fn(|r| (17 - r) as f64);
    let s = [[1.0; 8]; 18];
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut a = e;
        a[0] = value;
        assert!(SlopeModel::new(a, t, s, 0.0).is_err());
        let mut b = t;
        b[0] = value;
        assert!(SlopeModel::new(e, b, s, 0.0).is_err());
        let mut c = s;
        c[0][0] = value;
        assert!(SlopeModel::new(e, t, c, 0.0).is_err());
    }
    let mut a = e;
    a[0] = f64::from_bits(1);
    a[1] = f64::from_bits(2);
    assert!(SlopeModel::new(a, t, s, 0.001).is_err());
}
