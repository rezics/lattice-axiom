//! Lattice Axiom composition command line.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};
use latticeaxiom_compose::Evaluator;
use latticeaxiom_modules::RuntimeImage;
use latticeaxiom_packages::{
    PackageKernel, build_plan, canonical_lock_bytes, read_lock, write_build_artifacts,
    write_descriptor, write_lock,
};

#[derive(Debug, Parser)]
#[command(
    name = "latticeaxiom",
    version,
    about = "Compose and verify a Lattice Axiom game closure"
)]
struct Cli {
    #[arg(
        long,
        default_value = ".",
        global = true,
        help = "Project root containing nickel/, profiles/, and packages/"
    )]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Evaluate Nickel contracts and construct a typed `CompositionSpec`.
    Check {
        #[arg(default_value = "profiles/dev.ncl")]
        profile: PathBuf,
    },
    /// Resolve exact local sources and write canonical latticeaxiom.lock.
    Lock {
        #[arg(default_value = "profiles/dev.ncl")]
        profile: PathBuf,
        #[arg(short, long, default_value = "latticeaxiom.lock")]
        output: PathBuf,
    },
    /// Derive a `BuildPlan` and generate deterministic milestone 2 artifacts.
    Build {
        #[arg(short, long, default_value = "latticeaxiom.lock")]
        lock: PathBuf,
        #[arg(short, long, default_value = "target/latticeaxiom")]
        output: PathBuf,
    },
    /// Generate a typed published-package descriptor from a package source.
    Pack {
        package: PathBuf,
        #[arg(short, long, default_value = "target/latticeaxiom/packed")]
        output: PathBuf,
    },
    /// Verify source hashes, lock drift, runtime IDs, target, and toolchain.
    Doctor {
        #[arg(default_value = "profiles/dev.ncl")]
        profile: PathBuf,
        #[arg(short, long, default_value = "latticeaxiom.lock")]
        lock: PathBuf,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let evaluator = Evaluator::new(cli.root.join("nickel"));
    let kernel = PackageKernel::new(&cli.root, evaluator.clone());
    match cli.command {
        Command::Check { profile } => {
            let profile = rooted(&cli.root, &profile);
            let spec = evaluator
                .evaluate_game(&profile)
                .with_context(|| format!("checking `{}`", profile.display()))?;
            eprintln!(
                "ok: profile {}@{} contains {} exact package request(s)",
                spec.root_profile.id,
                spec.root_profile.version,
                spec.package_requests.len()
            );
        }
        Command::Lock { profile, output } => {
            let profile = rooted(&cli.root, &profile);
            let output = rooted(&cli.root, &output);
            let spec = evaluator
                .evaluate_game(&profile)
                .with_context(|| format!("evaluating `{}`", profile.display()))?;
            let graph = kernel
                .resolve(&spec)
                .context("resolving exact package graph")?;
            write_lock(&output, &graph)
                .with_context(|| format!("writing `{}`", output.display()))?;
            eprintln!(
                "ok: locked {} package(s) as {}",
                graph.packages.len(),
                graph.graph_sha256
            );
        }
        Command::Build { lock, output } => {
            let lock = rooted(&cli.root, &lock);
            let output = rooted(&cli.root, &output);
            let graph =
                read_lock(&lock).with_context(|| format!("reading `{}`", lock.display()))?;
            let image = RuntimeImage::from_locked_graph(&graph)
                .context("compiling stable runtime registries")?;
            let plan = build_plan(&graph).context("deriving deterministic build plan")?;
            write_build_artifacts(&output, &graph, &plan)
                .with_context(|| format!("writing build artifacts to `{}`", output.display()))?;
            eprintln!(
                "ok: planned {} job(s), {} runtime block(s), plan {}",
                plan.jobs.len(),
                image.blocks().len(),
                plan.plan_sha256
            );
        }
        Command::Pack { package, output } => {
            let output = rooted(&cli.root, &output);
            let descriptor = kernel
                .describe_package(&package)
                .with_context(|| format!("packing `{}`", package.display()))?;
            let descriptor_path = output.join("latticeaxiom-package.json");
            write_descriptor(&descriptor_path, &descriptor)
                .with_context(|| format!("writing `{}`", descriptor_path.display()))?;
            eprintln!(
                "ok: packed {}@{} with content hash {}",
                descriptor.package().id,
                descriptor.package().version,
                descriptor.content_sha256
            );
        }
        Command::Doctor { profile, lock } => {
            let profile = rooted(&cli.root, &profile);
            let lock = rooted(&cli.root, &lock);
            doctor(&evaluator, &kernel, &profile, &lock)?;
        }
    }
    Ok(())
}

fn doctor(
    evaluator: &Evaluator,
    kernel: &PackageKernel,
    profile: &Path,
    lock: &Path,
) -> Result<()> {
    let spec = evaluator
        .evaluate_game(profile)
        .with_context(|| format!("evaluating `{}`", profile.display()))?;
    let current = kernel
        .resolve(&spec)
        .context("resolving current exact sources")?;
    let locked = read_lock(lock).with_context(|| format!("reading `{}`", lock.display()))?;
    let on_disk = fs::read(lock).with_context(|| format!("reading `{}`", lock.display()))?;
    if on_disk != canonical_lock_bytes(&locked)? {
        bail!(
            "`{}` is valid but non-canonical; run `latticeaxiom lock`",
            lock.display()
        );
    }
    if locked != current {
        bail!(
            "`{}` has drifted from the profile or package sources; run `latticeaxiom lock`",
            lock.display()
        );
    }
    let image =
        RuntimeImage::from_locked_graph(&locked).context("compiling stable runtime registries")?;
    eprintln!(
        "ok: lock is canonical and current; target={}, toolchain={}, graph={}, blocks={}",
        locked.target,
        locked.toolchain,
        locked.graph_sha256,
        image.blocks().len()
    );
    Ok(())
}

fn rooted(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{Cli, Command};

    #[test]
    fn all_milestone_two_commands_parse() {
        for command in ["check", "lock", "build", "pack", "doctor"] {
            let mut arguments = vec!["latticeaxiom", command];
            if command == "pack" {
                arguments.push("packages/official");
            }
            let parsed = Cli::try_parse_from(arguments)
                .expect("every documented milestone 2 command must parse");
            assert!(matches!(
                parsed.command,
                Command::Check { .. }
                    | Command::Lock { .. }
                    | Command::Build { .. }
                    | Command::Pack { .. }
                    | Command::Doctor { .. }
            ));
        }
    }
}
