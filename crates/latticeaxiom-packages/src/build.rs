use std::fmt::Write as _;
use std::path::Path;

use latticeaxiom_compose::Realization;

use crate::canonical::{canonical_plan_bytes, plan_hash, write_bytes};
use crate::model::{
    BUILD_PLAN_SCHEMA_VERSION, BuildJob, BuildJobKind, BuildPlan, LockedGameGraph, PackageError,
};

/// Derives the deterministic milestone 2 build DAG from a locked graph.
///
/// # Errors
///
/// Returns [`PackageError::Json`] only if hashing the versioned plan fails.
pub fn build_plan(graph: &LockedGameGraph) -> Result<BuildPlan, PackageError> {
    let mut jobs = Vec::with_capacity(graph.packages.len() * 2 + 2);
    for package in &graph.packages {
        jobs.push(BuildJob {
            id: format!("prepare/{}", package.id),
            kind: BuildJobKind::PrepareSource,
            package: Some(package.id.clone()),
            dependencies: Vec::new(),
            input_sha256: package.content_sha256.clone(),
        });
        let mut dependencies = vec![format!("prepare/{}", package.id)];
        dependencies.extend(
            package
                .dependencies
                .iter()
                .map(|dependency| format!("realize/{dependency}")),
        );
        dependencies.sort();
        jobs.push(BuildJob {
            id: format!("realize/{}", package.id),
            kind: match package.realization {
                Realization::Data => BuildJobKind::RealizeData,
                Realization::NativeStatic => BuildJobKind::CompileNativeStatic,
                Realization::NativeDynamic => BuildJobKind::ValidateNativeDynamic,
            },
            package: Some(package.id.clone()),
            dependencies,
            input_sha256: package.content_sha256.clone(),
        });
    }
    let mut glue_dependencies = graph
        .packages
        .iter()
        .filter(|package| package.realization == Realization::NativeStatic)
        .map(|package| format!("realize/{}", package.id))
        .collect::<Vec<_>>();
    glue_dependencies.sort();
    jobs.push(BuildJob {
        id: "generate/static-glue".to_owned(),
        kind: BuildJobKind::GenerateStaticGlue,
        package: None,
        dependencies: glue_dependencies,
        input_sha256: graph.graph_sha256.clone(),
    });
    let mut closure_dependencies = graph
        .packages
        .iter()
        .map(|package| format!("realize/{}", package.id))
        .chain(std::iter::once("generate/static-glue".to_owned()))
        .collect::<Vec<_>>();
    closure_dependencies.sort();
    jobs.push(BuildJob {
        id: "package/game-closure".to_owned(),
        kind: BuildJobKind::PackageClosure,
        package: None,
        dependencies: closure_dependencies,
        input_sha256: graph.graph_sha256.clone(),
    });
    jobs.sort_by(|left, right| left.id.cmp(&right.id));
    let mut plan = BuildPlan {
        schema_version: BUILD_PLAN_SCHEMA_VERSION,
        graph_sha256: graph.graph_sha256.clone(),
        plan_sha256: String::new(),
        jobs,
    };
    plan.plan_sha256 = plan_hash(&plan)?;
    Ok(plan)
}

/// Writes a canonical build plan and minimal deterministic static-glue crate.
///
/// The milestone 2 glue crate records the exact native-static package closure.
/// Package registration entry points are introduced with native packages; the
/// official milestone 3 content package is pure data.
///
/// # Errors
///
/// Returns [`PackageError`] if serialization, formatting, or file-system access
/// fails.
pub fn write_build_artifacts(
    output_root: impl AsRef<Path>,
    graph: &LockedGameGraph,
    plan: &BuildPlan,
) -> Result<(), PackageError> {
    let output_root = output_root.as_ref();
    write_bytes(
        &output_root.join("build-plan.json"),
        &canonical_plan_bytes(plan)?,
    )?;
    let crate_root = output_root.join("static-glue");
    let cargo = "[package]\nname = \"latticeaxiom-static-glue\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[lib]\npath = \"src/lib.rs\"\n";
    write_bytes(&crate_root.join("Cargo.toml"), cargo.as_bytes())?;
    let mut source = String::from(
        "//! Generated from LockedGameGraph; do not edit.\n\n/// Native-static package ids in deterministic registration order.\npub const STATIC_PACKAGES: &[&str] = &[\n",
    );
    for package in graph
        .packages
        .iter()
        .filter(|package| package.realization == Realization::NativeStatic)
    {
        writeln!(&mut source, "    {:?},", package.id.as_str()).map_err(|source| {
            PackageError::Io {
                path: crate_root.join("src/lib.rs"),
                source: std::io::Error::other(source),
            }
        })?;
    }
    source.push_str("];\n");
    write_bytes(&crate_root.join("src/lib.rs"), source.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use latticeaxiom_compose::Evaluator;

    use super::build_plan;
    use crate::PackageKernel;

    #[test]
    fn plan_jobs_are_sorted_and_closed() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let evaluator = Evaluator::new(root.join("nickel"));
        let spec = evaluator
            .evaluate_game(root.join("profiles/dev.ncl"))
            .expect("checked-in profile evaluates");
        let graph = PackageKernel::new(&root, evaluator)
            .resolve(&spec)
            .expect("checked-in package graph resolves");
        let plan = build_plan(&graph).expect("plan hashing must succeed");
        assert!(plan.jobs.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert!(plan.jobs.iter().any(|job| job.id == "package/game-closure"));
    }
}
