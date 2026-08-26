//! Integer terrain-density fields for authoritative surface generation.
//!
//! The field graph follows the same separation used by Minecraft's Overworld
//! density router: broad continental shape, erosion, ridges, and local detail
//! are sampled independently and then composed. Gradient selection and
//! quintic interpolation are informed by `OpenSimplex2`, upstream revision
//! `4cd120d35bfc27096698de90d1bcbf4f9d359a3b` (CC0-1.0), but this is a
//! project-owned fixed-point implementation so authoritative output does not
//! depend on platform floating-point behavior.

use crate::hashes::sample_hash_2d;

const UNIT: i64 = 1_024;
const DIAGONAL: i64 = 724;

const WARP_X_SALT: u64 = 0x3c79_ac49_2ba7_b653;
const WARP_Z_SALT: u64 = 0x1c69_b3f7_4ac4_ae35;
const CONTINENT_SALT: u64 = 0xd1b5_4a32_d192_ed03;
const EROSION_SALT: u64 = 0x94d0_49bb_1331_11eb;
const RIDGE_SALT: u64 = 0xbf58_476d_1ce4_e5b9;
const DETAIL_SALT: u64 = 0x9e37_79b9_7f4a_7c15;

/// Samples one bounded Minecraft-style surface-shape field in `-1024..=1024`.
pub(crate) fn terrain_shape(seed: u64, x: i64, z: i64, base_scale: u16) -> i64 {
    let scale = i64::from(base_scale.max(1));
    let warp_scale = scale.saturating_mul(8);
    let warp_amplitude = scale.saturating_mul(2);
    let warp_x = scale_fixed(
        gradient_noise(seed ^ WARP_X_SALT, x, z, warp_scale),
        warp_amplitude,
    );
    let warp_z = scale_fixed(
        gradient_noise(seed ^ WARP_Z_SALT, x, z, warp_scale),
        warp_amplitude,
    );
    let warped_x = x.saturating_add(warp_x);
    let warped_z = z.saturating_add(warp_z);

    let continental = fractal_noise(
        seed ^ CONTINENT_SALT,
        warped_x,
        warped_z,
        scale.saturating_mul(16),
        2,
    );
    let erosion = fractal_noise(
        seed ^ EROSION_SALT,
        warped_x,
        warped_z,
        scale.saturating_mul(8),
        2,
    );
    let ridge_source = gradient_noise(
        seed ^ RIDGE_SALT,
        warped_x,
        warped_z,
        scale.saturating_mul(4),
    );
    let detail = fractal_noise(
        seed ^ DETAIL_SALT,
        warped_x,
        warped_z,
        scale.saturating_mul(2),
        3,
    );

    let ridge = UNIT.saturating_sub(ridge_source.abs().min(UNIT));
    let ridge = multiply_fixed(ridge, ridge);
    let highland = continental.saturating_add(UNIT / 4).clamp(0, UNIT);
    let low_erosion = (UNIT / 4).saturating_sub(erosion).clamp(0, UNIT);
    let mountain_mask = multiply_fixed(highland, low_erosion);
    let peaks = multiply_fixed(ridge, mountain_mask);

    continental
        .saturating_mul(4)
        .saturating_add(detail.saturating_mul(2))
        .saturating_add(peaks.saturating_mul(16))
        .div_euclid(8)
        .clamp(-UNIT, UNIT)
}

fn fractal_noise(seed: u64, x: i64, z: i64, scale: i64, octaves: u8) -> i64 {
    let mut weighted = 0_i64;
    let mut total_weight = 0_i64;
    let mut octave_scale = scale.max(1);
    let mut weight = 1_i64 << octaves.saturating_sub(1);
    for octave in 0..octaves {
        weighted = weighted.saturating_add(
            gradient_noise(
                seed ^ u64::from(octave).wrapping_mul(DETAIL_SALT),
                x,
                z,
                octave_scale,
            )
            .saturating_mul(weight),
        );
        total_weight = total_weight.saturating_add(weight);
        octave_scale = octave_scale.saturating_div(2).max(1);
        weight = weight.saturating_div(2).max(1);
    }
    weighted.div_euclid(total_weight.max(1)).clamp(-UNIT, UNIT)
}

fn gradient_noise(seed: u64, x: i64, z: i64, scale: i64) -> i64 {
    let scale = scale.max(1);
    let cell_x = x.div_euclid(scale);
    let cell_z = z.div_euclid(scale);
    let local_x = x.rem_euclid(scale).saturating_mul(UNIT).div_euclid(scale);
    let local_z = z.rem_euclid(scale).saturating_mul(UNIT).div_euclid(scale);
    let fade_x = quintic_fade(local_x);
    let fade_z = quintic_fade(local_z);

    let south_west = gradient_dot(seed, cell_x, cell_z, local_x, local_z);
    let south_east = gradient_dot(
        seed,
        cell_x.saturating_add(1),
        cell_z,
        local_x.saturating_sub(UNIT),
        local_z,
    );
    let north_west = gradient_dot(
        seed,
        cell_x,
        cell_z.saturating_add(1),
        local_x,
        local_z.saturating_sub(UNIT),
    );
    let north_east = gradient_dot(
        seed,
        cell_x.saturating_add(1),
        cell_z.saturating_add(1),
        local_x.saturating_sub(UNIT),
        local_z.saturating_sub(UNIT),
    );
    let south = lerp_fixed(south_west, south_east, fade_x);
    let north = lerp_fixed(north_west, north_east, fade_x);
    lerp_fixed(south, north, fade_z).clamp(-UNIT, UNIT)
}

fn gradient_dot(seed: u64, cell_x: i64, cell_z: i64, dx: i64, dz: i64) -> i64 {
    let (gradient_x, gradient_z) = match sample_hash_2d(seed, cell_x, cell_z) & 7 {
        0 => (UNIT, 0),
        1 => (-UNIT, 0),
        2 => (0, UNIT),
        3 => (0, -UNIT),
        4 => (DIAGONAL, DIAGONAL),
        5 => (-DIAGONAL, DIAGONAL),
        6 => (DIAGONAL, -DIAGONAL),
        _ => (-DIAGONAL, -DIAGONAL),
    };
    gradient_x
        .saturating_mul(dx)
        .saturating_add(gradient_z.saturating_mul(dz))
        .div_euclid(UNIT)
}

fn quintic_fade(value: i64) -> i64 {
    let square = multiply_fixed(value, value);
    let cube = multiply_fixed(square, value);
    let fourth = multiply_fixed(cube, value);
    let fifth = multiply_fixed(fourth, value);
    fifth
        .saturating_mul(6)
        .saturating_sub(fourth.saturating_mul(15))
        .saturating_add(cube.saturating_mul(10))
        .clamp(0, UNIT)
}

fn multiply_fixed(left: i64, right: i64) -> i64 {
    left.saturating_mul(right).div_euclid(UNIT)
}

fn scale_fixed(value: i64, scale: i64) -> i64 {
    value.saturating_mul(scale).div_euclid(UNIT)
}

fn lerp_fixed(left: i64, right: i64, amount: i64) -> i64 {
    left.saturating_add(multiply_fixed(right.saturating_sub(left), amount))
}

#[cfg(test)]
mod tests {
    use super::{UNIT, gradient_noise, quintic_fade, terrain_shape};

    #[test]
    fn quintic_fade_has_exact_endpoints() {
        assert_eq!(quintic_fade(0), 0);
        assert_eq!(quintic_fade(UNIT), UNIT);
        assert_eq!(quintic_fade(UNIT / 2), UNIT / 2);
    }

    #[test]
    fn gradient_noise_is_continuous_across_signed_lattice_edges() {
        let seed = 0x5eed_5eed_d15c_a11e;
        for boundary in -8_i64..=8 {
            let x = boundary.saturating_mul(16);
            let left = gradient_noise(seed, x.saturating_sub(1), -7, 16);
            let center = gradient_noise(seed, x, -7, 16);
            let right = gradient_noise(seed, x.saturating_add(1), -7, 16);
            assert!((center - left).abs() <= 128, "left edge jump at {x}");
            assert!((right - center).abs() <= 128, "right edge jump at {x}");
        }
    }

    #[test]
    fn terrain_shape_is_bounded_and_varies_on_both_signed_axes() {
        let mut samples = std::collections::BTreeSet::new();
        let mut minimum = UNIT;
        let mut maximum = -UNIT;
        for z in -96_i64..=96 {
            for x in -96_i64..=96 {
                let sample = terrain_shape(42, x, z, 16);
                assert!((-UNIT..=UNIT).contains(&sample));
                minimum = minimum.min(sample);
                maximum = maximum.max(sample);
                samples.insert(sample);
            }
        }
        assert!(minimum <= -128, "terrain field lacks lowlands: {minimum}");
        assert!(maximum >= 512, "terrain field lacks highlands: {maximum}");
        assert!(
            samples.len() > 256,
            "multi-scale terrain must not quantize into a small set, got {} values",
            samples.len()
        );
    }

    #[test]
    fn highland_extremes_are_reachable_but_rare() {
        let mut broad_minimum = UNIT;
        let mut broad_maximum = -UNIT;
        let mut saturated = 0_u64;
        let mut broad_samples = 0_u64;
        for z in (-4_096_i64..=4_096).step_by(8) {
            for x in (-4_096_i64..=4_096).step_by(8) {
                let sample = terrain_shape(42, x, z, 32);
                broad_minimum = broad_minimum.min(sample);
                broad_maximum = broad_maximum.max(sample);
                saturated = saturated.saturating_add(u64::from(sample == UNIT));
                broad_samples = broad_samples.saturating_add(1);
            }
        }
        assert!(broad_minimum <= -320, "lowlands regressed: {broad_minimum}");
        assert_eq!(broad_maximum, UNIT, "the corpus must contain high peaks");
        assert!(
            saturated.saturating_mul(10_000) < broad_samples,
            "clamped peaks must stay below 0.01% of the sampled field"
        );
    }
}
