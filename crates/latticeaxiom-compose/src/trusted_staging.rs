//! Trusted local evaluation from a verified immutable source closure.
//!
//! This adapter exists to exercise shipped authoring sources before the
//! production Nickel worker has a source-table-native import resolver. It
//! verifies a complete [`SourceClosureRequest`] against immutable snapshots,
//! stages only the exact reachable bytes into a fresh temporary tree, and
//! configures Nickel package aliases exclusively from the verified grants.
//!
//! It is not the production `r0@1` containment boundary: Nickel still reads
//! the private staging tree through its filesystem resolver, and this adapter
//! does not enforce hard process memory, recursion, or call meters. Production
//! `r0@1` evaluation therefore remains fail-closed in the worker.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use latticeaxiom_core::{
    CanonicalHash, SourceId, SourceProvenance, canonical_json_bytes, canonical_json_hash,
};
use nickel_lang_core::eval::cache::CacheImpl;
use nickel_lang_core::identifier::Ident;
use nickel_lang_core::package::PackageMap;
use nickel_lang_core::program::{Program, ProgramBuilder};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

use crate::{
    PackageSpec, SourceAddress, SourceClosureError, SourceClosureReceipt, SourceClosureRequest,
    SourceSnapshot, resolve_nickel_source_closure,
};

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const STAGING_PREFIX: &str = "latticeaxiom-trusted-source-table";

/// Successful typed evaluation through the trusted staged adapter.
#[derive(Debug)]
pub struct TrustedStagedEvaluation<T> {
    /// Bound and typed authored value.
    pub value: T,
    /// Independently computed receipt for the exact authored import closure.
    pub source_closure: SourceClosureReceipt,
    /// Addresses materialized into the private tree, in receipt order.
    pub staged_addresses: Vec<SourceAddress>,
    /// Transient staging path used by this evaluation.
    ///
    /// The directory has already been removed when this value is returned. It
    /// is exposed only for conformance tests that prove machine-local paths do
    /// not enter canonical typed output.
    pub staging_path: PathBuf,
}

/// Failure from the trusted staged source-table adapter.
#[derive(Debug, Error)]
pub enum TrustedStagedEvaluationError {
    /// The request, grants, imports, or closure receipt was invalid.
    #[error("source closure rejected: {0}")]
    SourceClosure(#[from] SourceClosureError),
    /// A frozen snapshot or grant binding was invalid.
    #[error("snapshot set rejected: {details}")]
    SnapshotSet {
        /// Stable validation detail.
        details: String,
    },
    /// A granted Nickel package alias cannot be represented by this adapter.
    #[error("package alias `{alias}` must target `main.ncl`, found `{entry}`")]
    AliasEntry {
        /// Granted alias.
        alias: String,
        /// Unsupported logical entry.
        entry: String,
    },
    /// The private source tree could not be created or populated.
    #[error("staging I/O failed at `{path}`: {source}")]
    StagingIo {
        /// Machine-local diagnostic path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The kernel binder could not be rendered as a finite Nickel value.
    #[error("kernel binder is not representable as Nickel data: {details}")]
    Binder {
        /// Stable conversion detail.
        details: String,
    },
    /// A two-stage package binder produced invalid or inconsistent receipts.
    #[error("trusted staged package binding failed: {details}")]
    PackageBinding {
        /// Stable binding or package-validation detail.
        details: String,
    },
    /// Nickel construction, import resolution, typing, or evaluation failed.
    #[error("trusted staged Nickel evaluation failed: {details}")]
    Evaluation {
        /// Stable evaluator detail without an original physical source path.
        details: String,
    },
    /// The evaluated value did not match the requested typed model.
    #[error("trusted staged Nickel output has the wrong schema: {details}")]
    Schema {
        /// Serde schema detail.
        details: String,
    },
    /// Canonical encoding of the typed output failed.
    #[error("trusted staged Nickel output could not be canonically encoded: {details}")]
    CanonicalEncoding {
        /// Canonical encoder detail.
        details: String,
    },
    /// Canonical typed output exceeded the request limit.
    #[error("trusted staged Nickel output is {actual} bytes, limit is {limit} bytes")]
    OutputLimit {
        /// Configured inclusive maximum.
        limit: u64,
        /// Actual canonical byte count.
        actual: u64,
    },
}

/// Evaluates and kernel-binds one shipped package source without exposing an unbound spec.
///
/// The authored function receives a raw entry-file provenance receipt and a temporary
/// registration-fragment placeholder. Its typed registration fragment is then hashed by
/// Rust canonical JSON and the same verified source closure is evaluated again with that
/// exact hash. Only the second, fully bound [`PackageSpec`] is returned.
///
/// # Security
///
/// This composes the trusted staged adapter and therefore has the same local-only security
/// boundary. It is not a production `r0@1` containment proof.
///
/// # Errors
///
/// Returns [`TrustedStagedEvaluationError`] for source closure, staging, evaluation,
/// canonical encoding, package validation, or receipt-binding failures.
pub fn evaluate_trusted_staged_package(
    request: &SourceClosureRequest,
    snapshots: &[SourceSnapshot],
    package_provenance: &SourceProvenance,
) -> Result<TrustedStagedEvaluation<PackageSpec>, TrustedStagedEvaluationError> {
    let authored_binder = serde_json::json!({
        "package_provenance": package_provenance,
        "registration_fragment_hash": CanonicalHash::digest(
            b"unbound-registration-fragment-receipt"
        ),
    });
    let authored: TrustedStagedEvaluation<PackageSpec> =
        evaluate_trusted_staged_nickel_function(request, snapshots, &authored_binder)?;
    authored
        .value
        .validate()
        .map_err(|error| TrustedStagedEvaluationError::PackageBinding {
            details: error.to_string(),
        })?;
    let registration_fragment_hash =
        canonical_json_hash(&authored.value.registration).map_err(|error| {
            TrustedStagedEvaluationError::CanonicalEncoding {
                details: error.to_string(),
            }
        })?;
    let bound_binder = serde_json::json!({
        "package_provenance": package_provenance,
        "registration_fragment_hash": registration_fragment_hash,
    });
    let bound: TrustedStagedEvaluation<PackageSpec> =
        evaluate_trusted_staged_nickel_function(request, snapshots, &bound_binder)?;
    bound
        .value
        .validate()
        .map_err(|error| TrustedStagedEvaluationError::PackageBinding {
            details: error.to_string(),
        })?;
    if authored.value.registration != bound.value.registration {
        return Err(TrustedStagedEvaluationError::PackageBinding {
            details: "kernel receipt binding altered authored registration semantics".to_owned(),
        });
    }
    if bound.value.provenance != *package_provenance
        || bound
            .value
            .registration
            .registrations
            .iter()
            .any(|row| row.provenance != *package_provenance)
    {
        return Err(TrustedStagedEvaluationError::PackageBinding {
            details: "package or direct registration provenance differs from the kernel receipt"
                .to_owned(),
        });
    }
    if bound
        .value
        .realizations
        .values()
        .any(|realization| realization.registration_fragment != registration_fragment_hash)
    {
        return Err(TrustedStagedEvaluationError::PackageBinding {
            details: "a realization retained a noncanonical registration-fragment receipt"
                .to_owned(),
        });
    }
    Ok(bound)
}

/// Applies a kernel-owned data binder to a shipped authored Nickel function.
///
/// The entry and every import are read from verified [`SourceSnapshot`] bytes.
/// Original acquisition paths are never passed to Nickel. Only members of the
/// independently resolved closure are materialized, and package aliases come
/// only from the request's explicit grants.
///
/// The entry must evaluate to a unary function. Package sources use the binder
/// for raw entry provenance; profiles use it for distinct whole-root and raw
/// entry-file receipts. The binder is kernel data and is not part of the
/// authored closure hash.
///
/// # Security
///
/// This is a trusted local D0 adapter, not the production controlled evaluator.
/// Callers must not pass untrusted Nickel or use successful evaluation as proof
/// of hard resource containment.
///
/// # Errors
///
/// Returns [`TrustedStagedEvaluationError`] when snapshot/grant verification,
/// static closure authorization, private staging, Nickel evaluation, typed
/// decoding, canonical encoding, or output metering fails.
pub fn evaluate_trusted_staged_nickel_function<T>(
    request: &SourceClosureRequest,
    snapshots: &[SourceSnapshot],
    binder: &Value,
) -> Result<TrustedStagedEvaluation<T>, TrustedStagedEvaluationError>
where
    T: DeserializeOwned + Serialize,
{
    let snapshot_set = verified_snapshot_set(request, snapshots)?;
    let closure = resolve_nickel_source_closure(&snapshot_set, request)?;
    closure.verify_against_snapshot(&snapshot_set)?;

    let staged = StagedSourceTree::create(request, &snapshot_set, &closure)?;
    let staging_path = staged.root().to_path_buf();
    let staged_addresses = closure
        .sources
        .iter()
        .map(|member| member.address.clone())
        .collect::<Vec<_>>();
    let entry = source_bytes(&snapshot_set, &request.entry)?;
    let entry =
        std::str::from_utf8(entry).map_err(|error| TrustedStagedEvaluationError::Evaluation {
            details: format!("entry is not UTF-8: {error}"),
        })?;
    let binder = nickel_literal(binder)?;
    let wrapped = format!("let authored = (\n{entry}\n) in\nauthored ({binder})\n");
    let package_map = staged.package_map(request)?;

    let mut program: Program<CacheImpl> = ProgramBuilder::new()
        .add_source_string(wrapped, "<verified-source-table-entry>")
        .with_package_map(package_map)
        .with_trace(io::sink())
        .build()
        .map_err(|error| TrustedStagedEvaluationError::Evaluation {
            details: error.to_string(),
        })?;
    let evaluated = program.eval_full_for_export().map_err(|error| {
        TrustedStagedEvaluationError::Evaluation {
            details: format!("{error:?}"),
        }
    })?;
    let exported = crate::evaluation::exact_json_value(&evaluated)
        .map_err(|details| TrustedStagedEvaluationError::Schema { details })?;
    let value =
        serde_json::from_value(exported).map_err(|error| TrustedStagedEvaluationError::Schema {
            details: error.to_string(),
        })?;
    let canonical = canonical_json_bytes(&value).map_err(|error| {
        TrustedStagedEvaluationError::CanonicalEncoding {
            details: error.to_string(),
        }
    })?;
    let actual =
        u64::try_from(canonical.len()).map_err(|_| TrustedStagedEvaluationError::OutputLimit {
            limit: request.limits.output_bytes,
            actual: u64::MAX,
        })?;
    if actual > request.limits.output_bytes {
        return Err(TrustedStagedEvaluationError::OutputLimit {
            limit: request.limits.output_bytes,
            actual,
        });
    }

    drop(staged);
    Ok(TrustedStagedEvaluation {
        value,
        source_closure: closure,
        staged_addresses,
        staging_path,
    })
}

fn verified_snapshot_set(
    request: &SourceClosureRequest,
    snapshots: &[SourceSnapshot],
) -> Result<BTreeMap<SourceId, SourceSnapshot>, TrustedStagedEvaluationError> {
    request.validate()?;
    if snapshots.len() != request.root_grants.len() {
        return Err(TrustedStagedEvaluationError::SnapshotSet {
            details: "snapshot set must exactly match the granted root set".to_owned(),
        });
    }

    let grants = request
        .root_grants
        .iter()
        .map(|grant| (&grant.source_id, grant))
        .collect::<BTreeMap<_, _>>();
    let mut snapshot_set = BTreeMap::new();
    for snapshot in snapshots {
        snapshot
            .verify()
            .map_err(|error| TrustedStagedEvaluationError::SnapshotSet {
                details: error.to_string(),
            })?;
        let grant = grants.get(snapshot.source_id()).ok_or_else(|| {
            TrustedStagedEvaluationError::SnapshotSet {
                details: format!("snapshot `{}` has no grant", snapshot.source_id()),
            }
        })?;
        if grant.root_kind != snapshot.root_kind() {
            return Err(TrustedStagedEvaluationError::SnapshotSet {
                details: format!(
                    "snapshot `{}` root kind differs from its grant",
                    snapshot.source_id()
                ),
            });
        }
        if grant.source_hash != snapshot.source_hash() {
            return Err(TrustedStagedEvaluationError::SnapshotSet {
                details: format!(
                    "snapshot `{}` whole-root hash differs from its grant",
                    snapshot.source_id()
                ),
            });
        }
        if snapshot_set
            .insert(snapshot.source_id().clone(), snapshot.clone())
            .is_some()
        {
            return Err(TrustedStagedEvaluationError::SnapshotSet {
                details: format!("duplicate snapshot `{}`", snapshot.source_id()),
            });
        }
    }
    Ok(snapshot_set)
}

fn source_bytes<'a>(
    snapshots: &'a BTreeMap<SourceId, SourceSnapshot>,
    address: &SourceAddress,
) -> Result<&'a [u8], TrustedStagedEvaluationError> {
    snapshots
        .get(address.source_id())
        .and_then(|snapshot| snapshot.files().get(address.logical_path()))
        .map(crate::SourceFileSnapshot::bytes)
        .ok_or_else(|| TrustedStagedEvaluationError::SnapshotSet {
            details: format!("source `{address}` is absent"),
        })
}

struct StagedSourceTree {
    temporary: TemporaryTree,
    roots: BTreeMap<SourceId, PathBuf>,
}

impl StagedSourceTree {
    fn create(
        request: &SourceClosureRequest,
        snapshots: &BTreeMap<SourceId, SourceSnapshot>,
        closure: &SourceClosureReceipt,
    ) -> Result<Self, TrustedStagedEvaluationError> {
        let temporary = TemporaryTree::create()?;
        let mut roots = BTreeMap::new();
        let mut grants = request.root_grants.iter().collect::<Vec<_>>();
        grants.sort_by(|left, right| left.source_id.cmp(&right.source_id));
        for (index, grant) in grants.into_iter().enumerate() {
            let root = temporary.path.join(format!("source-{index:04}"));
            fs::create_dir(&root).map_err(|source| staging_io(&root, source))?;
            roots.insert(grant.source_id.clone(), root);
        }

        for member in &closure.sources {
            let root = roots.get(member.address.source_id()).ok_or_else(|| {
                TrustedStagedEvaluationError::SnapshotSet {
                    details: format!("closure source `{}` has no staged root", member.address),
                }
            })?;
            let destination = logical_destination(root, member.address.logical_path())?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| staging_io(parent, source))?;
            }
            let bytes = source_bytes(snapshots, &member.address)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|source| staging_io(&destination, source))?;
            file.write_all(bytes)
                .map_err(|source| staging_io(&destination, source))?;
            file.flush()
                .map_err(|source| staging_io(&destination, source))?;
            let staged_bytes =
                fs::read(&destination).map_err(|source| staging_io(&destination, source))?;
            if staged_bytes != bytes {
                return Err(TrustedStagedEvaluationError::SnapshotSet {
                    details: format!(
                        "staged source `{}` differs from its frozen bytes",
                        member.address
                    ),
                });
            }
        }

        Ok(Self { temporary, roots })
    }

    fn root(&self) -> &Path {
        &self.temporary.path
    }

    fn package_map(
        &self,
        request: &SourceClosureRequest,
    ) -> Result<PackageMap, TrustedStagedEvaluationError> {
        let mut aliases = Vec::new();
        for grant in &request.root_grants {
            let Some(alias) = &grant.package_alias else {
                continue;
            };
            if grant.entry.logical_path() != "main.ncl" {
                return Err(TrustedStagedEvaluationError::AliasEntry {
                    alias: alias.to_string(),
                    entry: grant.entry.logical_path().to_owned(),
                });
            }
            let root = self.roots.get(&grant.source_id).ok_or_else(|| {
                TrustedStagedEvaluationError::SnapshotSet {
                    details: format!("alias `{alias}` has no staged root"),
                }
            })?;
            aliases.push((Ident::new(alias.as_str()), root.clone()));
        }
        aliases.sort_by_key(|entry| entry.0);

        let top_level = aliases.iter().cloned().collect();
        let mut packages = std::collections::HashMap::new();
        for parent in self.roots.values() {
            for (alias, target) in &aliases {
                packages.insert((parent.clone(), *alias), target.clone());
            }
        }
        Ok(PackageMap {
            top_level,
            packages,
        })
    }
}

fn logical_destination(
    root: &Path,
    logical_path: &str,
) -> Result<PathBuf, TrustedStagedEvaluationError> {
    let mut destination = root.to_path_buf();
    for component in logical_path.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(TrustedStagedEvaluationError::SnapshotSet {
                details: format!("non-canonical staged logical path `{logical_path}`"),
            });
        }
        destination.push(component);
    }
    if !destination.starts_with(root) {
        return Err(TrustedStagedEvaluationError::SnapshotSet {
            details: format!("staged logical path `{logical_path}` escapes its root"),
        });
    }
    Ok(destination)
}

struct TemporaryTree {
    path: PathBuf,
}

impl TemporaryTree {
    fn create() -> Result<Self, TrustedStagedEvaluationError> {
        let parent = std::env::temp_dir();
        for _ in 0..128 {
            let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                "{STAGING_PREFIX}-{}-{sequence:016x}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(staging_io(&path, source)),
            }
        }
        Err(TrustedStagedEvaluationError::StagingIo {
            path: parent,
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate a unique staging directory",
            ),
        })
    }
}

impl Drop for TemporaryTree {
    fn drop(&mut self) {
        let Some(name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if name.starts_with(STAGING_PREFIX)
            && self
                .path
                .parent()
                .is_some_and(|parent| parent == std::env::temp_dir())
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn staging_io(path: &Path, source: io::Error) -> TrustedStagedEvaluationError {
    TrustedStagedEvaluationError::StagingIo {
        path: path.to_path_buf(),
        source,
    }
}

fn nickel_literal(value: &Value) -> Result<String, TrustedStagedEvaluationError> {
    fn render(value: &Value, output: &mut String) -> Result<(), TrustedStagedEvaluationError> {
        match value {
            Value::Null => output.push_str("null"),
            Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => output.push_str(&value.to_string()),
            Value::String(value) => {
                output.push_str(&serde_json::to_string(value).map_err(|error| {
                    TrustedStagedEvaluationError::Binder {
                        details: error.to_string(),
                    }
                })?);
            }
            Value::Array(values) => {
                output.push('[');
                for value in values {
                    render(value, output)?;
                    output.push(',');
                }
                output.push(']');
            }
            Value::Object(values) => {
                output.push('{');
                let mut fields = values.iter().collect::<Vec<_>>();
                fields.sort_by(|left, right| left.0.cmp(right.0));
                for (name, value) in fields {
                    output.push_str(&serde_json::to_string(name).map_err(|error| {
                        TrustedStagedEvaluationError::Binder {
                            details: error.to_string(),
                        }
                    })?);
                    output.push('=');
                    render(value, output)?;
                    output.push(',');
                }
                output.push('}');
            }
        }
        Ok(())
    }

    let mut output = String::new();
    render(value, &mut output)?;
    Ok(output)
}
