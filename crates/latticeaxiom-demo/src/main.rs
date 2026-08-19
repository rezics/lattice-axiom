//! Lattice Axiom first playable sandbox (milestones 1 through 3).
//!
//! Windowed mode composes official content, opens the persistent voxel world,
//! and runs first-person play. Headless mode validates the same composition,
//! streaming, persistence, meshing, physics, and render contracts without a
//! window or GPU.

mod app;
mod block_catalog;
mod chunk_codec;
mod chunk_mesh;
mod chunk_render;
mod headless;
mod player_controller;
mod runtime_config;
mod sandbox_world;
mod ui;

use anyhow::bail;
use tracing_subscriber::EnvFilter;

enum Mode {
    Windowed,
    Headless { frames: u32 },
}

fn parse_args(args: &[String]) -> anyhow::Result<Mode> {
    match args {
        [] => Ok(Mode::Windowed),
        [flag] if flag == "--headless" => Ok(Mode::Headless { frames: 60 }),
        [flag, frames] if flag == "--headless" => Ok(Mode::Headless {
            frames: frames
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid frame count {frames:?}"))?,
        }),
        _ => bail!("usage: latticeaxiom-demo [--headless [FRAMES]]"),
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args)? {
        Mode::Headless { frames } => headless::run(frames),
        Mode::Windowed => app::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert!(matches!(parse_args(&[]).unwrap(), Mode::Windowed));
        assert!(matches!(
            parse_args(&["--headless".to_owned()]).unwrap(),
            Mode::Headless { frames: 60 }
        ));
        assert!(matches!(
            parse_args(&["--headless".to_owned(), "7".to_owned()]).unwrap(),
            Mode::Headless { frames: 7 }
        ));
        assert!(parse_args(&["--nonsense".to_owned()]).is_err());
    }
}
