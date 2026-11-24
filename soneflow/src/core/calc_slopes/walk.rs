use super::model::SlopeModel;
use super::positions::POSITIONS;
struct Piece {
    x: f64,
    end: f64,
    high: f64,
    low: f64,
    slope: f64,
}
impl Piece {
    #[inline(always)]
    fn sample(&self, z: f64) -> f64 {
        if z == self.end {
            return self.low;
        }
        (self.high - self.slope * (z - self.x)).clamp(self.low, self.high)
    }
    fn area(&self) -> f64 {
        (self.end - self.x) * ((self.high + self.low) * 0.5)
    }
}
#[derive(Default)]
struct Sum {
    value: f64,
}
impl Sum {
    fn add(&mut self, value: f64) {
        self.value += value;
    }
}
fn clean(v: f64) -> f64 {
    if v.to_bits() <= 0x7fef_ffff_ffff_ffff || v.to_bits() == 0x8000_0000_0000_0000 {
        v
    } else {
        0.0
    }
}
impl SlopeModel {
    #[inline(always)]
    pub(crate) fn walk<const SAMPLES: bool>(&self, cores: &[f64; 21], out: &mut [f64]) -> f64 {
        let mut sum = Sum::default();
        let mut cursor = 0;
        let mut x = 0.0;
        let mut n = clean(cores[0]);
        let mut hint = 0;
        for (i, core) in cores.iter().enumerate() {
            let core = clean(*core);
            let end = self.edges[i];
            let col = i.saturating_sub(1).min(7);
            let mut range = self.range(n, core, hint);
            while x < end {
                let piece = self.piece(x, end, n, core, range, col);
                sum.add(piece.area());
                if SAMPLES {
                    emit(&piece, out, &mut cursor, self.sample_ends[i], end);
                }
                x = piece.end;
                if n > core {
                    hint = range;
                }
                n = piece.low;
                range = (range + 1).min(17);
            }
        }
        sum.value
    }
    #[inline(always)]
    fn range(&self, n: f64, core: f64, hint: usize) -> usize {
        if n <= core || n > self.thresholds[0] {
            return 0;
        }
        if hint > 0 && n > self.thresholds[hint] && n <= self.thresholds[hint - 1] {
            return hint;
        }
        self.thresholds.iter().filter(|t| **t >= n).count().min(17)
    }
    #[inline(always)]
    fn piece(&self, x: f64, end: f64, n: f64, core: f64, r: usize, col: usize) -> Piece {
        if n <= core {
            return Piece {
                x,
                end,
                high: core,
                low: core,
                slope: 0.0,
            };
        }
        let target = core.max(self.thresholds[r]);
        let slope = self.slopes[r][col];
        let drop = slope * (end - x);
        if n - target <= drop {
            let distance = (n - target) / slope;
            Piece {
                x,
                end: (x + distance).min(end),
                high: n,
                low: target,
                slope,
            }
        } else {
            Piece {
                x,
                end,
                high: n,
                low: n - drop,
                slope,
            }
        }
    }
}
#[inline(always)]
fn emit(piece: &Piece, out: &mut [f64], cursor: &mut usize, band_end: usize, edge: f64) {
    if piece.slope == 0.0 {
        out[*cursor..band_end].fill(piece.low);
        *cursor = band_end;
        return;
    }
    if piece.end == edge {
        emit_terminal(piece, out, cursor, band_end);
        return;
    }
    while *cursor < band_end {
        let z = POSITIONS[*cursor];
        if z > piece.end {
            break;
        }
        out[*cursor] = piece.sample(z);
        *cursor += 1;
    }
}

#[inline(always)]
fn emit_terminal(piece: &Piece, out: &mut [f64], cursor: &mut usize, band_end: usize) {
    let endpoint = band_end > *cursor && POSITIONS[band_end - 1] == piece.end;
    let interior_end = band_end - usize::from(endpoint);
    for (slot, z) in out[*cursor..interior_end]
        .iter_mut()
        .zip(&POSITIONS[*cursor..interior_end])
    {
        *slot = (piece.high - piece.slope * (z - piece.x)).clamp(piece.low, piece.high);
    }
    if endpoint {
        out[interior_end] = piece.low;
    }
    *cursor = band_end;
}

impl SlopeModel {
    /// Returns area and index-derived samples. Invalid cores are zero (experiment policy).
    #[inline(always)]
    pub fn evaluate(&self, cores: &[f64; 21]) -> (f64, [f64; 240]) {
        let mut samples = [0.0; 240];
        let total = self.evaluate_into(cores, &mut samples);
        (total, samples)
    }
    /// Writes 240 samples without allocating. Panics if `samples.len() != 240`.
    #[inline(always)]
    pub fn evaluate_into(&self, cores: &[f64; 21], samples: &mut [f64]) -> f64 {
        assert_eq!(
            samples.len(),
            240,
            "sample destination must have length 240"
        );
        self.walk::<true>(cores, samples)
    }
    /// Returns the identical area without constructing or filling a sample array.
    #[inline(always)]
    pub fn total_only(&self, cores: &[f64; 21]) -> f64 {
        self.walk::<false>(cores, &mut [])
    }
}
