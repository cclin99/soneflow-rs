use soneflow::{loudness_zwst, FieldType, SoneflowError};

#[test]
fn temporal_extreme_fault_does_not_poison_reusable_processor() {
    let mut processor = soneflow::ZwtvProcessor::new();
    assert!(matches!(
        processor.process(&vec![f64::MAX; 4800], 48_000.0, FieldType::Free),
        Err(SoneflowError::InternalNumericFault { .. })
    ));
    let good: Vec<_> = (0..4800)
        .map(|i| ((i % 71) as f64 - 35.0) * 0.0001)
        .collect();
    let actual = processor.process(&good, 48_000.0, FieldType::Free).unwrap();
    let fresh = soneflow::loudness_zwtv(&good, 48_000.0, FieldType::Free).unwrap();
    assert_eq!(
        actual.n.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        fresh.n.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    assert_eq!(
        actual
            .n_specific
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        fresh
            .n_specific
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
    assert_eq!(actual.time_axis, fresh.time_axis);
}

#[test]
fn stationary_finite_extremes_are_internal_faults() {
    for value in [f64::MAX, -f64::MAX] {
        let result = loudness_zwst(&vec![value; 4800], 48_000.0, FieldType::Free);
        assert!(matches!(
            result,
            Err(SoneflowError::InternalNumericFault { .. })
        ));
    }
}

#[test]
fn stationary_input_error_precedence_stays_rate_length_finiteness() {
    assert_eq!(
        loudness_zwst(&[f64::NAN], 1.0, FieldType::Free)
            .unwrap_err()
            .code(),
        3
    );
    assert_eq!(
        loudness_zwst(&[f64::NAN], 48_000.0, FieldType::Free)
            .unwrap_err()
            .code(),
        2
    );
    assert_eq!(
        loudness_zwst(&vec![f64::NAN; 4800], 48_000.0, FieldType::Free)
            .unwrap_err()
            .code(),
        4
    );
}
