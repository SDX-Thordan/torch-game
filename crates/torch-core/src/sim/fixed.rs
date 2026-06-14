//! Deterministic fixed-point trig (§27: integer math, no floats in the sim).
//!
//! The orbital model needs sine/cosine, but floats would risk cross-platform
//! divergence. We use Bhaskara I's integer approximation (max error ~0.2%),
//! which is plenty for a stub orrery and is bit-for-bit identical everywhere.

/// Q16 fixed-point unit: `1.0` is represented as `65536`.
pub const Q16_ONE: i64 = 65_536;

const MDEG_HALF: i64 = 180_000;
const MDEG_FULL: i64 = 360_000;
/// Bhaskara's constant `40500`, scaled to milli-degree² units (× 1e6).
const BHASKARA_K: i64 = 40_500_000_000;

/// Sine of an angle given in milli-degrees, returned in Q16 (≈ `[-65536, 65536]`).
pub fn sin_q16(mdeg: i64) -> i64 {
    let mut a = mdeg % MDEG_FULL;
    if a < 0 {
        a += MDEG_FULL;
    }
    // Reflect the upper half-turn onto [0, 180000] and negate.
    let (p, sign) = if a <= MDEG_HALF {
        (a, 1)
    } else {
        (a - MDEG_HALF, -1)
    };
    let pp = p * (MDEG_HALF - p);
    let num = 4 * pp * Q16_ONE;
    let den = BHASKARA_K - pp;
    sign * (num / den)
}

/// Cosine of an angle given in milli-degrees, returned in Q16.
pub fn cos_q16(mdeg: i64) -> i64 {
    sin_q16(mdeg + 90_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinal_points() {
        assert_eq!(sin_q16(0), 0);
        assert!((sin_q16(90_000) - Q16_ONE).abs() < 50); // ~1.0
        assert!(sin_q16(180_000).abs() < 50); // ~0.0
        assert!((sin_q16(270_000) + Q16_ONE).abs() < 50); // ~-1.0
        assert!((cos_q16(0) - Q16_ONE).abs() < 50); // ~1.0
    }

    #[test]
    fn negative_and_wrapped_angles_match() {
        for mdeg in (0..360_000).step_by(1234) {
            assert_eq!(sin_q16(mdeg), sin_q16(mdeg + 360_000));
            assert_eq!(sin_q16(mdeg), sin_q16(mdeg - 360_000));
        }
    }

    #[test]
    fn pythagorean_identity_holds() {
        // sin² + cos² ≈ 1 (within Bhaskara's error budget) at every angle.
        let one_sq = Q16_ONE * Q16_ONE;
        for mdeg in (0..360_000).step_by(777) {
            let s = sin_q16(mdeg);
            let c = cos_q16(mdeg);
            let sum = s * s + c * c;
            let err = (sum - one_sq).abs();
            assert!(err * 100 < one_sq * 2, "angle {mdeg}: off by {err}"); // < 2%
        }
    }
}
