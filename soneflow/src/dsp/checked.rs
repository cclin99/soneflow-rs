//! Fallible stationary filtering with explicit logical sample coordinates.
use super::filtfilt::odd_ext;
use super::sos::{sosfilt_zi, Sos};
use crate::tables_noct::{CHEBY_QS, CHEBY_SOS};

pub(crate) fn sos(sections: &[Sos], x: &mut [f64], zi: &mut [[f64; 2]]) -> Result<(), usize> {
    sos_mapped(sections, x, zi, |i| i)
}

fn sos_mapped(
    sections: &[Sos],
    x: &mut [f64],
    zi: &mut [[f64; 2]],
    coordinate: impl Fn(usize) -> usize,
) -> Result<(), usize> {
    let mut first_bad = x.len();
    let mut first_fault = None;
    for (s, z) in sections.iter().zip(zi.iter_mut()) {
        let (mut z0, mut z1) = (z[0], z[1]);
        if !z0.is_finite() || !z1.is_finite() {
            first_bad = 0;
            let point = coordinate(0);
            first_fault = Some(first_fault.map_or(point, |old: usize| old.min(point)));
            continue;
        }
        for (i, v) in x[..first_bad].iter_mut().enumerate() {
            let xin = *v;
            let y = s.b[0] * xin + z0;
            z0 = s.b[1] * xin - s.a[0] * y + z1;
            z1 = s.b[2] * xin - s.a[1] * y;
            if !xin.is_finite() || !y.is_finite() || !z0.is_finite() || !z1.is_finite() {
                first_bad = i;
                let point = coordinate(i);
                first_fault = Some(first_fault.map_or(point, |old| old.min(point)));
                break;
            }
            *v = y;
        }
        *z = [z0, z1];
    }
    first_fault.map_or(Ok(()), Err)
}

fn finite(values: &[f64]) -> Result<(), usize> {
    values
        .iter()
        .position(|x| !x.is_finite())
        .map_or(Ok(()), Err)
}

fn run_with_seed(
    sections: &[Sos],
    ext: &mut [f64],
    unit: &[[f64; 2]],
    coordinate: impl Fn(usize) -> usize,
) -> Result<(), usize> {
    let mut zi: Vec<[f64; 2]> = unit
        .iter()
        .map(|z| [z[0] * ext[0], z[1] * ext[0]])
        .collect();
    sos_mapped(sections, ext, &mut zi, coordinate)
}

pub(crate) fn decimate(x: &[f64], q: usize) -> Result<Vec<f64>, usize> {
    let index = CHEBY_QS
        .iter()
        .position(|&c| c == q)
        .expect("fixed product decimation");
    let sections = &CHEBY_SOS[index];
    let nb2 = sections.iter().filter(|s| s.b[2] == 0.0).count();
    let na2 = sections.iter().filter(|s| s.a[1] == 0.0).count();
    let padlen = 3 * (2 * sections.len() + 1 - nb2.min(na2));
    let unit = sosfilt_zi(sections);
    let mut ext = odd_ext(x, padlen);
    finite(&ext).map_err(|i| logical_sample(i, x.len(), padlen, false))?;
    run_with_seed(sections, &mut ext, &unit, |i| {
        logical_sample(i, x.len(), padlen, false)
    })?;
    ext.reverse();
    run_with_seed(sections, &mut ext, &unit, |i| {
        logical_sample(i, x.len(), padlen, true)
    })?;
    ext.reverse();
    Ok(ext[padlen..ext.len() - padlen]
        .iter()
        .step_by(q)
        .copied()
        .collect())
}

// Index of the logical sample being processed, not a claim about which input
// caused the fault. Zero-phase filtering is noncausal. Padded positions are
// assigned to the nearest endpoint; reverse-pass positions are un-reversed.
fn logical_sample(index: usize, len: usize, pad: usize, reverse: bool) -> usize {
    let position = if reverse {
        len + 2 * pad - 1 - index
    } else {
        index
    };
    position.saturating_sub(pad).min(len - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_prefix_cutoff_is_separate_from_reverse_fault_order() {
        let sections = [
            Sos {
                b: [1.0, 0.0, 2.0],
                a: [0.0, 0.0],
            },
            Sos {
                b: [4.0, 0.0, 0.0],
                a: [0.0, 0.0],
            },
        ];
        let mut input = [0.0; 30];
        input[10] = f64::MAX / 2.0;
        input[20] = f64::MAX;
        let mut forward = input;
        assert_eq!(
            sos_mapped(&sections, &mut forward, &mut [[0.0; 2]; 2], |i| i),
            Err(10)
        );
        assert_eq!(
            sos_mapped(&sections, &mut input, &mut [[0.0; 2]; 2], |i| 29 - i),
            Err(9)
        );
    }

    #[test]
    fn padded_and_reverse_coordinates_share_original_sample_grid() {
        assert_eq!(logical_sample(0, 100, 9, false), 0);
        assert_eq!(logical_sample(117, 100, 9, false), 99);
        assert_eq!(logical_sample(16, 100, 9, false), 7);
        assert_eq!(logical_sample(117 - 16, 100, 9, true), 7);
        assert_eq!(logical_sample(0, 100, 9, true), 99);
        assert_eq!(logical_sample(117, 100, 9, true), 0);
    }

    #[test]
    fn hidden_delay_overflow_is_rejected_even_with_finite_output() {
        let section = Sos {
            b: [0.0, 2.0, 0.0],
            a: [0.0, 0.0],
        };
        let mut x = [f64::MAX];
        assert_eq!(sos(&[section], &mut x, &mut [[0.0; 2]]), Err(0));
    }

    #[test]
    fn checked_filters_preserve_normal_bits() {
        let x: Vec<_> = (0..4800)
            .map(|i| ((i % 71) as f64 - 35.0) * 0.001)
            .collect();
        for &q in &CHEBY_QS {
            let old = super::super::filtfilt::decimate(&x, q);
            let new = decimate(&x, q).unwrap();
            assert_eq!(
                old.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                new.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
    }
}
