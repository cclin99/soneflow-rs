use super::super::model::SlopeModel;
fn model() -> SlopeModel {
    let mut edges = std::array::from_fn(|i| (i + 1) as f64);
    edges[20] = 24.0;
    SlopeModel::new(
        edges,
        std::array::from_fn(|r| (17 - r) as f64),
        std::array::from_fn(|_| std::array::from_fn(|c| 0.1 + 0.03 * c as f64)),
        0.0,
    )
    .unwrap()
}
#[test]
fn high_single_piece_samples_and_edges() {
    let m = model();
    let mut core = [0.0; 21];
    core[0] = 1e4;
    let (total, samples) = m.evaluate(&core);
    let mut starts = [1e4; 21];
    let mut expected = 1e4;
    for i in 1..21 {
        let slope = 0.1 + 0.03 * ((i - 1).min(7)) as f64;
        let width = if i == 20 { 4.0 } else { 1.0 };
        let low = starts[i] - slope * width;
        expected += width * (starts[i] * 0.5 + low * 0.5);
        if i < 20 {
            starts[i + 1] = low;
        }
    }
    assert_eq!(total.to_bits(), expected.to_bits());
    for (k, value) in samples.iter().enumerate() {
        let z = (k + 1) as f64 / 10.0;
        let band = ((z.ceil() as usize).saturating_sub(1)).min(20);
        let reference = if band == 0 {
            1e4
        } else {
            starts[band] - (0.1 + 0.03 * ((band - 1).min(7)) as f64) * (z - band as f64)
        };
        assert_eq!(value.to_bits(), reference.to_bits());
    }
}
#[test]
fn sparse_and_walking_range_transitions() {
    let m = model();
    for seed in 0..120 {
        let mut core = [0.0; 21];
        let mut previous = 17.0;
        for (i, v) in core.iter_mut().enumerate() {
            if (i + seed) % 7 == 0 {
                previous = (seed % 18) as f64;
            } else {
                previous *= 0.6 + 0.05 * ((i + seed) % 11) as f64;
            }
            *v = if (i + seed) % 3 == 0 { 0.0 } else { previous };
        }
        let (n, s) = m.evaluate(&core);
        let mut into = [0.0; 240];
        assert_eq!(n.to_bits(), m.evaluate_into(&core, &mut into).to_bits());
        assert_eq!(n.to_bits(), m.total_only(&core).to_bits());
        assert_eq!(s, into);
        assert!(n.is_finite() && s.iter().all(|v| v.is_finite() && *v >= 0.0));
    }
}
