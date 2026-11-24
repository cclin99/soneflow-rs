#[cfg(test)]
use super::positions::POSITIONS;

/// Validated caller-supplied piecewise model. No standard tables are bundled.
pub struct SlopeModel {
    pub(crate) edges: [f64; 21],
    pub(crate) sample_ends: [usize; 21],
    pub(crate) thresholds: [f64; 18],
    pub(crate) slopes: [[f64; 8]; 18],
}
impl SlopeModel {
    /// Rejects nonfinite, unordered, nonpositive or out-of-domain model fields.
    #[cfg(test)]
    pub fn new(
        edges: [f64; 21],
        thresholds: [f64; 18],
        slopes: [[f64; 8]; 18],
        extension: f64,
    ) -> Result<Self, &'static str> {
        validate_edges(&edges, extension)?;
        validate_thresholds(&thresholds)?;
        if slopes.iter().flatten().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err("invalid steepness");
        }
        let edges = edges.map(|e| e + extension);
        if edges.windows(2).any(|w| w[0] >= w[1]) {
            return Err("collapsed effective edges");
        }
        let sample_ends = std::array::from_fn(|i| POSITIONS.partition_point(|z| *z <= edges[i]));
        Ok(Self {
            edges,
            sample_ends,
            thresholds,
            slopes,
        })
    }
}
#[cfg(test)]
fn validate_edges(edges: &[f64; 21], extension: f64) -> Result<(), &'static str> {
    if !extension.is_finite() || !(0.0..=0.001).contains(&extension) {
        return Err("invalid extension");
    }
    if edges.iter().any(|e| !e.is_finite()) {
        return Err("nonfinite edge");
    }
    if edges[0] <= 0.0 || edges[20] != 24.0 {
        return Err("invalid endpoint");
    }
    if edges.windows(2).any(|w| w[0] >= w[1]) {
        return Err("unordered edges");
    }
    Ok(())
}
#[cfg(test)]
fn validate_thresholds(t: &[f64; 18]) -> Result<(), &'static str> {
    if t.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite threshold");
    }
    if t[17] != 0.0 || t.windows(2).any(|w| w[0] <= w[1]) {
        return Err("unordered thresholds");
    }
    Ok(())
}
