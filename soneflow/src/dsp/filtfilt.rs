#[cfg(any(test, feature = "internals"))]
use super::sos::{sosfilt_zi, sosfilt_zi_run, Sos};

/// Odd extension padding as scipy.signal._arraytools.odd_ext.
pub(super) fn odd_ext(x: &[f64], n: usize) -> Vec<f64> {
    assert!(x.len() > n, "padlen {n} >= signal length {}", x.len());
    let mut out = Vec::with_capacity(x.len() + 2 * n);
    let x0 = x[0];
    for i in (1..=n).rev() {
        out.push(2.0 * x0 - x[i]);
    }
    out.extend_from_slice(x);
    let xl = x[x.len() - 1];
    for i in 2..=(n + 1) {
        out.push(2.0 * xl - x[x.len() - i]);
    }
    out
}

/// scipy.signal.sosfiltfilt with defaults (padtype='odd', padlen=None).
#[cfg(any(test, feature = "internals"))]
pub fn sosfiltfilt(sections: &[Sos], x: &[f64]) -> Vec<f64> {
    let nb2 = sections.iter().filter(|s| s.b[2] == 0.0).count();
    let na2 = sections.iter().filter(|s| s.a[1] == 0.0).count();
    let padlen = 3 * (2 * sections.len() + 1 - nb2.min(na2));
    let zi_unit = sosfilt_zi(sections);

    let mut ext = odd_ext(x, padlen);
    let x0 = ext[0];
    let mut zi: Vec<[f64; 2]> = zi_unit.iter().map(|z| [z[0] * x0, z[1] * x0]).collect();
    sosfilt_zi_run(sections, &mut ext, &mut zi);

    ext.reverse();
    let y0 = ext[0];
    let mut zi: Vec<[f64; 2]> = zi_unit.iter().map(|z| [z[0] * y0, z[1] * y0]).collect();
    sosfilt_zi_run(sections, &mut ext, &mut zi);
    ext.reverse();
    ext[padlen..ext.len() - padlen].to_vec()
}

/// Zero-phase decimation by `q` with the given anti-alias filter: as
/// scipy.signal.decimate(x, q) when `sections` is its default Chebyshev-I
/// order-8 design for that `q`.
#[cfg(any(test, feature = "internals"))]
pub fn decimate_with_sos(sections: &[Sos], x: &[f64], q: usize) -> Vec<f64> {
    let y = sosfiltfilt(sections, x);
    y.iter().step_by(q).copied().collect()
}
