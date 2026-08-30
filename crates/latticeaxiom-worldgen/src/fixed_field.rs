//! Safe fixed-point `OpenSimplex2` field candidates.
//!
//! The topology, hash constants, and gradient sets are independently adapted
//! from K.jpg's CC0 `OpenSimplex2` reference revision
//! `4cd120d35bfc27096698de90d1bcbf4f9d359a3b`. Authoritative evaluation uses
//! no floating point, mutable global table, SIMD-width choice, or unsafe code.
//! This module is conformance infrastructure only; the existing terrain epoch
//! does not consume it.
//!
//! Inputs use a split integer plus Q0.32 fraction. Transformed offsets and
//! outputs use signed Q30. Every multiplication reduces by round-to-nearest,
//! with exact half cases rounded away from zero. Phase conversion after a
//! Euclidean lattice floor drops the two least-significant bits toward zero so
//! the fractional coordinate remains in `[0, 1)`. Lattice hashing deliberately
//! uses two's-complement wrapping multiplication, matching the reference.
//!
//! The supported integer envelope is smaller than `i64` by design. At the
//! inclusive bound, an input raw value is below `2^79`; the largest three-axis
//! transform product is below `2^114`, and attenuation/gradient products are
//! below `2^100`. All fit signed `i128` with margin. The bound covers the full
//! `i32` chunk-coordinate range at the frozen 32-voxel chunk edge.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::{WorldgenError, WorldgenResult};

/// Fraction bits in the split authoritative input coordinate.
pub const FIXED_FIELD_FRACTION_BITS_V1: u32 = 32;
/// Fraction bits in transformed arithmetic and returned samples.
pub const FIXED_FIELD_OUTPUT_BITS_V1: u32 = 30;
/// Inclusive integer-coordinate magnitude accepted by the v1 field candidate.
pub const MAX_FIXED_FIELD_INTEGER_COORDINATE_V1: i64 = (1_i64 << 47) - 1;

const Q32_ONE: i128 = 1_i128 << FIXED_FIELD_FRACTION_BITS_V1;
const Q32_HALF: i128 = Q32_ONE / 2;
const Q30_ONE: i128 = 1_i128 << FIXED_FIELD_OUTPUT_BITS_V1;
const Q30_HALF: i128 = Q30_ONE / 2;

const PRIME_X: i64 = 0x5205_402B_9270_C86F;
const PRIME_Y: i64 = 0x598C_D327_0038_17B5;
const PRIME_Z: i64 = 0x5BCC_226E_9FA0_BACB;
const HASH_MULTIPLIER: i64 = 0x53A3_F72D_EEC5_46F5;
const SEED_FLIP_3D: i64 = -0x52D5_47B2_E96E_D629;

const SKEW_2D_Q32: i128 = 1_572_067_139;
const UNSKEW_2D_Q30: i128 = -226_908_346;
const UNSKEW_2D_Q32: i128 = -907_633_386;
const ROOT3_OVER3_Q32: i128 = 2_479_700_525;
const NEG_ROOT3_OVER3_Q32: i128 = -2_479_700_525;
const RSQUARED_2D_Q30: i128 = 715_827_883;
const RSQUARED_3D_Q30: i128 = 644_245_094;
const THREE_QUARTERS_Q30: i128 = 805_306_368;
const ONE_PLUS_TWO_UNSKEW_Q30: i128 = 619_925_131;
const UNSKEW_PLUS_ONE_Q30: i128 = 846_833_478;
const THREE_UNSKEW_PLUS_ONE_Q30: i128 = 393_016_785;
const THREE_UNSKEW_PLUS_TWO_Q30: i128 = 1_466_758_609;
const A1_T_COEFFICIENT_Q30: i128 = -3_387_333_910;
const A1_BASE_Q30: i128 = -715_827_883;

/// Frozen authoritative field algorithm identities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthoritativeFieldAlgorithmV1 {
    /// Safe fixed-point 2D `OpenSimplex2S` candidate, revision 1.
    OpenSimplex2s2dRevision1,
    /// Safe fixed-point 3D `OpenSimplex2F` candidate, revision 1.
    OpenSimplex2f3dRevision1,
}

impl AuthoritativeFieldAlgorithmV1 {
    /// Returns the stable algorithm identity used in provenance and cache keys.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenSimplex2s2dRevision1 => "latticeaxiom:field/open-simplex-2s-2d-fixed@1",
            Self::OpenSimplex2f3dRevision1 => "latticeaxiom:field/open-simplex-2f-3d-fixed@1",
        }
    }
}

/// One coordinate represented as an integer lattice part and Q0.32 fraction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixedCoordinateV1 {
    integer: i64,
    fraction_q32: u32,
}

impl FixedCoordinateV1 {
    /// Creates a validated split coordinate.
    ///
    /// Every `u32` fraction is canonical and denotes `fraction / 2^32`.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfig`] when the integer part exceeds
    /// [`MAX_FIXED_FIELD_INTEGER_COORDINATE_V1`].
    pub fn new(integer: i64, fraction_q32: u32) -> WorldgenResult<Self> {
        validate_integer(integer)?;
        Ok(Self {
            integer,
            fraction_q32,
        })
    }

    /// Converts `voxel / scale` using Euclidean division for negative voxels.
    ///
    /// Fraction quantization rounds toward the lower lattice point.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfig`] when the resulting integer part
    /// exceeds the supported coordinate envelope.
    pub fn from_voxel(voxel: i64, scale: NonZeroU32) -> WorldgenResult<Self> {
        let scale_i64 = i64::from(scale.get());
        let integer = voxel.div_euclid(scale_i64);
        validate_integer(integer)?;
        let remainder = voxel.rem_euclid(scale_i64);
        let numerator = u128::try_from(remainder)
            .unwrap_or_default()
            .saturating_mul(1_u128 << FIXED_FIELD_FRACTION_BITS_V1);
        let fraction = numerator / u128::from(scale.get());
        let fraction_q32 = u32::try_from(fraction).unwrap_or(u32::MAX);
        Ok(Self {
            integer,
            fraction_q32,
        })
    }

    /// Returns the signed integer lattice part.
    #[must_use]
    pub const fn integer(self) -> i64 {
        self.integer
    }

    /// Returns the exact Q0.32 fractional numerator.
    #[must_use]
    pub const fn fraction_q32(self) -> u32 {
        self.fraction_q32
    }

    fn raw_q32(self) -> i128 {
        i128::from(self.integer) * Q32_ONE + i128::from(self.fraction_q32)
    }
}

/// Signed Q2.30 authoritative field result, clamped to `[-1, 1]`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FixedFieldSampleV1(i32);

impl FixedFieldSampleV1 {
    /// Exact zero sample.
    pub const ZERO: Self = Self(0);

    /// Returns the signed Q30 numerator.
    #[must_use]
    pub const fn raw_q30(self) -> i32 {
        self.0
    }

    /// Returns a deterministic parts-per-million projection.
    #[must_use]
    pub fn parts_per_million(self) -> i32 {
        let scaled = round_divide(i128::from(self.0) * 1_000_000, Q30_ONE);
        match i32::try_from(scaled) {
            Ok(value) => value,
            Err(_) if scaled.is_negative() => -1_000_000,
            Err(_) => 1_000_000,
        }
    }

    fn from_accumulator(value: i128) -> Self {
        let clamped = value.clamp(-Q30_ONE, Q30_ONE);
        match i32::try_from(clamped) {
            Ok(raw) => Self(raw),
            Err(_) if clamped.is_negative() => Self(-(1_i32 << FIXED_FIELD_OUTPUT_BITS_V1)),
            Err(_) => Self(1_i32 << FIXED_FIELD_OUTPUT_BITS_V1),
        }
    }
}

/// Samples safe fixed-point 2D `OpenSimplex2S` in its standard orientation.
///
/// # Errors
///
/// Returns [`WorldgenError::ArithmeticOverflow`] only if a validated coordinate
/// violates the documented interval proof.
pub fn open_simplex_2s_2d_v1(
    seed: i64,
    x: FixedCoordinateV1,
    z: FixedCoordinateV1,
) -> WorldgenResult<FixedFieldSampleV1> {
    let x_raw = x.raw_q32();
    let z_raw = z.raw_q32();
    let skew = multiply_shift(x_raw + z_raw, SKEW_2D_Q32, FIXED_FIELD_FRACTION_BITS_V1);
    open_simplex_2s_unskewed(seed, x_raw + skew, z_raw + skew)
}

/// Samples safe fixed-point 3D `OpenSimplex2F` with the XZ plane improved for a
/// Y-up world.
///
/// # Errors
///
/// Returns [`WorldgenError::ArithmeticOverflow`] only if a validated coordinate
/// violates the documented interval proof.
pub fn open_simplex_2f_3d_v1(
    seed: i64,
    x: FixedCoordinateV1,
    y: FixedCoordinateV1,
    z: FixedCoordinateV1,
) -> WorldgenResult<FixedFieldSampleV1> {
    let x_raw = x.raw_q32();
    let y_raw = y.raw_q32();
    let z_raw = z.raw_q32();
    let xz = x_raw + z_raw;
    let s2 = multiply_shift(xz, UNSKEW_2D_Q32, FIXED_FIELD_FRACTION_BITS_V1);
    let yy = multiply_shift(y_raw, ROOT3_OVER3_Q32, FIXED_FIELD_FRACTION_BITS_V1);
    let xr = x_raw + s2 + yy;
    let zr = z_raw + s2 + yy;
    let yr = multiply_shift(xz, NEG_ROOT3_OVER3_Q32, FIXED_FIELD_FRACTION_BITS_V1) + yy;
    open_simplex_2f_unrotated(seed, xr, yr, zr)
}

#[allow(
    clippy::too_many_lines,
    reason = "the branch structure is the frozen OpenSimplex2S vertex-selection contract"
)]
fn open_simplex_2s_unskewed(seed: i64, xs: i128, ys: i128) -> WorldgenResult<FixedFieldSampleV1> {
    let (xsb, xi) = lattice_floor(xs)?;
    let (ysb, yi) = lattice_floor(ys)?;
    let xsbp = xsb.wrapping_mul(PRIME_X);
    let ysbp = ysb.wrapping_mul(PRIME_Y);
    let t = multiply_q30(xi + yi, UNSKEW_2D_Q30);
    let dx0 = xi + t;
    let dy0 = yi + t;
    let a0 = RSQUARED_2D_Q30 - square_q30(dx0) - square_q30(dy0);
    let mut value = contribution2(seed, xsbp, ysbp, dx0, dy0, a0);

    let a1 = multiply_q30(A1_T_COEFFICIENT_Q30, t) + A1_BASE_Q30 + a0;
    let dx1 = dx0 - ONE_PLUS_TWO_UNSKEW_Q30;
    let dy1 = dy0 - ONE_PLUS_TWO_UNSKEW_Q30;
    value += contribution2(
        seed,
        xsbp.wrapping_add(PRIME_X),
        ysbp.wrapping_add(PRIME_Y),
        dx1,
        dy1,
        a1,
    );

    let x_minus_y = xi - yi;
    if t < UNSKEW_2D_Q30 {
        if xi + x_minus_y > Q30_ONE {
            add_vertex2(
                &mut value,
                seed,
                xsbp.wrapping_add(PRIME_X.wrapping_mul(2)),
                ysbp.wrapping_add(PRIME_Y),
                dx0 - THREE_UNSKEW_PLUS_TWO_Q30,
                dy0 - THREE_UNSKEW_PLUS_ONE_Q30,
            );
        } else {
            add_vertex2(
                &mut value,
                seed,
                xsbp,
                ysbp.wrapping_add(PRIME_Y),
                dx0 - UNSKEW_2D_Q30,
                dy0 - UNSKEW_PLUS_ONE_Q30,
            );
        }
        if yi - x_minus_y > Q30_ONE {
            add_vertex2(
                &mut value,
                seed,
                xsbp.wrapping_add(PRIME_X),
                ysbp.wrapping_add(PRIME_Y.wrapping_mul(2)),
                dx0 - THREE_UNSKEW_PLUS_ONE_Q30,
                dy0 - THREE_UNSKEW_PLUS_TWO_Q30,
            );
        } else {
            add_vertex2(
                &mut value,
                seed,
                xsbp.wrapping_add(PRIME_X),
                ysbp,
                dx0 - UNSKEW_PLUS_ONE_Q30,
                dy0 - UNSKEW_2D_Q30,
            );
        }
    } else {
        if xi + x_minus_y < 0 {
            add_vertex2(
                &mut value,
                seed,
                xsbp.wrapping_sub(PRIME_X),
                ysbp,
                dx0 + UNSKEW_PLUS_ONE_Q30,
                dy0 + UNSKEW_2D_Q30,
            );
        } else {
            add_vertex2(
                &mut value,
                seed,
                xsbp.wrapping_add(PRIME_X),
                ysbp,
                dx0 - UNSKEW_PLUS_ONE_Q30,
                dy0 - UNSKEW_2D_Q30,
            );
        }
        if yi < x_minus_y {
            add_vertex2(
                &mut value,
                seed,
                xsbp,
                ysbp.wrapping_sub(PRIME_Y),
                dx0 + UNSKEW_2D_Q30,
                dy0 + UNSKEW_PLUS_ONE_Q30,
            );
        } else {
            add_vertex2(
                &mut value,
                seed,
                xsbp,
                ysbp.wrapping_add(PRIME_Y),
                dx0 - UNSKEW_2D_Q30,
                dy0 - UNSKEW_PLUS_ONE_Q30,
            );
        }
    }
    Ok(FixedFieldSampleV1::from_accumulator(value))
}

fn open_simplex_2f_unrotated(
    seed: i64,
    xr: i128,
    yr: i128,
    zr: i128,
) -> WorldgenResult<FixedFieldSampleV1> {
    let xrb = fast_round_q32(xr)?;
    let yrb = fast_round_q32(yr)?;
    let zrb = fast_round_q32(zr)?;
    let mut xri = q32_offset_to_q30(xr - i128::from(xrb) * Q32_ONE);
    let mut yri = q32_offset_to_q30(yr - i128::from(yrb) * Q32_ONE);
    let mut zri = q32_offset_to_q30(zr - i128::from(zrb) * Q32_ONE);
    let mut x_sign = if xri >= 0 { -1_i64 } else { 1_i64 };
    let mut y_sign = if yri >= 0 { -1_i64 } else { 1_i64 };
    let mut z_sign = if zri >= 0 { -1_i64 } else { 1_i64 };
    let mut ax0 = i128::from(x_sign) * -xri;
    let mut ay0 = i128::from(y_sign) * -yri;
    let mut az0 = i128::from(z_sign) * -zri;
    let mut xrbp = xrb.wrapping_mul(PRIME_X);
    let mut yrbp = yrb.wrapping_mul(PRIME_Y);
    let mut zrbp = zrb.wrapping_mul(PRIME_Z);
    let mut lattice_seed = seed;
    let mut value = 0_i128;
    let mut attenuation = RSQUARED_3D_Q30 - square_q30(xri) - square_q30(yri) - square_q30(zri);

    for lattice in 0..2 {
        value += contribution3(lattice_seed, xrbp, yrbp, zrbp, xri, yri, zri, attenuation);
        if ax0 >= ay0 && ax0 >= az0 {
            let b = attenuation + ax0 + ax0 - Q30_ONE;
            if b > 0 {
                value += contribution3(
                    lattice_seed,
                    xrbp.wrapping_sub(x_sign.wrapping_mul(PRIME_X)),
                    yrbp,
                    zrbp,
                    xri + i128::from(x_sign) * Q30_ONE,
                    yri,
                    zri,
                    b,
                );
            }
        } else if ay0 > ax0 && ay0 >= az0 {
            let b = attenuation + ay0 + ay0 - Q30_ONE;
            if b > 0 {
                value += contribution3(
                    lattice_seed,
                    xrbp,
                    yrbp.wrapping_sub(y_sign.wrapping_mul(PRIME_Y)),
                    zrbp,
                    xri,
                    yri + i128::from(y_sign) * Q30_ONE,
                    zri,
                    b,
                );
            }
        } else {
            let b = attenuation + az0 + az0 - Q30_ONE;
            if b > 0 {
                value += contribution3(
                    lattice_seed,
                    xrbp,
                    yrbp,
                    zrbp.wrapping_sub(z_sign.wrapping_mul(PRIME_Z)),
                    xri,
                    yri,
                    zri + i128::from(z_sign) * Q30_ONE,
                    b,
                );
            }
        }
        if lattice == 1 {
            break;
        }
        ax0 = Q30_HALF - ax0;
        ay0 = Q30_HALF - ay0;
        az0 = Q30_HALF - az0;
        xri = i128::from(x_sign) * ax0;
        yri = i128::from(y_sign) * ay0;
        zri = i128::from(z_sign) * az0;
        attenuation += (THREE_QUARTERS_Q30 - ax0) - (ay0 + az0);
        xrbp = xrbp.wrapping_add((x_sign >> 1) & PRIME_X);
        yrbp = yrbp.wrapping_add((y_sign >> 1) & PRIME_Y);
        zrbp = zrbp.wrapping_add((z_sign >> 1) & PRIME_Z);
        x_sign = -x_sign;
        y_sign = -y_sign;
        z_sign = -z_sign;
        lattice_seed ^= SEED_FLIP_3D;
    }
    Ok(FixedFieldSampleV1::from_accumulator(value))
}

fn add_vertex2(value: &mut i128, seed: i64, xsvp: i64, ysvp: i64, dx: i128, dy: i128) {
    let attenuation = RSQUARED_2D_Q30 - square_q30(dx) - square_q30(dy);
    if attenuation > 0 {
        *value += contribution2(seed, xsvp, ysvp, dx, dy, attenuation);
    }
}

fn contribution2(seed: i64, xsvp: i64, ysvp: i64, dx: i128, dy: i128, attenuation: i128) -> i128 {
    if attenuation <= 0 {
        return 0;
    }
    let attenuation_squared = square_q30(attenuation);
    let attenuation_fourth = square_q30(attenuation_squared);
    multiply_q30(attenuation_fourth, gradient2(seed, xsvp, ysvp, dx, dy))
}

#[allow(
    clippy::too_many_arguments,
    reason = "one lattice contribution keeps its hash coordinate, displacement, and attenuation explicit"
)]
fn contribution3(
    seed: i64,
    xrvp: i64,
    yrvp: i64,
    zrvp: i64,
    dx: i128,
    dy: i128,
    dz: i128,
    attenuation: i128,
) -> i128 {
    if attenuation <= 0 {
        return 0;
    }
    let attenuation_squared = square_q30(attenuation);
    let attenuation_fourth = square_q30(attenuation_squared);
    multiply_q30(
        attenuation_fourth,
        gradient3(seed, xrvp, yrvp, zrvp, dx, dy, dz),
    )
}

fn gradient2(seed: i64, xsvp: i64, ysvp: i64, dx: i128, dy: i128) -> i128 {
    let mut hash = (seed ^ xsvp ^ ysvp).wrapping_mul(HASH_MULTIPLIER);
    hash ^= hash >> 58;
    let bits = u64::from_le_bytes(hash.to_le_bytes());
    let vector = usize::try_from(((bits & 0x00fe) >> 1) % 24).unwrap_or_default();
    let (gx, gy) = GRADIENTS_2D_Q30[vector];
    multiply_q30(i128::from(gx), dx) + multiply_q30(i128::from(gy), dy)
}

fn gradient3(seed: i64, xrvp: i64, yrvp: i64, zrvp: i64, dx: i128, dy: i128, dz: i128) -> i128 {
    let mut hash = (seed ^ xrvp ^ yrvp ^ zrvp).wrapping_mul(HASH_MULTIPLIER);
    hash ^= hash >> 58;
    let bits = u64::from_le_bytes(hash.to_le_bytes());
    let vector = usize::try_from(((bits & 0x03fc) >> 2) % 48).unwrap_or_default();
    let (gx, gy, gz) = GRADIENTS_3D_Q30[vector];
    multiply_q30(i128::from(gx), dx)
        + multiply_q30(i128::from(gy), dy)
        + multiply_q30(i128::from(gz), dz)
}

fn lattice_floor(raw_q32: i128) -> WorldgenResult<(i64, i128)> {
    let cell_i128 = raw_q32.div_euclid(Q32_ONE);
    let cell = i64::try_from(cell_i128).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "fixed field lattice floor",
    })?;
    let fraction_q32 = raw_q32.rem_euclid(Q32_ONE);
    Ok((cell, fraction_q32 >> 2))
}

fn fast_round_q32(raw_q32: i128) -> WorldgenResult<i64> {
    let rounded = if raw_q32 < 0 {
        (raw_q32 - Q32_HALF) / Q32_ONE
    } else {
        (raw_q32 + Q32_HALF) / Q32_ONE
    };
    i64::try_from(rounded).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "fixed field nearest lattice point",
    })
}

fn q32_offset_to_q30(raw_q32: i128) -> i128 {
    round_shift(raw_q32, 2)
}

fn square_q30(value: i128) -> i128 {
    multiply_q30(value, value)
}

fn multiply_q30(left: i128, right: i128) -> i128 {
    multiply_shift(left, right, FIXED_FIELD_OUTPUT_BITS_V1)
}

fn multiply_shift(left: i128, right: i128, bits: u32) -> i128 {
    round_shift(left * right, bits)
}

fn round_shift(value: i128, bits: u32) -> i128 {
    let half = 1_i128 << bits.saturating_sub(1);
    if value < 0 {
        -((-value + half) >> bits)
    } else {
        (value + half) >> bits
    }
}

fn round_divide(numerator: i128, denominator: i128) -> i128 {
    let half = denominator / 2;
    if numerator < 0 {
        -((-numerator + half) / denominator)
    } else {
        (numerator + half) / denominator
    }
}

fn validate_integer(integer: i64) -> WorldgenResult<()> {
    if integer.unsigned_abs() > MAX_FIXED_FIELD_INTEGER_COORDINATE_V1 as u64 {
        Err(WorldgenError::InvalidConfig {
            field: "fixed_field.integer_coordinate",
            reason: format!(
                "magnitude {} exceeds {}",
                integer.unsigned_abs(),
                MAX_FIXED_FIELD_INTEGER_COORDINATE_V1
            ),
        })
    } else {
        Ok(())
    }
}

#[rustfmt::skip]
const GRADIENTS_2D_Q30: [(i64, i64); 24] = [
    ( 7_495_680_659,  18_096_173_907),
    (18_096_173_907,   7_495_680_659),
    (18_096_173_907,  -7_495_680_659),
    ( 7_495_680_659, -18_096_173_907),
    (-7_495_680_659, -18_096_173_907),
    (-18_096_173_907, -7_495_680_659),
    (-18_096_173_907,  7_495_680_659),
    (-7_495_680_659,  18_096_173_907),
    ( 2_556_637_084,  19_419_586_644),
    (11_923_905_985,  15_539_536_823),
    (15_539_536_823,  11_923_905_985),
    (19_419_586_644,   2_556_637_084),
    (19_419_586_644,  -2_556_637_084),
    (15_539_536_823, -11_923_905_985),
    (11_923_905_985, -15_539_536_823),
    ( 2_556_637_084, -19_419_586_644),
    (-2_556_637_084, -19_419_586_644),
    (-11_923_905_985, -15_539_536_823),
    (-15_539_536_823, -11_923_905_985),
    (-19_419_586_644,  -2_556_637_084),
    (-19_419_586_644,   2_556_637_084),
    (-15_539_536_823,  11_923_905_985),
    (-11_923_905_985,  15_539_536_823),
    (-2_556_637_084,  19_419_586_644),
];

#[rustfmt::skip]
const GRADIENTS_3D_Q30: [(i64, i64, i64); 48] = [
    ( 29_973_027_248,  29_973_027_248, -13_472_568_308),
    ( 29_973_027_248,  29_973_027_248,  13_472_568_308),
    ( 41_579_935_818,  15_791_889_026,               0),
    ( 15_791_889_026,  41_579_935_818,               0),
    (-29_973_027_248,  29_973_027_248, -13_472_568_308),
    (-29_973_027_248,  29_973_027_248,  13_472_568_308),
    (-15_791_889_026,  41_579_935_818,               0),
    (-41_579_935_818,  15_791_889_026,               0),
    (-13_472_568_308, -29_973_027_248, -29_973_027_248),
    ( 13_472_568_308, -29_973_027_248, -29_973_027_248),
    (              0, -41_579_935_818, -15_791_889_026),
    (              0, -15_791_889_026, -41_579_935_818),
    (-13_472_568_308, -29_973_027_248,  29_973_027_248),
    ( 13_472_568_308, -29_973_027_248,  29_973_027_248),
    (              0, -15_791_889_026,  41_579_935_818),
    (              0, -41_579_935_818,  15_791_889_026),
    (-29_973_027_248, -29_973_027_248, -13_472_568_308),
    (-29_973_027_248, -29_973_027_248,  13_472_568_308),
    (-41_579_935_818, -15_791_889_026,               0),
    (-15_791_889_026, -41_579_935_818,               0),
    (-29_973_027_248, -13_472_568_308, -29_973_027_248),
    (-29_973_027_248,  13_472_568_308, -29_973_027_248),
    (-15_791_889_026,               0, -41_579_935_818),
    (-41_579_935_818,               0, -15_791_889_026),
    (-29_973_027_248, -13_472_568_308,  29_973_027_248),
    (-29_973_027_248,  13_472_568_308,  29_973_027_248),
    (-41_579_935_818,               0,  15_791_889_026),
    (-15_791_889_026,               0,  41_579_935_818),
    (-13_472_568_308,  29_973_027_248, -29_973_027_248),
    ( 13_472_568_308,  29_973_027_248, -29_973_027_248),
    (              0,  15_791_889_026, -41_579_935_818),
    (              0,  41_579_935_818, -15_791_889_026),
    (-13_472_568_308,  29_973_027_248,  29_973_027_248),
    ( 13_472_568_308,  29_973_027_248,  29_973_027_248),
    (              0,  41_579_935_818,  15_791_889_026),
    (              0,  15_791_889_026,  41_579_935_818),
    ( 29_973_027_248, -29_973_027_248, -13_472_568_308),
    ( 29_973_027_248, -29_973_027_248,  13_472_568_308),
    ( 15_791_889_026, -41_579_935_818,               0),
    ( 41_579_935_818, -15_791_889_026,               0),
    ( 29_973_027_248, -13_472_568_308, -29_973_027_248),
    ( 29_973_027_248,  13_472_568_308, -29_973_027_248),
    ( 41_579_935_818,               0, -15_791_889_026),
    ( 15_791_889_026,               0, -41_579_935_818),
    ( 29_973_027_248, -13_472_568_308,  29_973_027_248),
    ( 29_973_027_248,  13_472_568_308,  29_973_027_248),
    ( 15_791_889_026,               0,  41_579_935_818),
    ( 41_579_935_818,               0,  15_791_889_026),
];

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const ORACLE_SEED: i64 = i64::from_le_bytes(0x1234_5678_9abc_def0_u64.to_le_bytes());

    fn coordinate(integer: i64, fraction_q32: u32) -> FixedCoordinateV1 {
        FixedCoordinateV1::new(integer, fraction_q32)
            .unwrap_or_else(|error| panic!("fixture coordinate is valid: {error}"))
    }

    fn sample_as_f64(sample: FixedFieldSampleV1) -> f64 {
        f64::from(sample.raw_q30()) / f64::from(1_u32 << FIXED_FIELD_OUTPUT_BITS_V1)
    }

    #[test]
    fn split_coordinates_use_euclidean_negative_division_and_hard_bounds() {
        let scale = NonZeroU32::new(32).unwrap_or_else(|| panic!("scale is nonzero"));
        let negative = FixedCoordinateV1::from_voxel(-1, scale)
            .unwrap_or_else(|error| panic!("negative coordinate converts: {error}"));
        assert_eq!(negative.integer(), -1);
        assert_eq!(negative.fraction_q32(), 0xf800_0000);
        assert_eq!(
            FixedCoordinateV1::from_voxel(-33, scale)
                .unwrap_or_else(|error| panic!("negative coordinate converts: {error}")),
            coordinate(-2, 0xf800_0000)
        );
        assert!(FixedCoordinateV1::new(MAX_FIXED_FIELD_INTEGER_COORDINATE_V1, u32::MAX).is_ok());
        assert!(FixedCoordinateV1::new(-MAX_FIXED_FIELD_INTEGER_COORDINATE_V1, 0).is_ok());
        assert!(FixedCoordinateV1::new(MAX_FIXED_FIELD_INTEGER_COORDINATE_V1 + 1, 0).is_err());
        assert!(FixedCoordinateV1::new(-MAX_FIXED_FIELD_INTEGER_COORDINATE_V1 - 1, 0).is_err());
    }

    #[test]
    fn fixed_field_algorithm_identities_are_frozen() {
        assert_eq!(
            AuthoritativeFieldAlgorithmV1::OpenSimplex2s2dRevision1.as_str(),
            "latticeaxiom:field/open-simplex-2s-2d-fixed@1"
        );
        assert_eq!(
            AuthoritativeFieldAlgorithmV1::OpenSimplex2f3dRevision1.as_str(),
            "latticeaxiom:field/open-simplex-2f-3d-fixed@1"
        );
    }

    #[test]
    fn upstream_known_answer_vectors_match_the_fixed_revision() {
        let cases_2d = [
            (0_i64, coordinate(0, 0), coordinate(0, 0), 0_i32),
            (
                42,
                coordinate(0, 0x4000_0000),
                coordinate(-1, 0x4000_0000),
                -80_037_978,
            ),
            (
                -1,
                coordinate(12_345, 0x2000_0000),
                coordinate(-9_877, 0x8000_0000),
                -790_371_194,
            ),
            (
                ORACLE_SEED,
                coordinate(-32, 1),
                coordinate(64, 0x8000_0000),
                -894_618_694,
            ),
        ];
        for (seed, x, z, expected) in cases_2d {
            let sample = open_simplex_2s_2d_v1(seed, x, z)
                .unwrap_or_else(|error| panic!("2D vector evaluates: {error}"));
            assert_eq!(sample.raw_q30(), expected);
        }

        let cases_3d = [
            (
                0_i64,
                coordinate(0, 0),
                coordinate(0, 0),
                coordinate(0, 0),
                0_i32,
            ),
            (
                42,
                coordinate(0, 0x4000_0000),
                coordinate(-1, 0x4000_0000),
                coordinate(1, 0x8000_0000),
                -265_611_032,
            ),
            (
                -1,
                coordinate(12_345, 0x2000_0000),
                coordinate(-33, 0xc000_0000),
                coordinate(-9_877, 0x8000_0000),
                -637_417_149,
            ),
            (
                ORACLE_SEED,
                coordinate(-32, 1),
                coordinate(64, 0x8000_0000),
                coordinate(-5, 0xe000_0000),
                261_583_048,
            ),
        ];
        for (seed, x, y, z, expected) in cases_3d {
            let sample = open_simplex_2f_3d_v1(seed, x, y, z)
                .unwrap_or_else(|error| panic!("3D vector evaluates: {error}"));
            assert_eq!(sample.raw_q30(), expected);
        }
    }

    #[test]
    fn fixed_vectors_stay_within_the_float_reference_error_envelope() {
        let cases_2d = [
            (
                42,
                coordinate(0, 0x4000_0000),
                coordinate(-1, 0x4000_0000),
                -0.074_541_173_875_331_88,
            ),
            (
                -1,
                coordinate(12_345, 0x2000_0000),
                coordinate(-9_877, 0x8000_0000),
                -0.736_090_838_909_149_2,
            ),
            (
                ORACLE_SEED,
                coordinate(-32, 1),
                coordinate(64, 0x8000_0000),
                -0.833_178_639_411_926_3,
            ),
        ];
        for (seed, x, z, oracle) in cases_2d {
            let fixed = sample_as_f64(
                open_simplex_2s_2d_v1(seed, x, z)
                    .unwrap_or_else(|error| panic!("2D oracle vector evaluates: {error}")),
            );
            assert!((fixed - oracle).abs() <= 0.000_001);
        }
        let cases_3d = [
            (
                42,
                coordinate(0, 0x4000_0000),
                coordinate(-1, 0x4000_0000),
                coordinate(1, 0x8000_0000),
                -0.247_369_721_531_867_98,
            ),
            (
                -1,
                coordinate(12_345, 0x2000_0000),
                coordinate(-33, 0xc000_0000),
                coordinate(-9_877, 0x8000_0000),
                -0.593_641_042_709_350_6,
            ),
            (
                ORACLE_SEED,
                coordinate(-32, 1),
                coordinate(64, 0x8000_0000),
                coordinate(-5, 0xe000_0000),
                0.243_618_294_596_672_06,
            ),
        ];
        for (seed, x, y, z, oracle) in cases_3d {
            let fixed = sample_as_f64(
                open_simplex_2f_3d_v1(seed, x, y, z)
                    .unwrap_or_else(|error| panic!("3D oracle vector evaluates: {error}")),
            );
            assert!((fixed - oracle).abs() <= 0.000_001);
        }
    }

    #[test]
    fn coordinate_envelope_and_one_past_boundary_fail_closed() {
        let maximum = coordinate(MAX_FIXED_FIELD_INTEGER_COORDINATE_V1, u32::MAX);
        let minimum = coordinate(-MAX_FIXED_FIELD_INTEGER_COORDINATE_V1, 0);
        assert!(open_simplex_2s_2d_v1(i64::MIN, maximum, minimum).is_ok());
        assert!(open_simplex_2f_3d_v1(i64::MAX, minimum, maximum, minimum).is_ok());
        assert!(FixedCoordinateV1::new(MAX_FIXED_FIELD_INTEGER_COORDINATE_V1 + 1, 0).is_err());
    }

    #[test]
    #[allow(
        clippy::similar_names,
        reason = "NE and NW name the two measured diagonal directions"
    )]
    fn fixed_map_has_no_quantization_plateaus_or_axis_energy_spike() {
        let edge = 96_i64;
        let scale = NonZeroU32::new(24).unwrap_or_else(|| panic!("scale is nonzero"));
        let mut rows = Vec::with_capacity(usize::try_from(edge * edge).unwrap_or_default());
        for z in 0..edge {
            for x in 0..edge {
                let x = FixedCoordinateV1::from_voxel(x - edge / 2, scale)
                    .unwrap_or_else(|error| panic!("X coordinate converts: {error}"));
                let z = FixedCoordinateV1::from_voxel(z - edge / 2, scale)
                    .unwrap_or_else(|error| panic!("Z coordinate converts: {error}"));
                rows.push(
                    open_simplex_2s_2d_v1(0x5eed, x, z)
                        .unwrap_or_else(|error| panic!("field evaluates: {error}"))
                        .raw_q30(),
                );
            }
        }
        let edge_usize = usize::try_from(edge).unwrap_or_default();
        let mut equal_neighbors = 0_u64;
        let mut comparisons = 0_u64;
        let mut energy_x = 0_u128;
        let mut energy_z = 0_u128;
        let mut energy_ne = 0_u128;
        let mut energy_nw = 0_u128;
        for z in 0..edge_usize - 1 {
            for x in 0..edge_usize - 1 {
                let index = x + edge_usize * z;
                let dx = i64::from(rows[index + 1]) - i64::from(rows[index]);
                let dz = i64::from(rows[index + edge_usize]) - i64::from(rows[index]);
                let dne = i64::from(rows[index + edge_usize + 1]) - i64::from(rows[index]);
                let dnw = if x > 0 {
                    i64::from(rows[index + edge_usize - 1]) - i64::from(rows[index])
                } else {
                    dne
                };
                equal_neighbors += u64::from(dx == 0) + u64::from(dz == 0);
                comparisons += 2;
                energy_x = energy_x.saturating_add(u128::from(dx.unsigned_abs()).pow(2));
                energy_z = energy_z.saturating_add(u128::from(dz.unsigned_abs()).pow(2));
                energy_ne = energy_ne.saturating_add(u128::from(dne.unsigned_abs()).pow(2));
                energy_nw = energy_nw.saturating_add(u128::from(dnw.unsigned_abs()).pow(2));
            }
        }
        assert!(equal_neighbors.saturating_mul(10_000) <= comparisons);
        assert_balanced_energy(energy_x, energy_z, 70);
        assert_balanced_energy(energy_ne, energy_nw, 70);
    }

    #[test]
    fn small_radial_spectrum_has_no_axis_dominant_band() {
        const EDGE: usize = 32;
        let scale = NonZeroU32::new(8).unwrap_or_else(|| panic!("scale is nonzero"));
        let mut samples = [0_f64; EDGE * EDGE];
        for z in 0..EDGE {
            for x in 0..EDGE {
                let xi = i64::try_from(x).unwrap_or_default() - 16;
                let zi = i64::try_from(z).unwrap_or_default() - 16;
                let point_x = FixedCoordinateV1::from_voxel(xi, scale)
                    .unwrap_or_else(|error| panic!("spectrum X converts: {error}"));
                let point_z = FixedCoordinateV1::from_voxel(zi, scale)
                    .unwrap_or_else(|error| panic!("spectrum Z converts: {error}"));
                samples[x + EDGE * z] = sample_as_f64(
                    open_simplex_2s_2d_v1(0x517e_5eed, point_x, point_z)
                        .unwrap_or_else(|error| panic!("spectrum field evaluates: {error}")),
                );
            }
        }
        let edge_u32 = u32::try_from(EDGE).unwrap_or(u32::MAX);
        let sample_count = u32::try_from(EDGE * EDGE).unwrap_or(u32::MAX);
        let mean = samples.iter().sum::<f64>() / f64::from(sample_count);
        let mut axis = 0_f64;
        let mut diagonal = 0_f64;
        let mut off_axis = 0_f64;
        for kz in -8_i32..=8 {
            for kx in -8_i32..=8 {
                if kx == 0 && kz == 0 {
                    continue;
                }
                let mut real = 0_f64;
                let mut imaginary = 0_f64;
                for z in 0..EDGE {
                    for x in 0..EDGE {
                        let phase = -std::f64::consts::TAU
                            * (f64::from(kx) * f64::from(u32::try_from(x).unwrap_or_default())
                                + f64::from(kz) * f64::from(u32::try_from(z).unwrap_or_default()))
                            / f64::from(edge_u32);
                        let sample = samples[x + EDGE * z] - mean;
                        real += sample * phase.cos();
                        imaginary += sample * phase.sin();
                    }
                }
                let energy = real.mul_add(real, imaginary * imaginary);
                if kx == 0 || kz == 0 {
                    axis += energy;
                } else if kx.unsigned_abs() == kz.unsigned_abs() {
                    diagonal += energy;
                } else {
                    off_axis += energy;
                }
            }
        }
        assert!(axis.is_finite() && diagonal.is_finite() && off_axis.is_finite());
        assert!(axis < off_axis * 0.75, "axis={axis} off_axis={off_axis}");
        assert!(
            diagonal < off_axis * 0.75,
            "diagonal={diagonal} off_axis={off_axis}"
        );
    }

    #[test]
    fn fixed_field_corpus_has_a_byte_stable_golden_digest() {
        let scale = NonZeroU32::new(37).unwrap_or_else(|| panic!("scale is nonzero"));
        let mut digest = Sha256::new();
        for seed in [i64::MIN, -1, 0, 42, i64::MAX] {
            for z in -32_i64..32 {
                for x in -32_i64..32 {
                    let x = FixedCoordinateV1::from_voxel(x, scale)
                        .unwrap_or_else(|error| panic!("golden X converts: {error}"));
                    let z = FixedCoordinateV1::from_voxel(z, scale)
                        .unwrap_or_else(|error| panic!("golden Z converts: {error}"));
                    let raw = open_simplex_2s_2d_v1(seed, x, z)
                        .unwrap_or_else(|error| panic!("golden 2D sample evaluates: {error}"))
                        .raw_q30();
                    digest.update(raw.to_le_bytes());
                }
            }
        }
        for z in -8_i64..8 {
            for y in -8_i64..8 {
                for x in -8_i64..8 {
                    let x = FixedCoordinateV1::from_voxel(x, scale)
                        .unwrap_or_else(|error| panic!("golden X converts: {error}"));
                    let y = FixedCoordinateV1::from_voxel(y, scale)
                        .unwrap_or_else(|error| panic!("golden Y converts: {error}"));
                    let z = FixedCoordinateV1::from_voxel(z, scale)
                        .unwrap_or_else(|error| panic!("golden Z converts: {error}"));
                    let raw = open_simplex_2f_3d_v1(0x6a09_e667, x, y, z)
                        .unwrap_or_else(|error| panic!("golden 3D sample evaluates: {error}"))
                        .raw_q30();
                    digest.update(raw.to_le_bytes());
                }
            }
        }
        let actual: [u8; 32] = digest.finalize().into();
        assert_eq!(
            actual,
            [
                77, 171, 75, 7, 178, 188, 38, 13, 200, 162, 236, 53, 106, 8, 249, 240, 111, 118,
                89, 111, 56, 47, 35, 228, 221, 156, 166, 85, 49, 221, 151, 192,
            ]
        );
    }

    fn assert_balanced_energy(first: u128, second: u128, minimum_percent: u128) {
        let lower = first.min(second);
        let upper = first.max(second);
        assert!(
            lower.saturating_mul(100) >= upper.saturating_mul(minimum_percent),
            "directional energy imbalance: {first} versus {second}"
        );
    }
}
