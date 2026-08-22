#!/usr/bin/env python3
"""v1 locked dependency, advisory-exception, and workspace license gate.

This is the v1 reproduction entry for cargo-deny plus the temporary advisory
exception policy. It does not fetch, publish, or generate SBOMs.
"""

from __future__ import annotations

import argparse
from datetime import date
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from typing import Any, Mapping, Sequence


EXPECTED_WORKSPACE_LICENSE = "AGPL-3.0-only"
ADVISORY_ID = re.compile(r"RUSTSEC-[0-9]{4}-[0-9]{4}\Z")
CARGO_DENY_VERSION = "0.20.2"
EXCEPTION_FIELDS = {
    "package",
    "owner",
    "added",
    "expires",
    "mitigation",
    "issue",
    "replace-when",
}


class PolicyError(RuntimeError):
    """Raised when the v1 dependency or license policy fails closed."""


def workspace_root() -> Path:
    """Resolve the repository root from this script location."""

    return Path(__file__).resolve().parents[2]


def run_command(arguments: Sequence[str], *, cwd: Path) -> str:
    """Run a bounded command and return stdout."""

    completed = subprocess.run(
        list(arguments),
        cwd=cwd,
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
        raise PolicyError(
            f"command failed with exit code {completed.returncode}: "
            f"{' '.join(arguments)}\n{output}"
        )
    return completed.stdout


def cargo_metadata(workspace: Path) -> dict[str, Any]:
    """Read locked all-feature workspace metadata."""

    output = run_command(
        [
            "cargo",
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--all-features",
        ],
        cwd=workspace,
    )
    try:
        metadata = json.loads(output)
    except json.JSONDecodeError as error:
        raise PolicyError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise PolicyError("cargo metadata is not an object")
    return metadata


def validate_path_dependencies(metadata: Mapping[str, Any], workspace: Path) -> None:
    """Reject path dependencies that escape the workspace snapshot."""

    workspace_root = workspace.resolve()
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise PolicyError("cargo metadata lacks packages")
    for package in packages:
        if package.get("source") is not None:
            continue
        try:
            manifest = Path(package["manifest_path"]).resolve()
        except (KeyError, TypeError, OSError) as error:
            raise PolicyError("path dependency manifest is malformed") from error
        if not manifest.is_relative_to(workspace_root):
            raise PolicyError(f"path dependency escapes workspace: {manifest}")


def parse_exception_fields(reason: str, advisory_id: str) -> dict[str, str]:
    """Parse the machine-readable advisory exception reason."""

    fields: dict[str, str] = {}
    for field in reason.split("; "):
        key, separator, value = field.partition("=")
        if not separator or not key or not value or key in fields:
            raise PolicyError(f"{advisory_id}: malformed exception field {field!r}")
        fields[key] = value
    missing = sorted(EXCEPTION_FIELDS - fields.keys())
    extra = sorted(fields.keys() - EXCEPTION_FIELDS)
    if missing or extra:
        raise PolicyError(f"{advisory_id}: missing={missing!r}, extra={extra!r}")
    return fields


def validate_advisory_exceptions(
    policy: Mapping[str, Any],
    locked_packages: set[str],
    *,
    today: date,
) -> int:
    """Enforce the 30-day, exact-package, fail-closed advisory exception policy."""

    seen: set[str] = set()
    exceptions = policy.get("advisories", {}).get("ignore", [])
    if not isinstance(exceptions, list):
        raise PolicyError("deny.toml advisories.ignore must be an array")
    for exception in exceptions:
        if not isinstance(exception, dict):
            raise PolicyError("advisory exception must be a table")
        advisory_id = exception.get("id", "")
        if not isinstance(advisory_id, str) or not ADVISORY_ID.fullmatch(advisory_id):
            raise PolicyError(f"invalid advisory exception ID: {advisory_id!r}")
        if advisory_id in seen:
            raise PolicyError(f"duplicate advisory exception: {advisory_id}")
        seen.add(advisory_id)
        reason = exception.get("reason", "")
        if not isinstance(reason, str):
            raise PolicyError(f"{advisory_id}: reason must be a string")
        fields = parse_exception_fields(reason, advisory_id)
        if fields["package"] not in locked_packages:
            raise PolicyError(
                f"{advisory_id}: package is not in locked metadata: {fields['package']}"
            )
        added = date.fromisoformat(fields["added"])
        expires = date.fromisoformat(fields["expires"])
        if not 0 < (expires - added).days <= 30:
            raise PolicyError(f"{advisory_id}: exception exceeds 30 days")
        if today < added or today >= expires:
            raise PolicyError(
                f"{advisory_id}: exception is not active on {today.isoformat()}"
            )
        if fields["owner"] != "core-runtime-maintainer":
            raise PolicyError(f"{advisory_id}: unexpected exception owner")
        if not fields["issue"].startswith("https://"):
            raise PolicyError(f"{advisory_id}: issue must be an HTTPS URL")
    return len(seen)


def validate_workspace_licenses(metadata: Mapping[str, Any]) -> None:
    """Require every workspace package to declare AGPL-3.0-only."""

    packages = metadata.get("packages")
    members = metadata.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(members, list):
        raise PolicyError("cargo metadata lacks workspace members")
    package_by_id = {package.get("id"): package for package in packages}
    for member_id in members:
        package = package_by_id.get(member_id)
        if not isinstance(package, dict):
            raise PolicyError(f"workspace member is missing from packages: {member_id}")
        license_expression = package.get("license")
        if license_expression != EXPECTED_WORKSPACE_LICENSE:
            raise PolicyError(
                f"workspace package {package.get('name')}@{package.get('version')} "
                f"must declare {EXPECTED_WORKSPACE_LICENSE}; got {license_expression!r}"
            )


def load_deny_policy(path: Path) -> dict[str, Any]:
    """Load deny.toml."""

    try:
        policy = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"cannot read deny.toml: {error}") from error
    if not isinstance(policy, dict):
        raise PolicyError("deny.toml must be a table")
    return policy


def locked_package_set(metadata: Mapping[str, Any]) -> set[str]:
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise PolicyError("cargo metadata lacks packages")
    locked = set()
    for package in packages:
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            raise PolicyError("cargo metadata package identity is malformed")
        locked.add(f"{name}@{version}")
    return locked


def check_cargo_deny_version(workspace: Path) -> None:
    """Require the exact cargo-deny pin used by the v1 fixture."""

    output = run_command(["cargo", "deny", "--version"], cwd=workspace).strip()
    if CARGO_DENY_VERSION not in output:
        raise PolicyError(
            f"cargo-deny version mismatch: expected {CARGO_DENY_VERSION}, got {output!r}"
        )


def check(workspace: Path, *, today: date | None = None) -> int:
    """Run the complete v1 dependency and license gate."""

    check_cargo_deny_version(workspace)
    metadata = cargo_metadata(workspace)
    validate_path_dependencies(metadata, workspace)
    validate_workspace_licenses(metadata)
    policy = load_deny_policy(workspace / "deny.toml")
    count = validate_advisory_exceptions(
        policy,
        locked_package_set(metadata),
        today=today or date.today(),
    )
    run_command(["cargo", "deny", "--locked", "check"], cwd=workspace)
    print(f"validated {count} temporary advisory exceptions and locked license policy")
    return count


def parse_arguments(arguments: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=("check",),
        help="run locked metadata, exception, license, and cargo-deny checks",
    )
    return parser.parse_args(arguments)


def main(arguments: Sequence[str] | None = None) -> int:
    parse_arguments(sys.argv[1:] if arguments is None else arguments)
    try:
        check(workspace_root())
        return 0
    except (PolicyError, OSError, subprocess.TimeoutExpired) as error:
        print(f"v1 dependency/license failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
