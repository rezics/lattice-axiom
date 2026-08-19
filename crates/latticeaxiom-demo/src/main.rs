//! Lattice Axiom demo host (milestone 1).
//!
//! Windowed mode opens a winit window, draws the scene through the wgpu
//! renderer, and lets you fly the camera. Headless mode simulates and
//! validates frames through the GPU-free renderer — the mode CI uses, since
//! milestone 1 requires the whole vertical to be checkable without a GPU.

mod app;
mod camera_controller;
mod headless;
mod scene;

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
