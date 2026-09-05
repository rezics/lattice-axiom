#!/usr/bin/env python3
"""Generate deterministic Cargo SBOM and third-party notice evidence."""

from __future__ import annotations

import argparse
import copy
import dataclasses
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from typing import Any, Iterable, Mapping, Sequence


CONTRACT_PATH = Path("supply-chain/release-evidence.toml")
ABOUT_CONFIG_PATH = Path("supply-chain/about.toml")
EXPECTED_WORKSPACE_LICENSE = "AGPL-3.0-only"
SCHEMA_URL = "http://cyclonedx.org/schema/bom-1.5.schema.json"
EVIDENCE_KIND = "latticeaxiom.cargo-release-evidence/v1"
ID_PATTERN = re.compile(r"[a-z0-9]+(?:[-_][a-z0-9]+)*\Z")
PACKAGE_PATTERN = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_-]*\Z")
TARGET_PATTERN = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.-]*\Z")
FEATURE_PATTERN = re.compile(r"[A-Za-z0-9_+./?:-]+\Z")
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}\Z")
TOOL_VERSION_PATTERN = re.compile(r"(?<![0-9])([0-9]+\.[0-9]+\.[0-9]+)(?![0-9])")


class EvidenceError(RuntimeError):
    """Raised when release evidence cannot be proven from locked inputs."""


@dataclasses.dataclass(frozen=True)
class Configuration:
    """One shipped Cargo target, profile, and feature closure."""

    identifier: str
    target: str
    profile: str
    package: str
    manifest_path: PurePosixPath
    default_features: bool
    features: tuple[str, ...]

    def as_dict(self) -> dict[str, Any]:
        """Return the canonical public representation."""

        return {
            "default_features": self.default_features,
            "features": list(self.features),
            "id": self.identifier,
            "manifest_path": self.manifest_path.as_posix(),
            "package": self.package,
            "profile": self.profile,
            "target": self.target,
        }


@dataclasses.dataclass(frozen=True)
class Contract:
    """Validated release-evidence configuration."""

    tools: Mapping[str, str]
    configurations: tuple[Configuration, ...]

    def configuration(self, identifier: str) -> Configuration:
        """Find one configuration or fail with a stable diagnostic."""

        matches = [item for item in self.configurations if item.identifier == identifier]
        if len(matches) != 1:
            available = ", ".join(item.identifier for item in self.configurations)
            raise EvidenceError(
                f"unknown configuration {identifier!r}; available: {available}"
            )
        return matches[0]


@dataclasses.dataclass(frozen=True)
class Closure:
    """Locked, target-filtered non-development Cargo dependency closure."""

    root_id: str
    package_by_id: Mapping[str, Mapping[str, Any]]
    node_by_id: Mapping[str, Mapping[str, Any]]
    ids: frozenset[str]
    edges: frozenset[tuple[str, str]]
    edge_kinds: Mapping[tuple[str, str], tuple[str, ...]]
    lock_by_id: Mapping[str, Mapping[str, Any]]
    workspace_members: frozenset[str]


def _require_exact_keys(
    value: Mapping[str, Any], expected: set[str], context: str
) -> None:
    actual = set(value)
    if actual != expected:
        raise EvidenceError(
            f"{context} keys differ: missing={sorted(expected - actual)!r}, "
            f"extra={sorted(actual - expected)!r}"
        )


def _require_string(value: Any, context: str, pattern: re.Pattern[str]) -> str:
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise EvidenceError(f"{context} is invalid: {value!r}")
    return value


def load_contract(path: Path) -> Contract:
    """Load and validate the machine-readable shipped-closure contract."""

    try:
        document = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise EvidenceError(f"cannot read evidence contract {path}: {error}") from error
    if not isinstance(document, dict):
        raise EvidenceError("release-evidence contract must be a TOML table")
    _require_exact_keys(document, {"schema", "tools", "configurations"}, "contract")
    if document["schema"] != 1:
        raise EvidenceError(f"unsupported evidence contract schema: {document['schema']!r}")

    tools = document["tools"]
    if not isinstance(tools, dict):
        raise EvidenceError("contract tools must be a table")
    _require_exact_keys(
        tools,
        {"cargo-cyclonedx", "cargo-about", "cargo-deny", "cyclonedx-spec"},
        "contract tools",
    )
    validated_tools: dict[str, str] = {}
    for name, value in tools.items():
        if not isinstance(value, str):
            raise EvidenceError(f"tool version {name!r} must be a string")
        if name == "cyclonedx-spec":
            if value != "1.5":
                raise EvidenceError("only CycloneDX 1.5 is supported by the pinned generator")
        elif not TOOL_VERSION_PATTERN.fullmatch(value):
            raise EvidenceError(f"tool {name!r} is not an exact version: {value!r}")
        validated_tools[name] = value

    rows = document["configurations"]
    if not isinstance(rows, list) or not rows:
        raise EvidenceError("contract must contain at least one configuration")
    configurations: list[Configuration] = []
    seen_ids: set[str] = set()
    for index, row in enumerate(rows):
        context = f"configuration[{index}]"
        if not isinstance(row, dict):
            raise EvidenceError(f"{context} must be a table")
        _require_exact_keys(
            row,
            {
                "id",
                "target",
                "profile",
                "package",
                "manifest-path",
                "default-features",
                "features",
            },
            context,
        )
        identifier = _require_string(row["id"], f"{context}.id", ID_PATTERN)
        if identifier in seen_ids:
            raise EvidenceError(f"duplicate configuration id: {identifier}")
        seen_ids.add(identifier)
        target = _require_string(row["target"], f"{context}.target", TARGET_PATTERN)
        package = _require_string(row["package"], f"{context}.package", PACKAGE_PATTERN)
        profile = row["profile"]
        if profile != "release":
            raise EvidenceError(
                f"{context}.profile must be 'release'; development dependencies "
                "are intentionally excluded"
            )
        manifest_text = row["manifest-path"]
        if not isinstance(manifest_text, str):
            raise EvidenceError(f"{context}.manifest-path must be a string")
        manifest_path = PurePosixPath(manifest_text)
        if (
            manifest_path.is_absolute()
            or ".." in manifest_path.parts
            or manifest_path.name != "Cargo.toml"
        ):
            raise EvidenceError(f"{context}.manifest-path is unsafe: {manifest_text!r}")
        default_features = row["default-features"]
        if not isinstance(default_features, bool):
            raise EvidenceError(f"{context}.default-features must be a boolean")
        raw_features = row["features"]
        if not isinstance(raw_features, list) or not all(
            isinstance(feature, str) for feature in raw_features
        ):
            raise EvidenceError(f"{context}.features must be an array of strings")
        features = tuple(raw_features)
        if any(not FEATURE_PATTERN.fullmatch(feature) for feature in features):
            raise EvidenceError(f"{context}.features contains an invalid Cargo feature")
        if tuple(sorted(set(features))) != features:
            raise EvidenceError(f"{context}.features must be sorted and unique")
        configurations.append(
            Configuration(
                identifier=identifier,
                target=target,
                profile=profile,
                package=package,
                manifest_path=manifest_path,
                default_features=default_features,
                features=features,
            )
        )

    if tuple(sorted(item.identifier for item in configurations)) != tuple(
        item.identifier for item in configurations
    ):
        raise EvidenceError("configurations must be sorted by id")
    return Contract(tools=validated_tools, configurations=tuple(configurations))


def canonical_json_bytes(value: Any) -> bytes:
    """Serialize JSON with stable keys, whitespace, UTF-8, and LF."""

    text = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)
    return (text + "\n").encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    """Return a lowercase SHA-256 digest."""

    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    """Hash one regular file without following an alternate path."""

    if not path.is_file():
        raise EvidenceError(f"expected regular file: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_command(
    arguments: Sequence[str],
    *,
    cwd: Path,
    environment: Mapping[str, str] | None = None,
) -> str:
    """Run a bounded command and return stdout or a concise failure."""

    process_environment = os.environ.copy()
    if environment is not None:
        process_environment.update(environment)
    completed = subprocess.run(
        list(arguments),
        cwd=cwd,
        env=process_environment,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=900,
    )
    if completed.returncode != 0:
        output = (completed.stdout + "\n" + completed.stderr).strip()
        if len(output) > 12_000:
            output = output[-12_000:]
        rendered = " ".join(arguments)
        raise EvidenceError(
            f"command failed with exit code {completed.returncode}: {rendered}\n{output}"
        )
    return completed.stdout


def check_tool_version(
    arguments: Sequence[str], expected: str, *, cwd: Path
) -> str:
    """Require an exact generator version rather than a compatible range."""

    output = run_command(arguments, cwd=cwd).strip()
    matches = TOOL_VERSION_PATTERN.findall(output)
    if len(matches) != 1 or matches[0] != expected:
        raise EvidenceError(
            f"tool version mismatch for {' '.join(arguments)}: "
            f"expected {expected}, got {output!r}"
        )
    return matches[0]


def feature_arguments(
    configuration: Configuration, *, separator: str = ","
) -> list[str]:
    """Return the explicit Cargo feature selection for one tool invocation."""

    arguments: list[str] = []
    if not configuration.default_features:
        arguments.append("--no-default-features")
    if configuration.features:
        arguments.extend(["--features", separator.join(configuration.features)])
    return arguments


def cargo_metadata(workspace: Path, configuration: Configuration) -> dict[str, Any]:
    """Read exact target-filtered Cargo metadata under --locked."""

    manifest = workspace.joinpath(*configuration.manifest_path.parts)
    if not manifest.is_file():
        raise EvidenceError(f"configuration manifest does not exist: {manifest}")
    arguments = [
        "cargo",
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--manifest-path",
        str(manifest),
        "--filter-platform",
        configuration.target,
        *feature_arguments(configuration),
    ]
    output = run_command(arguments, cwd=workspace)
    try:
        metadata = json.loads(output)
    except json.JSONDecodeError as error:
        raise EvidenceError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(metadata, dict) or metadata.get("version") != 1:
        raise EvidenceError("cargo metadata format version is not 1")
    return metadata

def validate_about_workspace_policy(
    metadata: Mapping[str, Any], policy_path: Path
) -> None:
    """Require explicit cargo-about attribution policy for every workspace crate."""

    try:
        policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise EvidenceError(f"cannot parse cargo-about policy: {error}") from error
    if policy.get("private") != {"ignore": False}:
        raise EvidenceError("cargo-about must not ignore private workspace packages")
    expected_flags = {
        "ignore-build-dependencies": False,
        "ignore-dev-dependencies": True,
        "ignore-transitive-dependencies": False,
    }
    for name, expected in expected_flags.items():
        if policy.get(name) is not expected:
            raise EvidenceError(f"cargo-about policy {name} must be {expected}")
    packages = metadata.get("packages")
    workspace_members = metadata.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(workspace_members, list):
        raise EvidenceError("cargo metadata lacks workspace packages for attribution policy")
    member_ids = set(workspace_members)
    member_names = {
        package.get("name")
        for package in packages
        if package.get("id") in member_ids
    }
    if None in member_names or len(member_names) != len(member_ids):
        raise EvidenceError("workspace package names are missing or duplicated")
    missing = []
    malformed = []
    for name in sorted(member_names):
        table = policy.get(name)
        if table is None:
            missing.append(name)
        elif not isinstance(table, dict) or table.get("accepted") != [
            EXPECTED_WORKSPACE_LICENSE
        ]:
            malformed.append(name)
    if missing or malformed:
        raise EvidenceError(
            "cargo-about workspace attribution policy is incomplete: "
            f"missing={missing!r}, malformed={malformed!r}"
        )

def _non_dev_kinds(dependency: Mapping[str, Any]) -> tuple[str, ...]:
    dep_kinds = dependency.get("dep_kinds", [])
    if not dep_kinds:
        return ("normal",)
    kinds = {
        "normal" if item.get("kind") is None else item.get("kind")
        for item in dep_kinds
        if item.get("kind") != "dev"
    }
    if not all(isinstance(kind, str) for kind in kinds):
        raise EvidenceError("cargo metadata dependency kind is malformed")
    return tuple(sorted(kinds))


def _resolve_root_package(
    metadata: Mapping[str, Any], workspace: Path, configuration: Configuration
) -> Mapping[str, Any]:
    expected_manifest = workspace.joinpath(*configuration.manifest_path.parts).resolve()
    matches = []
    for package in metadata.get("packages", []):
        try:
            package_manifest = Path(package["manifest_path"]).resolve()
        except (KeyError, TypeError, OSError) as error:
            raise EvidenceError("cargo metadata contains a malformed package") from error
        if package.get("name") == configuration.package and package_manifest == expected_manifest:
            matches.append(package)
    if len(matches) != 1:
        raise EvidenceError(
            f"expected exactly one root package {configuration.package!r} at "
            f"{configuration.manifest_path}, found {len(matches)}"
        )
    return matches[0]


def _load_lock_packages(lock_path: Path) -> list[Mapping[str, Any]]:
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise EvidenceError(f"cannot parse exact Cargo.lock: {error}") from error
    packages = lock.get("package")
    if lock.get("version") != 4 or not isinstance(packages, list):
        raise EvidenceError("Cargo.lock must use supported format version 4")
    if not all(isinstance(package, dict) for package in packages):
        raise EvidenceError("Cargo.lock package table is malformed")
    return packages


def _lock_entry_for_package(
    package: Mapping[str, Any], lock_packages: Iterable[Mapping[str, Any]]
) -> Mapping[str, Any]:
    key = (package.get("name"), package.get("version"), package.get("source"))
    matches = [
        entry
        for entry in lock_packages
        if (entry.get("name"), entry.get("version"), entry.get("source")) == key
    ]
    if len(matches) != 1:
        raise EvidenceError(
            f"locked package match is not unique for {key!r}: found {len(matches)}"
        )
    return matches[0]


def derive_closure(
    metadata: Mapping[str, Any],
    lock_path: Path,
    workspace: Path,
    configuration: Configuration,
) -> Closure:
    """Derive the release graph without dev-only dependencies."""

    packages = metadata.get("packages")
    resolution = metadata.get("resolve")
    if not isinstance(packages, list) or not isinstance(resolution, dict):
        raise EvidenceError("cargo metadata is missing packages or resolve graph")
    nodes = resolution.get("nodes")
    if not isinstance(nodes, list):
        raise EvidenceError("cargo metadata resolve graph is missing nodes")
    package_by_id = {package.get("id"): package for package in packages}
    node_by_id = {node.get("id"): node for node in nodes}
    if None in package_by_id or None in node_by_id:
        raise EvidenceError("cargo metadata contains a package or node without an id")
    if len(package_by_id) != len(packages) or len(node_by_id) != len(nodes):
        raise EvidenceError("cargo metadata contains duplicate package or node ids")

    root = _resolve_root_package(metadata, workspace, configuration)
    root_id = root["id"]
    pending = [root_id]
    closure_ids = {root_id}
    edges: set[tuple[str, str]] = set()
    edge_kind_sets: dict[tuple[str, str], set[str]] = {}
    while pending:
        package_id = pending.pop()
        node = node_by_id.get(package_id)
        if node is None:
            raise EvidenceError(f"cargo metadata has no resolve node for {package_id}")
        dependencies = node.get("deps")
        if not isinstance(dependencies, list):
            raise EvidenceError(f"cargo metadata node deps are malformed for {package_id}")
        for dependency in dependencies:
            kinds = _non_dev_kinds(dependency)
            if not kinds:
                continue
            dependency_id = dependency.get("pkg")
            if dependency_id not in package_by_id:
                raise EvidenceError(
                    f"cargo metadata edge references unknown package {dependency_id!r}"
                )
            edge = (package_id, dependency_id)
            edges.add(edge)
            edge_kind_sets.setdefault(edge, set()).update(kinds)
            if dependency_id not in closure_ids:
                closure_ids.add(dependency_id)
                pending.append(dependency_id)

    lock_packages = _load_lock_packages(lock_path)
    lock_by_id: dict[str, Mapping[str, Any]] = {}
    workspace_root = workspace.resolve()
    for package_id in closure_ids:
        package = package_by_id[package_id]
        entry = _lock_entry_for_package(package, lock_packages)
        lock_by_id[package_id] = entry
        source = package.get("source")
        if source is None:
            manifest = Path(package["manifest_path"]).resolve()
            if not manifest.is_relative_to(workspace_root):
                raise EvidenceError(
                    f"path dependency escapes the workspace snapshot: {manifest}"
                )
            if entry.get("checksum") is not None:
                raise EvidenceError(f"path package unexpectedly has a lock checksum: {package_id}")
        elif source.startswith("registry+"):
            checksum = entry.get("checksum")
            if not isinstance(checksum, str) or not SHA256_PATTERN.fullmatch(checksum):
                raise EvidenceError(
                    f"registry package lacks a valid locked checksum: {package_id}"
                )

    workspace_members = frozenset(metadata.get("workspace_members", []))
    if not workspace_members:
        raise EvidenceError("cargo metadata has no workspace members")
    missing_workspace = workspace_members - package_by_id.keys()
    if missing_workspace:
        raise EvidenceError(
            f"workspace member ids are absent from metadata packages: "
            f"{sorted(missing_workspace)!r}"
        )
    for package_id in sorted(workspace_members):
        package = package_by_id[package_id]
        license_expression = package.get("license")
        if license_expression != EXPECTED_WORKSPACE_LICENSE:
            raise EvidenceError(
                f"workspace package {package['name']}@{package['version']} must declare "
                f"{EXPECTED_WORKSPACE_LICENSE}; got {license_expression!r}"
            )

    edge_kinds = {
        edge: tuple(sorted(kinds)) for edge, kinds in edge_kind_sets.items()
    }
    return Closure(
        root_id=root_id,
        package_by_id=package_by_id,
        node_by_id=node_by_id,
        ids=frozenset(closure_ids),
        edges=frozenset(edges),
        edge_kinds=edge_kinds,
        lock_by_id=lock_by_id,
        workspace_members=workspace_members,
    )


def _tree_package_id(
    rendered: str, packages: Iterable[Mapping[str, Any]]
) -> str:
    """Map one stable Cargo tree package rendering back to metadata."""

    while rendered.endswith(" (*)"):
        rendered = rendered[:-4]
    if rendered.endswith(" (proc-macro)"):
        rendered = rendered[: -len(" (proc-macro)")]
    match = re.fullmatch(
        r"(?P<name>[A-Za-z0-9_-]+) v(?P<version>[^ ]+)"
        r"(?: \((?P<location>.*)\))?",
        rendered,
    )
    if match is None:
        raise EvidenceError(f"cargo tree package rendering is malformed: {rendered!r}")
    candidates = [
        package
        for package in packages
        if package.get("name") == match.group("name")
        and package.get("version") == match.group("version")
    ]
    location = match.group("location")
    if len(candidates) > 1 and location is not None:
        path_matches = []
        try:
            rendered_path = Path(location).resolve()
        except OSError:
            rendered_path = None
        if rendered_path is not None:
            for package in candidates:
                if package.get("source") is not None:
                    continue
                try:
                    package_path = Path(package["manifest_path"]).resolve().parent
                except (KeyError, OSError, TypeError):
                    continue
                if package_path == rendered_path:
                    path_matches.append(package)
        if len(path_matches) == 1:
            candidates = path_matches
        else:
            source_matches = [
                package
                for package in candidates
                if isinstance(package.get("source"), str)
                and location in package["source"]
            ]
            if len(source_matches) == 1:
                candidates = source_matches
    if len(candidates) != 1 or not isinstance(candidates[0].get("id"), str):
        raise EvidenceError(
            f"cargo tree package is not uniquely mapped to metadata: {rendered!r}"
        )
    return candidates[0]["id"]


def parse_cargo_tree(
    output: str,
    metadata: Mapping[str, Any],
    workspace: Path,
    configuration: Configuration,
) -> tuple[
    frozenset[str],
    frozenset[tuple[str, str]],
    Mapping[str, tuple[str, ...]],
]:
    """Parse Cargo's depth-prefixed package and enabled-feature graph."""

    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise EvidenceError("cargo metadata is missing packages for tree mapping")
    root = _resolve_root_package(metadata, workspace, configuration)
    ids: set[str] = set()
    edges: set[tuple[str, str]] = set()
    features_by_id: dict[str, set[str]] = {}
    stack: dict[int, str] = {}
    line_count = 0
    for raw_line in output.splitlines():
        if not raw_line:
            continue
        line_count += 1
        depth_text, separator, remainder = raw_line.partition("|")
        if separator != "|" or not depth_text.isdecimal():
            raise EvidenceError(f"cargo tree line lacks a numeric depth: {raw_line!r}")
        rendered, separator, feature_text = remainder.rpartition("|")
        if separator != "|" or not rendered:
            raise EvidenceError(f"cargo tree line is malformed: {raw_line!r}")
        depth = int(depth_text)
        package_id = _tree_package_id(rendered, packages)
        if line_count == 1:
            if depth != 0 or package_id != root["id"]:
                raise EvidenceError("cargo tree does not start at the configured root")
        elif depth == 0:
            raise EvidenceError("cargo tree contains more than one root")
        if depth > 0:
            parent = stack.get(depth - 1)
            if parent is None:
                raise EvidenceError(
                    f"cargo tree depth jumps without a parent at depth {depth}"
                )
            edges.add((parent, package_id))
        for stale_depth in [value for value in stack if value >= depth]:
            del stack[stale_depth]
        stack[depth] = package_id
        ids.add(package_id)
        raw_features = [] if not feature_text else feature_text.split(",")
        if any(not feature or "|" in feature for feature in raw_features):
            raise EvidenceError(f"cargo tree feature list is malformed: {feature_text!r}")
        if len(set(raw_features)) != len(raw_features):
            raise EvidenceError(f"cargo tree feature list has duplicates: {feature_text!r}")
        features_by_id.setdefault(package_id, set()).update(raw_features)
    if line_count == 0:
        raise EvidenceError("cargo tree returned no packages")
    if root["id"] not in ids:
        raise EvidenceError("cargo tree omitted the configured root")
    features = {
        package_id: tuple(sorted(values))
        for package_id, values in features_by_id.items()
    }
    return frozenset(ids), frozenset(edges), features


def derive_cargo_tree_closure(
    metadata: Mapping[str, Any],
    lock_path: Path,
    workspace: Path,
    configuration: Configuration,
) -> Closure:
    """Derive the selected normal/build graph and corroborate it with metadata."""

    metadata_closure = derive_closure(
        metadata, lock_path, workspace, configuration
    )
    manifest = workspace.joinpath(*configuration.manifest_path.parts)
    arguments = [
        "cargo",
        "tree",
        "--locked",
        "--quiet",
        "--color",
        "never",
        "--manifest-path",
        str(manifest),
        "--package",
        configuration.package,
        "--target",
        configuration.target,
        "--edges",
        "normal,build",
        "--prefix",
        "depth",
        "--format",
        "|{p}|{f}",
        *feature_arguments(configuration),
    ]
    output = run_command(arguments, cwd=workspace)
    ids, edges, features_by_id = parse_cargo_tree(
        output, metadata, workspace, configuration
    )
    missing_packages = ids - metadata_closure.ids
    missing_edges = edges - metadata_closure.edges
    if missing_packages or missing_edges:
        raise EvidenceError(
            "locked cargo tree is not corroborated by target-filtered metadata: "
            f"packages={sorted(missing_packages)!r}, edges={sorted(missing_edges)!r}"
        )
    nodes = {
        package_id: {
            "deps": [],
            "features": list(features_by_id.get(package_id, ())),
            "id": package_id,
        }
        for package_id in ids
    }
    return Closure(
        root_id=metadata_closure.root_id,
        package_by_id=metadata_closure.package_by_id,
        node_by_id=nodes,
        ids=ids,
        edges=edges,
        edge_kinds={
            edge: metadata_closure.edge_kinds[edge] for edge in edges
        },
        lock_by_id={
            package_id: metadata_closure.lock_by_id[package_id]
            for package_id in ids
        },
        workspace_members=metadata_closure.workspace_members,
    )

def _stable_source(package: Mapping[str, Any], workspace: Path) -> str:
    source = package.get("source")
    if source is not None:
        if not isinstance(source, str) or not source:
            raise EvidenceError(f"package has malformed source: {package.get('id')}")
        return source
    manifest = Path(package["manifest_path"]).resolve()
    relative = manifest.parent.relative_to(workspace.resolve()).as_posix()
    return f"workspace:{relative or '.'}"


def _stable_ref(package: Mapping[str, Any], workspace: Path) -> str:
    source_hash = sha256_bytes(_stable_source(package, workspace).encode("utf-8"))
    name = package["name"]
    version = package["version"]
    return f"urn:latticeaxiom:cargo:v1:{source_hash}:{name}@{version}"


def build_closure_receipt(
    closure: Closure,
    workspace: Path,
    configuration: Configuration,
    lock_sha256: str,
) -> tuple[dict[str, Any], Mapping[str, str]]:
    """Build a path-independent receipt and raw-to-stable reference map."""

    reference_map = {
        package_id: _stable_ref(closure.package_by_id[package_id], workspace)
        for package_id in closure.ids
    }
    packages = []
    for package_id in closure.ids:
        package = closure.package_by_id[package_id]
        node = closure.node_by_id[package_id]
        lock_entry = closure.lock_by_id[package_id]
        packages.append(
            {
                "checksum": lock_entry.get("checksum"),
                "features": sorted(node.get("features", [])),
                "license": package.get("license"),
                "name": package["name"],
                "ref": reference_map[package_id],
                "source": _stable_source(package, workspace),
                "version": package["version"],
                "workspace_member": package_id in closure.workspace_members,
            }
        )
    packages.sort(key=lambda item: item["ref"])
    dependencies = [
        {
            "from": reference_map[source],
            "kinds": list(closure.edge_kinds[(source, target)]),
            "to": reference_map[target],
        }
        for source, target in closure.edges
    ]
    dependencies.sort(key=lambda item: (item["from"], item["to"], item["kinds"]))
    receipt = {
        "cargo_lock_sha256": lock_sha256,
        "configuration": configuration.as_dict(),
        "dependencies": dependencies,
        "graph": {
            "command": "cargo tree --locked --edges normal,build",
            "enabled_feature_source": "cargo tree --format {f}",
            "metadata_corroborated": True,
        },
        "packages": packages,
        "root_ref": reference_map[closure.root_id],
        "schema": 1,
    }
    envelope = {
        "kind": "latticeaxiom.cargo-closure/v1",
        "receipt": receipt,
        "receipt_sha256": sha256_bytes(canonical_json_bytes(receipt)),
    }
    return envelope, reference_map


def _component_map(sbom: Mapping[str, Any]) -> dict[str, Mapping[str, Any]]:
    metadata = sbom.get("metadata")
    if not isinstance(metadata, dict) or not isinstance(metadata.get("component"), dict):
        raise EvidenceError("CycloneDX metadata.component is missing")
    components = sbom.get("components", [])
    if not isinstance(components, list) or not all(
        isinstance(component, dict) for component in components
    ):
        raise EvidenceError("CycloneDX components must be an array of objects")
    all_components = [metadata["component"], *components]
    mapping: dict[str, Mapping[str, Any]] = {}
    for component in all_components:
        reference = component.get("bom-ref")
        if not isinstance(reference, str) or not reference:
            raise EvidenceError("CycloneDX package component lacks bom-ref")
        if reference in mapping:
            raise EvidenceError(f"duplicate CycloneDX package bom-ref: {reference}")
        mapping[reference] = component
    return mapping


def select_sbom_closure(
    raw_sbom: Mapping[str, Any], closure: Closure
) -> dict[str, Any]:
    """Select Cargo's actual tree from the generator's metadata superset."""

    components = _component_map(raw_sbom)
    raw_component_ids = set(components)
    if not set(closure.ids).issubset(raw_component_ids):
        raise EvidenceError(
            "raw CycloneDX omits packages from the locked Cargo tree: "
            f"{sorted(set(closure.ids) - raw_component_ids)!r}"
        )
    if raw_sbom.get("metadata", {}).get("component", {}).get("bom-ref") != closure.root_id:
        raise EvidenceError("raw CycloneDX root differs from the locked Cargo tree")
    dependencies = raw_sbom.get("dependencies")
    if not isinstance(dependencies, list):
        raise EvidenceError("raw CycloneDX dependencies array is missing")
    dependency_by_ref: dict[str, Mapping[str, Any]] = {}
    raw_edges: set[tuple[str, str]] = set()
    for dependency in dependencies:
        if not isinstance(dependency, dict):
            raise EvidenceError("raw CycloneDX dependency entry is not an object")
        source = dependency.get("ref")
        targets = dependency.get("dependsOn", [])
        if (
            not isinstance(source, str)
            or not isinstance(targets, list)
            or not all(isinstance(target, str) for target in targets)
        ):
            raise EvidenceError("raw CycloneDX dependency entry is malformed")
        if source in dependency_by_ref:
            raise EvidenceError(f"duplicate raw CycloneDX dependency ref: {source}")
        dependency_by_ref[source] = dependency
        raw_edges.update((source, target) for target in targets)
    if set(dependency_by_ref) != raw_component_ids:
        raise EvidenceError(
            "raw CycloneDX dependency refs differ from its package components"
        )
    dangling = {
        target for _, target in raw_edges if target not in raw_component_ids
    }
    if dangling:
        raise EvidenceError(
            f"raw CycloneDX dependencies reference unknown packages: {sorted(dangling)!r}"
        )
    if not set(closure.edges).issubset(raw_edges):
        raise EvidenceError(
            "raw CycloneDX omits edges from the locked Cargo tree: "
            f"{sorted(set(closure.edges) - raw_edges)!r}"
        )

    selected = copy.deepcopy(raw_sbom)
    selected["components"] = [
        component
        for component in selected.get("components", [])
        if component.get("bom-ref") in closure.ids
    ]
    targets_by_source: dict[str, list[str]] = {
        package_id: [] for package_id in closure.ids
    }
    for source, target in closure.edges:
        targets_by_source[source].append(target)
    selected_dependencies = []
    for package_id in sorted(closure.ids):
        dependency = copy.deepcopy(dependency_by_ref[package_id])
        dependency["dependsOn"] = sorted(targets_by_source[package_id])
        selected_dependencies.append(dependency)
    selected["dependencies"] = selected_dependencies
    return selected

def validate_sbom_against_closure(
    sbom: Mapping[str, Any],
    closure: Closure,
    configuration: Configuration,
    cyclonedx_version: str,
) -> None:
    """Bidirectionally compare CycloneDX packages, edges, and checksums."""

    if sbom.get("bomFormat") != "CycloneDX":
        raise EvidenceError("SBOM bomFormat is not CycloneDX")
    if sbom.get("specVersion") != "1.5":
        raise EvidenceError(f"SBOM specVersion is not 1.5: {sbom.get('specVersion')!r}")
    if not isinstance(sbom.get("version"), int) or sbom["version"] < 1:
        raise EvidenceError("SBOM version must be a positive integer")
    components = _component_map(sbom)
    component_ids = set(components)
    if component_ids != set(closure.ids):
        raise EvidenceError(
            "SBOM package closure differs from locked metadata: "
            f"missing={sorted(set(closure.ids) - component_ids)!r}, "
            f"extra={sorted(component_ids - set(closure.ids))!r}"
        )
    metadata_component = sbom["metadata"]["component"]
    if metadata_component.get("bom-ref") != closure.root_id:
        raise EvidenceError("SBOM root component does not match configured root package")
    for package_id, component in components.items():
        package = closure.package_by_id[package_id]
        if (component.get("name"), component.get("version")) != (
            package["name"],
            package["version"],
        ):
            raise EvidenceError(f"SBOM identity differs for {package_id}")
        checksum = closure.lock_by_id[package_id].get("checksum")
        if checksum is not None:
            hashes = component.get("hashes", [])
            actual = {
                item.get("content")
                for item in hashes
                if isinstance(item, dict) and item.get("alg") == "SHA-256"
            }
            if actual != {checksum}:
                raise EvidenceError(
                    f"SBOM checksum differs from Cargo.lock for {package_id}: "
                    f"expected {checksum}, got {sorted(value for value in actual if value)!r}"
                )

    dependencies = sbom.get("dependencies")
    if not isinstance(dependencies, list):
        raise EvidenceError("CycloneDX dependencies array is missing")
    dependency_refs: set[str] = set()
    sbom_edges: set[tuple[str, str]] = set()
    for dependency in dependencies:
        if not isinstance(dependency, dict):
            raise EvidenceError("CycloneDX dependency entry is not an object")
        source = dependency.get("ref")
        targets = dependency.get("dependsOn", [])
        if not isinstance(source, str) or not isinstance(targets, list) or not all(
            isinstance(target, str) for target in targets
        ):
            raise EvidenceError("CycloneDX dependency entry is malformed")
        if source in dependency_refs:
            raise EvidenceError(f"duplicate CycloneDX dependency ref: {source}")
        dependency_refs.add(source)
        sbom_edges.update((source, target) for target in targets)
    if dependency_refs != set(closure.ids):
        raise EvidenceError(
            "SBOM dependency ref set differs from locked metadata: "
            f"missing={sorted(set(closure.ids) - dependency_refs)!r}, "
            f"extra={sorted(dependency_refs - set(closure.ids))!r}"
        )
    if sbom_edges != set(closure.edges):
        raise EvidenceError(
            "SBOM dependency edges differ from locked metadata: "
            f"missing={sorted(set(closure.edges) - sbom_edges)!r}, "
            f"extra={sorted(sbom_edges - set(closure.edges))!r}"
        )

    properties = sbom.get("metadata", {}).get("properties", [])
    target_values = {
        item.get("value")
        for item in properties
        if isinstance(item, dict) and item.get("name") == "cdx:rustc:sbom:target:triple"
    }
    if target_values != {configuration.target}:
        raise EvidenceError(
            f"SBOM target property differs: expected {configuration.target}, "
            f"got {sorted(value for value in target_values if value)!r}"
        )
    tools = sbom.get("metadata", {}).get("tools", [])
    tool_versions = {
        tool.get("version")
        for tool in tools
        if isinstance(tool, dict) and tool.get("name") == "cargo-cyclonedx"
    }
    if tool_versions != {cyclonedx_version}:
        raise EvidenceError(
            f"SBOM generator version differs: expected {cyclonedx_version}, "
            f"got {sorted(value for value in tool_versions if value)!r}"
        )


def _replace_reference(value: str, reference_map: Mapping[str, str]) -> str:
    for raw in sorted(reference_map, key=len, reverse=True):
        if value == raw:
            return reference_map[raw]
        if value.startswith(raw + " "):
            return reference_map[raw] + value[len(raw) :]
    return value


def _rewrite_references(value: Any, reference_map: Mapping[str, str]) -> Any:
    if isinstance(value, list):
        return [_rewrite_references(item, reference_map) for item in value]
    if not isinstance(value, dict):
        return value
    raw_reference = value.get("bom-ref")
    is_workspace_component = False
    if isinstance(raw_reference, str):
        for package_id in reference_map:
            if raw_reference == package_id or raw_reference.startswith(package_id + " "):
                if package_id.startswith("path+file:"):
                    is_workspace_component = True
                break
    rewritten: dict[str, Any] = {}
    for key, item in value.items():
        if key == "purl" and is_workspace_component:
            continue
        if key in {"bom-ref", "ref"} and isinstance(item, str):
            rewritten[key] = _replace_reference(item, reference_map)
        elif key == "dependsOn" and isinstance(item, list):
            rewritten[key] = [
                _replace_reference(reference, reference_map) for reference in item
            ]
        else:
            rewritten[key] = _rewrite_references(item, reference_map)
    return rewritten


def _sort_semantic_arrays(value: Any, parent_key: str | None = None) -> Any:
    if isinstance(value, dict):
        return {
            key: _sort_semantic_arrays(item, key)
            for key, item in value.items()
        }
    if not isinstance(value, list):
        return value
    normalized = [_sort_semantic_arrays(item, parent_key) for item in value]
    if all(isinstance(item, (str, int, float, bool)) or item is None for item in normalized):
        return sorted(normalized, key=lambda item: json.dumps(item, sort_keys=True))
    if parent_key in {"components", "dependencies"}:
        key_name = "bom-ref" if parent_key == "components" else "ref"
        return sorted(normalized, key=lambda item: item.get(key_name, ""))
    if parent_key == "properties":
        return sorted(normalized, key=lambda item: (item.get("name", ""), item.get("value", "")))
    if parent_key == "hashes":
        return sorted(normalized, key=lambda item: (item.get("alg", ""), item.get("content", "")))
    if parent_key == "externalReferences":
        return sorted(normalized, key=lambda item: (item.get("type", ""), item.get("url", "")))
    if parent_key == "tools":
        return sorted(
            normalized,
            key=lambda item: (
                item.get("vendor", ""),
                item.get("name", ""),
                item.get("version", ""),
            ),
        )
    if parent_key == "licenses":
        return sorted(normalized, key=lambda item: json.dumps(item, sort_keys=True))
    return normalized


def normalize_sbom(
    sbom: Mapping[str, Any],
    reference_map: Mapping[str, str],
    configuration: Configuration,
    lock_sha256: str,
    closure_sha256: str,
    workspace: Path,
) -> dict[str, Any]:
    """Remove nondeterminism and attach the explicit build selection."""

    normalized = _rewrite_references(copy.deepcopy(sbom), reference_map)
    normalized.pop("serialNumber", None)
    normalized["$schema"] = SCHEMA_URL
    metadata = normalized.get("metadata")
    if not isinstance(metadata, dict):
        raise EvidenceError("normalized SBOM lost metadata")
    metadata.pop("timestamp", None)
    properties = metadata.setdefault("properties", [])
    if not isinstance(properties, list) or not all(
        isinstance(item, dict) for item in properties
    ):
        raise EvidenceError("CycloneDX metadata properties are malformed")
    by_name: dict[str, str] = {}
    for item in properties:
        name = item.get("name")
        value = item.get("value")
        if not isinstance(name, str) or not isinstance(value, str):
            raise EvidenceError("CycloneDX property must contain string name and value")
        if name in by_name:
            raise EvidenceError(f"duplicate CycloneDX property: {name}")
        by_name[name] = value
    additions = {
        "latticeaxiom:release-evidence:cargo-lock-sha256": lock_sha256,
        "latticeaxiom:release-evidence:closure-sha256": closure_sha256,
        "latticeaxiom:release-evidence:content-assets-included": "false",
        "latticeaxiom:release-evidence:cargo-graph": "tree:normal,build;metadata-corroborated",
        "latticeaxiom:release-evidence:default-features": str(
            configuration.default_features
        ).lower(),
        "latticeaxiom:release-evidence:features": json.dumps(
            list(configuration.features), separators=(",", ":")
        ),
        "latticeaxiom:release-evidence:package": configuration.package,
        "latticeaxiom:release-evidence:profile": configuration.profile,
        "latticeaxiom:release-evidence:target": configuration.target,
    }
    overlap = set(by_name) & set(additions)
    if overlap:
        raise EvidenceError(
            f"generator unexpectedly emitted reserved properties: {sorted(overlap)!r}"
        )
    properties.extend(
        {"name": name, "value": value} for name, value in additions.items()
    )
    normalized = _sort_semantic_arrays(normalized)
    rendered = canonical_json_bytes(normalized).decode("utf-8")
    forbidden = {
        workspace.resolve().as_posix().lower(),
        str(workspace.resolve()).replace("\\", "/").lower(),
        "path+file:",
    }
    rendered_lower = rendered.replace("\\", "/").lower()
    leaks = sorted(item for item in forbidden if item and item in rendered_lower)
    if leaks:
        raise EvidenceError(f"normalized SBOM contains machine-local path data: {leaks!r}")
    return normalized


def validate_and_render_notices(
    about: Mapping[str, Any],
    closure: Closure,
    workspace: Path,
    reference_map: Mapping[str, str],
    configuration: Configuration,
    lock_sha256: str,
) -> bytes:
    """Validate cargo-about attribution coverage and render stable notices."""

    crates = about.get("crates")
    licenses = about.get("licenses")
    if not isinstance(crates, list) or not isinstance(licenses, list):
        raise EvidenceError("cargo-about JSON lacks crates or licenses arrays")
    crate_by_id: dict[str, Mapping[str, Any]] = {}
    license_expression_by_id: dict[str, str] = {}
    for item in crates:
        if not isinstance(item, dict) or not isinstance(item.get("package"), dict):
            raise EvidenceError("cargo-about crate entry is malformed")
        package = item["package"]
        package_id = package.get("id")
        license_expression = item.get("license")
        if not isinstance(package_id, str) or not package_id:
            raise EvidenceError("cargo-about package lacks an id")
        if package_id in crate_by_id:
            raise EvidenceError(f"cargo-about package is duplicated: {package_id}")
        if (
            not isinstance(license_expression, str)
            or not license_expression.strip()
            or license_expression in {"Unknown", "Ignore"}
        ):
            raise EvidenceError(
                f"cargo-about returned an unresolved license for {package_id}: "
                f"{license_expression!r}"
            )
        crate_by_id[package_id] = package
        license_expression_by_id[package_id] = license_expression
    if set(crate_by_id) != set(closure.ids):
        raise EvidenceError(
            "cargo-about package closure differs from locked metadata: "
            f"missing={sorted(set(closure.ids) - set(crate_by_id))!r}, "
            f"extra={sorted(set(crate_by_id) - set(closure.ids))!r}"
        )

    third_party_ids = {
        package_id
        for package_id in closure.ids
        if closure.package_by_id[package_id].get("source") is not None
    }
    attribution_by_package: dict[str, list[tuple[str, str]]] = {
        package_id: [] for package_id in third_party_ids
    }
    rendered_licenses: list[dict[str, Any]] = []
    for item in licenses:
        if not isinstance(item, dict):
            raise EvidenceError("cargo-about license entry is malformed")
        identifier = item.get("id")
        name = item.get("name")
        text = item.get("text")
        used_by = item.get("used_by")
        if (
            not isinstance(identifier, str)
            or not identifier
            or identifier in {"Unknown", "Ignore"}
            or not isinstance(name, str)
            or not name
            or not isinstance(text, str)
            or not text.strip()
            or not isinstance(used_by, list)
        ):
            raise EvidenceError(f"cargo-about license entry is incomplete: {identifier!r}")
        normalized_text = text.replace("\r\n", "\n").replace("\r", "\n").rstrip("\n")
        text_hash = sha256_bytes((normalized_text + "\n").encode("utf-8"))
        users: set[str] = set()
        for use in used_by:
            if not isinstance(use, dict) or not isinstance(use.get("crate"), dict):
                raise EvidenceError("cargo-about used_by entry is malformed")
            package_id = use["crate"].get("id")
            if package_id not in closure.ids:
                raise EvidenceError(
                    f"cargo-about attribution references out-of-closure package: {package_id!r}"
                )
            if package_id in third_party_ids:
                users.add(package_id)
                attribution_by_package[package_id].append((identifier, text_hash))
        if users:
            rendered_licenses.append(
                {
                    "id": identifier,
                    "name": name,
                    "packages": sorted(users, key=lambda package_id: reference_map[package_id]),
                    "text": normalized_text,
                    "text_hash": text_hash,
                }
            )
    missing = sorted(
        package_id
        for package_id, attributions in attribution_by_package.items()
        if not attributions
    )
    if missing:
        raise EvidenceError(
            f"third-party packages lack license-text attribution: {missing!r}"
        )

    package_records = []
    for package_id in third_party_ids:
        package = closure.package_by_id[package_id]
        attributions = sorted(set(attribution_by_package[package_id]))
        package_records.append(
            {
                "attributions": attributions,
                "license": license_expression_by_id[package_id],
                "name": package["name"],
                "ref": reference_map[package_id],
                "source": _stable_source(package, workspace),
                "version": package["version"],
            }
        )
    package_records.sort(key=lambda item: item["ref"])
    rendered_licenses.sort(
        key=lambda item: (
            item["id"],
            item["text_hash"],
            tuple(reference_map[package_id] for package_id in item["packages"]),
        )
    )

    lines = [
        "Lattice Axiom third-party notices",
        "===================================",
        "",
        f"Configuration: {configuration.identifier}",
        f"Target: {configuration.target}",
        f"Profile: {configuration.profile}",
        f"Root package: {configuration.package}",
        f"Default features: {str(configuration.default_features).lower()}",
        f"Features: {json.dumps(list(configuration.features), separators=(',', ':'))}",
        f"Cargo.lock SHA-256: {lock_sha256}",
        "Scope: Cargo dependency closure only; content assets are not included.",
        "",
        "Third-party packages",
        "--------------------",
        "",
    ]
    for record in package_records:
        attribution_text = ", ".join(
            f"{identifier} [{text_hash}]"
            for identifier, text_hash in record["attributions"]
        )
        lines.extend(
            [
                f"- {record['name']} {record['version']}",
                f"  Source: {record['source']}",
                f"  Resolved license: {record['license']}",
                f"  Attribution: {attribution_text}",
            ]
        )
    lines.extend(["", "License texts", "-------------", ""])
    for item in rendered_licenses:
        used_by = ", ".join(
            f"{closure.package_by_id[package_id]['name']}@"
            f"{closure.package_by_id[package_id]['version']}"
            for package_id in item["packages"]
        )
        lines.extend(
            [
                "=" * 79,
                f"{item['id']} — {item['name']}",
                f"Text SHA-256: {item['text_hash']}",
                f"Used by: {used_by}",
                "=" * 79,
                item["text"],
                "",
            ]
        )
    return ("\n".join(lines).rstrip("\n") + "\n").encode("utf-8")


def _copy_workspace_snapshot(source: Path, destination: Path) -> None:
    ignored_names = {".git", "target", ".codex", ".agents"}

    def ignore(directory: str, names: list[str]) -> set[str]:
        del directory
        return {name for name in names if name in ignored_names}

    shutil.copytree(source, destination, symlinks=True, ignore=ignore)


def _generate_raw_sbom(
    snapshot: Path,
    configuration: Configuration,
    version: str,
) -> Mapping[str, Any]:
    manifest = snapshot.joinpath(*configuration.manifest_path.parts)
    basename = "latticeaxiom-release-evidence-raw"
    arguments = [
        "cargo",
        "cyclonedx",
        "--manifest-path",
        str(manifest),
        "--format",
        "json",
        "--spec-version",
        "1.5",
        "--target",
        configuration.target,
        *feature_arguments(configuration),
        "--override-filename",
        basename,
        "--license-strict",
        "--quiet",
        "--quiet",
    ]
    run_command(
        arguments,
        cwd=snapshot,
        environment={"CARGO_NET_OFFLINE": "true", "SOURCE_DATE_EPOCH": "0"},
    )
    output = manifest.parent / f"{basename}.json"
    if not output.is_file():
        raise EvidenceError(
            f"cargo-cyclonedx {version} did not create the expected root SBOM: {output}"
        )
    try:
        sbom = json.loads(output.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read generated CycloneDX JSON: {error}") from error
    if not isinstance(sbom, dict):
        raise EvidenceError("generated CycloneDX document is not an object")
    return sbom


def _generate_raw_about(
    snapshot: Path, configuration: Configuration
) -> Mapping[str, Any]:
    manifest = snapshot.joinpath(*configuration.manifest_path.parts)
    config = snapshot / ABOUT_CONFIG_PATH
    output = snapshot / "cargo-about-raw.json"
    arguments = [
        "cargo",
        "about",
        "generate",
        "--config",
        str(config),
        "--format",
        "json",
        "--output-file",
        str(output),
        "--target",
        configuration.target,
        "--frozen",
        "--manifest-path",
        str(manifest),
        "--fail",
        *feature_arguments(configuration, separator=" "),
    ]
    run_command(arguments, cwd=snapshot)
    try:
        about = json.loads(output.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read cargo-about JSON: {error}") from error
    if not isinstance(about, dict):
        raise EvidenceError("cargo-about output is not an object")
    return about


def _write_evidence(
    output_dir: Path,
    configuration: Configuration,
    sbom_bytes: bytes,
    notices_bytes: bytes,
    closure_bytes: bytes,
    contract: Contract,
    lock_sha256: str,
    closure_sha256: str,
) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    if any(output_dir.iterdir()):
        raise EvidenceError(f"output directory must be empty: {output_dir}")
    filenames = {
        "sbom": f"latticeaxiom-{configuration.identifier}.cdx.json",
        "notices": f"THIRD_PARTY_NOTICES-{configuration.identifier}.txt",
        "closure": f"cargo-closure-{configuration.identifier}.json",
    }
    payloads = {
        filenames["sbom"]: (sbom_bytes, "application/vnd.cyclonedx+json"),
        filenames["notices"]: (notices_bytes, "text/plain; charset=utf-8"),
        filenames["closure"]: (closure_bytes, "application/json"),
    }
    files = []
    for name in sorted(payloads):
        payload, media_type = payloads[name]
        (output_dir / name).write_bytes(payload)
        files.append(
            {
                "bytes": len(payload),
                "media_type": media_type,
                "name": name,
                "sha256": sha256_bytes(payload),
            }
        )
    manifest = {
        "binary_included": False,
        "build_receipt_included": False,
        "cargo_lock_sha256": lock_sha256,
        "configuration": configuration.as_dict(),
        "content_assets_included": False,
        "files": files,
        "kind": EVIDENCE_KIND,
        "normalized_cargo_closure_sha256": closure_sha256,
        "tools": dict(sorted(contract.tools.items())),
    }
    manifest_bytes = canonical_json_bytes(manifest)
    manifest_name = f"release-evidence-manifest-{configuration.identifier}.json"
    (output_dir / manifest_name).write_bytes(manifest_bytes)
    manifest_hash = sha256_bytes(manifest_bytes)
    (output_dir / f"release-evidence-manifest-{configuration.identifier}.sha256").write_text(
        f"{manifest_hash}  {manifest_name}\n", encoding="utf-8", newline="\n"
    )
    verify_evidence_directory(output_dir)


def verify_evidence_directory(directory: Path) -> None:
    """Verify the non-circular evidence manifest and its SHA-256 sidecar."""

    manifest_paths = sorted(directory.glob("release-evidence-manifest-*.json"))
    sidecar_paths = sorted(directory.glob("release-evidence-manifest-*.sha256"))
    if len(manifest_paths) != 1 or len(sidecar_paths) != 1:
        raise EvidenceError("evidence directory must contain one manifest and one sidecar")
    manifest_path = manifest_paths[0]
    sidecar_path = sidecar_paths[0]
    try:
        manifest_bytes = manifest_path.read_bytes()
        manifest = json.loads(manifest_bytes)
        sidecar = sidecar_path.read_text(encoding="utf-8")
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read evidence manifest: {error}") from error
    if not isinstance(manifest, dict) or manifest.get("kind") != EVIDENCE_KIND:
        raise EvidenceError("evidence manifest kind is invalid")
    expected_sidecar = f"{sha256_bytes(manifest_bytes)}  {manifest_path.name}\n"
    if sidecar != expected_sidecar:
        raise EvidenceError("release-evidence manifest SHA-256 sidecar differs")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise EvidenceError("evidence manifest has no files")
    names: set[str] = set()
    for item in files:
        if not isinstance(item, dict):
            raise EvidenceError("evidence manifest file entry is malformed")
        _require_exact_keys(item, {"bytes", "media_type", "name", "sha256"}, "file")
        name = item["name"]
        if (
            not isinstance(name, str)
            or Path(name).name != name
            or name in names
        ):
            raise EvidenceError(f"unsafe or duplicate evidence filename: {name!r}")
        names.add(name)
        path = directory / name
        if path.stat().st_size != item["bytes"]:
            raise EvidenceError(f"evidence file size differs: {name}")
        if sha256_file(path) != item["sha256"]:
            raise EvidenceError(f"evidence file hash differs: {name}")
    expected_names = {path.name for path in directory.iterdir() if path.is_file()}
    expected_names -= {manifest_path.name, sidecar_path.name}
    if names != expected_names:
        raise EvidenceError(
            f"manifest file coverage differs: missing={sorted(expected_names - names)!r}, "
            f"extra={sorted(names - expected_names)!r}"
        )


def smoke(workspace: Path, contract: Contract) -> None:
    """Check pins, locked metadata, lock matching, and workspace licenses."""

    check_tool_version(
        ["cargo", "cyclonedx", "--version"],
        contract.tools["cargo-cyclonedx"],
        cwd=workspace,
    )
    check_tool_version(
        ["cargo", "about", "--version"],
        contract.tools["cargo-about"],
        cwd=workspace,
    )
    check_tool_version(
        ["cargo", "deny", "--version"],
        contract.tools["cargo-deny"],
        cwd=workspace,
    )
    lock_path = workspace / "Cargo.lock"
    for configuration in contract.configurations:
        metadata = cargo_metadata(workspace, configuration)
        validate_about_workspace_policy(metadata, workspace / ABOUT_CONFIG_PATH)
        closure = derive_cargo_tree_closure(
            metadata, lock_path, workspace, configuration
        )
        print(
            f"validated {configuration.identifier}: "
            f"{len(closure.ids)} packages, {len(closure.edges)} dependency edges"
        )


def generate(
    workspace: Path,
    contract: Contract,
    configuration: Configuration,
    output_dir: Path,
) -> None:
    """Generate one fully validated deterministic release-evidence set."""

    cyclonedx_version = check_tool_version(
        ["cargo", "cyclonedx", "--version"],
        contract.tools["cargo-cyclonedx"],
        cwd=workspace,
    )
    check_tool_version(
        ["cargo", "about", "--version"],
        contract.tools["cargo-about"],
        cwd=workspace,
    )
    check_tool_version(
        ["cargo", "deny", "--version"],
        contract.tools["cargo-deny"],
        cwd=workspace,
    )
    run_command(["cargo", "deny", "--locked", "check"], cwd=workspace)

    source_lock = workspace / "Cargo.lock"
    source_lock_hash = sha256_file(source_lock)
    source_metadata = cargo_metadata(workspace, configuration)
    validate_about_workspace_policy(source_metadata, workspace / ABOUT_CONFIG_PATH)
    derive_closure(source_metadata, source_lock, workspace, configuration)
    with tempfile.TemporaryDirectory(prefix="latticeaxiom-release-evidence-") as temp:
        snapshot = Path(temp) / "workspace"
        _copy_workspace_snapshot(workspace, snapshot)
        snapshot_lock = snapshot / "Cargo.lock"
        if sha256_file(snapshot_lock) != source_lock_hash:
            raise EvidenceError("Cargo.lock changed while creating the workspace snapshot")
        raw_about = _generate_raw_about(snapshot, configuration)
        if sha256_file(snapshot_lock) != source_lock_hash:
            raise EvidenceError("cargo-about changed the exact Cargo.lock snapshot")
        metadata = cargo_metadata(snapshot, configuration)
        closure = derive_cargo_tree_closure(
            metadata, snapshot_lock, snapshot, configuration
        )
        closure_receipt, reference_map = build_closure_receipt(
            closure, snapshot, configuration, source_lock_hash
        )
        closure_bytes = canonical_json_bytes(closure_receipt)
        closure_hash = sha256_bytes(closure_bytes)
        raw_sbom = _generate_raw_sbom(snapshot, configuration, cyclonedx_version)
        if sha256_file(snapshot_lock) != source_lock_hash:
            raise EvidenceError("cargo-cyclonedx changed the exact Cargo.lock snapshot")
        selected_sbom = select_sbom_closure(raw_sbom, closure)
        validate_sbom_against_closure(
            selected_sbom, closure, configuration, cyclonedx_version
        )
        normalized_sbom = normalize_sbom(
            selected_sbom,
            reference_map,
            configuration,
            source_lock_hash,
            closure_hash,
            snapshot,
        )
        sbom_bytes = canonical_json_bytes(normalized_sbom)
        notices_bytes = validate_and_render_notices(
            raw_about,
            closure,
            snapshot,
            reference_map,
            configuration,
            source_lock_hash,
        )
    if sha256_file(source_lock) != source_lock_hash:
        raise EvidenceError("source Cargo.lock changed during evidence generation")
    _write_evidence(
        output_dir,
        configuration,
        sbom_bytes,
        notices_bytes,
        closure_bytes,
        contract,
        source_lock_hash,
        closure_hash,
    )
    print(f"generated and verified release evidence in {output_dir}")


def workspace_root() -> Path:
    """Resolve the repository root from this script's stable location."""

    return Path(__file__).resolve().parents[2]


def parse_arguments(arguments: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("smoke", help="validate pins and locked closure inputs")
    generate_parser = subparsers.add_parser(
        "generate", help="generate one shipped release-evidence set"
    )
    generate_parser.add_argument("--configuration", required=True)
    generate_parser.add_argument("--output-dir", required=True, type=Path)
    verify_parser = subparsers.add_parser(
        "verify", help="verify hashes and coverage in an evidence directory"
    )
    verify_parser.add_argument("--evidence-dir", required=True, type=Path)
    return parser.parse_args(arguments)


def main(arguments: Sequence[str] | None = None) -> int:
    """CLI entry point."""

    parsed = parse_arguments(sys.argv[1:] if arguments is None else arguments)
    root = workspace_root()
    try:
        if parsed.command == "verify":
            verify_evidence_directory(parsed.evidence_dir.resolve())
            print(f"verified release evidence in {parsed.evidence_dir.resolve()}")
            return 0
        contract = load_contract(root / CONTRACT_PATH)
        if parsed.command == "smoke":
            smoke(root, contract)
            return 0
        configuration = contract.configuration(parsed.configuration)
        output_dir = parsed.output_dir
        if not output_dir.is_absolute():
            output_dir = root / output_dir
        generate(root, contract, configuration, output_dir.resolve())
        return 0
    except (EvidenceError, OSError, subprocess.TimeoutExpired) as error:
        print(f"release evidence failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
