use super::super::model::SlopeModel;
fn fixture() -> ([f64; 21], [f64; 18], [[f64; 8]; 18]) {
    let mut edges = std::array::from_fn(|i| (i + 1) as f64);
    edges[20] = 24.0;
    (
        edges,
        std::array::from_fn(|i| (17 - i) as f64),
        [[1.0; 8]; 18],
    )
}
fn model() -> SlopeModel {
    let (e, t, s) = fixture();
    SlopeModel::new(e, t, s, 0.0).unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-12 * b.abs().max(1.0), "{a} != {b}");
}
#[test]
fn flat_and_interfaces() {
    let m = model();
    let core = [3.0; 21];
    let (n, s) = m.evaluate(&core);
    close(n, 72.0);
    assert_eq!(s, [3.0; 240]);
    let mut into = [0.0; 240];
    assert_eq!(n.to_bits(), m.evaluate_into(&core, &mut into).to_bits());
    assert_eq!(n.to_bits(), m.total_only(&core).to_bits());
    assert_eq!(s, into);
}
#[test]
fn continued_descent() {
    let m = model();
    let mut c = [0.0; 21];
    c[0] = 20.0;
    let (n, s) = m.evaluate(&c);
    close(n, 220.0);
    for (k, v) in s.iter().enumerate() {
        let z = (k + 1) as f64 / 10.0;
        close(*v, if z <= 1.0 { 20.0 } else { (21.0 - z).max(0.0) });
    }
}
#[test]
fn core_hit_and_jump() {
    let m = model();
    let mut c = [1.5; 21];
    c[0] = 2.0;
    c[2] = 4.0;
    let (_, s) = m.evaluate(&c);
    close(s[9], 2.0);
    close(s[14], 1.5);
    close(s[19], 1.5);
    close(s[20], 4.0);
}
#[test]
fn invalid_models() {
    let (e, t, s) = fixture();
    let mut x = e;
    x[2] = x[1];
    assert!(SlopeModel::new(x, t, s, 0.0).is_err());
    let mut x = t;
    x[1] = x[0];
    assert!(SlopeModel::new(e, x, s, 0.0).is_err());
    let mut x = s;
    x[0][0] = 0.0;
    assert!(SlopeModel::new(e, t, x, 0.0).is_err());
    assert!(SlopeModel::new(e, t, s, 0.002).is_err());
}
#[test]
#[should_panic]
fn wrong_slice() {
    model().evaluate_into(&[0.0; 21], &mut [0.0; 239]);
}
