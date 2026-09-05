//! Development oracle comparison for the fixed `OpenSimplex2` candidates.
#![allow(
    clippy::print_stdout,
    reason = "this developer oracle intentionally prints each conformance delta"
)]

use std::{error::Error, io};

use latticeaxiom_worldgen::{
    FIXED_FIELD_OUTPUT_BITS_V1, FixedCoordinateV1, open_simplex_2f_3d_v1, open_simplex_2s_2d_v1,
};

const ORACLE_SEED: i64 = i64::from_le_bytes(0x1234_5678_9abc_def0_u64.to_le_bytes());
const MAX_ABSOLUTE_ERROR: f64 = 0.000_001;

fn main() -> Result<(), Box<dyn Error>> {
    let vectors_2d = [
        (0_i64, coordinate(0, 0)?, coordinate(0, 0)?, -0.0_f64),
        (
            42,
            coordinate(0, 0x4000_0000)?,
            coordinate(-1, 0x4000_0000)?,
            -0.074_541_173_875_331_88,
        ),
        (
            -1,
            coordinate(12_345, 0x2000_0000)?,
            coordinate(-9_877, 0x8000_0000)?,
            -0.736_090_838_909_149_2,
        ),
        (
            ORACLE_SEED,
            coordinate(-32, 1)?,
            coordinate(64, 0x8000_0000)?,
            -0.833_178_639_411_926_3,
        ),
    ];
    for (seed, x, z, oracle) in vectors_2d {
        let fixed = open_simplex_2s_2d_v1(seed, x, z)?;
        print_row("2S", seed, fixed.raw_q30(), oracle)?;
    }

    let vectors_3d = [
        (
            0_i64,
            coordinate(0, 0)?,
            coordinate(0, 0)?,
            coordinate(0, 0)?,
            0.0_f64,
        ),
        (
            42,
            coordinate(0, 0x4000_0000)?,
            coordinate(-1, 0x4000_0000)?,
            coordinate(1, 0x8000_0000)?,
            -0.247_369_721_531_867_98,
        ),
        (
            -1,
            coordinate(12_345, 0x2000_0000)?,
            coordinate(-33, 0xc000_0000)?,
            coordinate(-9_877, 0x8000_0000)?,
            -0.593_641_042_709_350_6,
        ),
        (
            ORACLE_SEED,
            coordinate(-32, 1)?,
            coordinate(64, 0x8000_0000)?,
            coordinate(-5, 0xe000_0000)?,
            0.243_618_294_596_672_06,
        ),
    ];
    for (seed, x, y, z, oracle) in vectors_3d {
        let fixed = open_simplex_2f_3d_v1(seed, x, y, z)?;
        print_row("2F", seed, fixed.raw_q30(), oracle)?;
    }
    Ok(())
}

fn coordinate(integer: i64, fraction_q32: u32) -> Result<FixedCoordinateV1, Box<dyn Error>> {
    Ok(FixedCoordinateV1::new(integer, fraction_q32)?)
}

fn print_row(algorithm: &str, seed: i64, raw: i32, oracle: f64) -> Result<(), Box<dyn Error>> {
    let fixed = f64::from(raw) / f64::from(1_u32 << FIXED_FIELD_OUTPUT_BITS_V1);
    let absolute_error = (fixed - oracle).abs();
    println!(
        "{algorithm} seed={seed:>20} raw={raw:>12} fixed={fixed:+.12} oracle={oracle:+.12} abs_error={absolute_error:.9}"
    );
    if absolute_error > MAX_ABSOLUTE_ERROR {
        return Err(io::Error::other(format!(
            "{algorithm} seed {seed} error {absolute_error} exceeds {MAX_ABSOLUTE_ERROR}"
        ))
        .into());
    }
    Ok(())
}
