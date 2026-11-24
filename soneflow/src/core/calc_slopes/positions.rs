pub(crate) const POSITIONS: [f64; 240] = positions();
const fn positions() -> [f64; 240] {
    let mut result = [0.0; 240];
    let mut i = 0;
    while i < 240 {
        result[i] = (i + 1) as f64 / 10.0;
        i += 1;
    }
    result
}
