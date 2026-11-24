use super::super::model::SlopeModel;
#[test]
fn sanitizer_bit_boundaries() {
    let model = SlopeModel::new(
        std::array::from_fn(|i| (i + 1) as f64 * 24.0 / 21.0),
        std::array::from_fn(|r| (17 - r) as f64),
        [[1.0; 8]; 18],
        0.0,
    )
    .unwrap();
    let values = [
        0.0,
        -0.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        1.0,
        -1.0,
        f64::MAX,
        -f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::from_bits(0xfff8_0000_0000_0001),
    ];
    for value in values {
        let expected = if value.is_finite() && value >= 0.0 {
            value
        } else {
            0.0
        };
        let core = [value; 21];
        let (total, samples) = model.evaluate(&core);
        assert!(samples.iter().all(|s| s.to_bits() == expected.to_bits()));
        let mut into = [0.0; 240];
        assert_eq!(
            total.to_bits(),
            model.evaluate_into(&core, &mut into).to_bits()
        );
        assert_eq!(total.to_bits(), model.total_only(&core).to_bits());
    }
}
