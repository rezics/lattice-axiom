#!/usr/bin/env python3
"""Reproduce the v1 release gate from documented commands.

This harness records actual command outcomes. File existence, fixture
presence, and skipped jobs are never recorded as pass. It does not fetch,
publish, push, or write commit-bound documentation evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
from typing import Any, Mapping, Sequence


CONTRACT_PATH = Path("fixtures/release/v1-reproduction.json")
SCHEMA_ID = "latticeaxiom.v1-release-reproduction.v1"
EVIDENCE_KIND = "latticeaxiom.v1-release-reproduction-report/v1"
PROFILES = frozenset({"ci-headless", "local-release", "reference-host"})
FORBIDDEN_TOKENS = (
    "cargo fetch",
    "cargo publish",
    "git push",
    "git commit",
    "gh release",
    "docs/delivery/evidence",
)
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}\Z")

# Exact ADR 0026 first-version numbers. Loosening any value is a contract fault.
FROZEN_ADR_0026_BUDGETS: dict[str, Any] = {
    "profile": "desktop-reference-v1",
    "status": "d2-provisional",
    "chunk_edge_voxels": 32,
    "frame_p95_ms": 16.67,
    "frame_p99_ms": 25,
    "long_frame_ms": 50,
    "long_frame_count_max": 3,
    "fixed_hz": 60,
    "fixed_interval_ms": 16.67,
    "fixed_update_p95_ms": 8,
    "fixed_update_p99_ms": 12,
    "catch_up_ticks_max": 2,
    "physics_p95_ms": 3,
    "physics_p99_ms": 5,
    "ram_gib": 4,
    "vram_gib": 3,
    "active_chunks": 405,
    "resident_chunks": 1183,
    "visible_chunks": 512,
    "requested_chunks": 1536,
    "active_radius_chunks": 4,
    "resident_radius_chunks": 6,
    "active_vertical_radius_chunks": 2,
    "resident_vertical_radius_chunks": 3,
    "active_coverage_m": 128,
    "resident_coverage_m": 192,
    "apply_cpu_ms": 2,
    "apply_bytes_mib": 16,
    "soft_high_water_percent": 75,
    "queues": {
        "world_generation": {"jobs": 128, "bytes_mib": 128},
        "mesh": {"jobs": 128, "bytes_mib": 128},
        "collider": {"jobs": 64, "bytes_mib": 64},
        "persistence": {"jobs": 64, "bytes_mib": 64},
        "other_tasks": {"jobs": 128, "bytes_mib": 64},
        "combined": {"jobs": 512, "bytes_mib": 384},
    },
    "latencies_ms": {
        "edit_to_visible": {"p95": 100, "p99": 200},
        "collider_rebuild": {"p95": 150, "p99": 300},
        "warm_chunk_load": {"p95": 50, "p99": 100},
        "cold_chunk_load": {"p95": 250, "p99": 500},
        "world_activation": {"p95": 2000, "p99": 3000},
        "save_written": {"p95": 250, "p99": 500},
        "save_durable": {"p95": 1000, "p99": 2000},
        "checkpoint": {"p95": 5000, "p99": 10000},
    },
    "dynamic_tick": {
        "host_module_callbacks_soft": 64,
        "host_module_callbacks_hard": 128,
        "abi_calls_soft": 256,
        "abi_calls_hard": 512,
        "command_count_soft": 4096,
        "command_count_hard": 16384,
        "command_bytes_soft_kib": 256,
        "command_bytes_hard_mib": 1,
        "bridge_p95_ms": 1,
    },
}

CONTRACT_KEYS = {
    "schema_id",
    "schema_version",
    "kind",
    "product_entry",
    "supervisor_bin",
    "engine_bin",
    "compose_bin",
    "linux_is_independent_root_gate",
    "windows_rustc_crash_policy",
    "claims_d10_frozen",
    "claims_d2_working_set_gate",
    "content_assets_included",
    "commit_bound_docs_evidence",
    "forbidden_operations",
    "tools",
    "actions",
    "matrix",
    "artifact_normalization",
    "budgets",
    "instructions",
    "jobs",
}


class ReproductionError(RuntimeError):
    """Raised when the v1 reproduction contract or a gate command fails closed."""


def canonical_json_bytes(value: Any) -> bytes:
    """Serialize JSON with stable keys, whitespace, UTF-8, and LF."""

    text = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)
    return (text + "\n").encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    """Return a lowercase SHA-256 digest."""

    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    """Hash one regular file."""

    if not path.is_file():
        raise ReproductionError(f"expected regular file: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_exact_keys(value: Mapping[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        raise ReproductionError(
            f"{context} keys differ: missing={sorted(expected - actual)!r}, "
            f"extra={sorted(actual - expected)!r}"
        )


def workspace_root() -> Path:
    """Resolve the repository root from this script location."""

    return Path(__file__).resolve().parents[2]


def host_matrix_entry(contract: Mapping[str, Any], identifier: str | None) -> dict[str, Any]:
    """Select the Linux or Windows matrix row for this host."""

    rows = contract["matrix"]
    if identifier:
        matches = [row for row in rows if row["id"] == identifier]
        if len(matches) != 1:
            available = ", ".join(row["id"] for row in rows)
            raise ReproductionError(
                f"unknown matrix id {identifier!r}; available: {available}"
            )
        return matches[0]
    system = platform.system()
    if system == "Windows":
        os_name = "windows"
    elif system == "Linux":
        os_name = "linux"
    else:
        raise ReproductionError(
            f"v1 reproduction supports Linux and Windows x86-64 only; got {system!r}"
        )
    matches = [row for row in rows if row["os"] == os_name]
    if len(matches) != 1:
        raise ReproductionError(f"matrix does not contain exactly one {os_name} row")
    machine = platform.machine().lower()
    if machine not in {"amd64", "x86_64"}:
        raise ReproductionError(
            f"v1 first-shipped target is x86-64; host machine is {machine!r}"
        )
    return matches[0]


def executable_name(name: str) -> str:
    """Return a platform executable file name."""

    if os.name == "nt" and not name.endswith(".exe"):
        return f"{name}.exe"
    return name


def cargo_target_dir(workspace: Path) -> Path:
    """Return the Cargo target directory for this workspace."""

    override = os.environ.get("CARGO_TARGET_DIR")
    if override:
        path = Path(override)
        return path if path.is_absolute() else workspace / path
    return workspace / "target"


def compose_bin(workspace: Path) -> Path:
    return cargo_target_dir(workspace) / "debug" / executable_name("latticeaxiom-compose")


def play_bin(workspace: Path) -> Path:
    return cargo_target_dir(workspace) / "release" / executable_name("latticeaxiom-play")


def engine_bin(workspace: Path) -> Path:
    return cargo_target_dir(workspace) / "release" / executable_name("latticeaxiom-engine")


def load_contract(path: Path) -> dict[str, Any]:
    """Load and freeze-check the v1 reproduction fixture."""

    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ReproductionError(f"cannot read reproduction contract {path}: {error}") from error
    if not isinstance(document, dict):
        raise ReproductionError("reproduction contract must be a JSON object")
    _require_exact_keys(document, CONTRACT_KEYS, "contract")
    if document["schema_id"] != SCHEMA_ID:
        raise ReproductionError(f"unsupported schema_id: {document['schema_id']!r}")
    if document["schema_version"] != 1:
        raise ReproductionError(f"unsupported schema_version: {document['schema_version']!r}")
    if document["kind"] != "latticeaxiom.v1-release-reproduction/v1":
        raise ReproductionError(f"unsupported kind: {document['kind']!r}")
    if document["product_entry"] != "task play":
        raise ReproductionError("product entry must be task play")
    if document["linux_is_independent_root_gate"] is not True:
        raise ReproductionError("Linux must remain the independent root gate")
    if document["windows_rustc_crash_policy"] != "linux-root-continues":
        raise ReproductionError("Windows crash policy must keep the Linux root gate")
    if document["claims_d10_frozen"] is not False:
        raise ReproductionError("this harness must not claim D10 freeze without reference-host evidence")
    if document["claims_d2_working_set_gate"] is not False:
        raise ReproductionError("this harness must not claim the D2 working-set gate")
    if document["content_assets_included"] is not False:
        raise ReproductionError("Cargo evidence must not pretend to cover content assets")
    if document["commit_bound_docs_evidence"] is not False:
        raise ReproductionError("this harness must not write commit-bound docs evidence")
    if document["budgets"] != FROZEN_ADR_0026_BUDGETS:
        raise ReproductionError(
            "reproduction budgets differ from frozen ADR 0026 desktop-reference-v1 numbers"
        )
    tools = document["tools"]
    _require_exact_keys(
        tools,
        {
            "rust",
            "python",
            "cargo-deny",
            "cargo-cyclonedx",
            "cargo-about",
            "cyclonedx-spec",
        },
        "tools",
    )
    if tools["rust"] != "1.97.1" or tools["python"] != "3.12.12":
        raise ReproductionError("toolchain pins drifted from rust-toolchain.toml / CI Python")
    if tools["cargo-deny"] != "0.20.2":
        raise ReproductionError("cargo-deny pin drifted")
    if tools["cargo-cyclonedx"] != "0.5.9" or tools["cargo-about"] != "0.9.1":
        raise ReproductionError("release-evidence tool pins drifted")
    if tools["cyclonedx-spec"] != "1.5":
        raise ReproductionError("only CycloneDX 1.5 is supported")
    actions = document["actions"]
    if not isinstance(actions, dict) or not actions:
        raise ReproductionError("actions must pin every third-party GitHub Action")
    for name, spec in actions.items():
        if not isinstance(spec, dict):
            raise ReproductionError(f"action {name!r} must be an object")
        _require_exact_keys(spec, {"version", "pin"}, f"action {name}")
        pin = spec["pin"]
        if not isinstance(pin, str) or not re.fullmatch(r"[0-9a-f]{40}", pin):
            raise ReproductionError(f"action {name!r} is not pinned to a 40-character SHA")
    jobs = document["jobs"]
    if not isinstance(jobs, list) or not jobs:
        raise ReproductionError("contract must list jobs")
    seen: set[str] = set()
    required_ids = {
        "fmt",
        "check",
        "clippy",
        "rustdoc",
        "test",
        "dependency-license",
        "offline-frozen-lock",
        "headless-smoke",
        "client-compile",
        "deterministic-corpus",
        "fault-corpus",
        "release-evidence-fixtures",
        "release-evidence-smoke",
        "product-supervisor",
        "performance-traversal",
    }
    for job in jobs:
        if not isinstance(job, dict):
            raise ReproductionError("job must be an object")
        job_id = job.get("id")
        if not isinstance(job_id, str) or job_id in seen:
            raise ReproductionError(f"job id is missing or duplicated: {job_id!r}")
        seen.add(job_id)
        profiles = job.get("profiles")
        if not isinstance(profiles, list) or not profiles:
            raise ReproductionError(f"{job_id}: profiles must be a non-empty array")
        unknown = set(profiles) - PROFILES
        if unknown:
            raise ReproductionError(f"{job_id}: unknown profiles {sorted(unknown)!r}")
        required_in = job.get("required_in")
        if not isinstance(required_in, list) or set(required_in) - set(profiles):
            raise ReproductionError(f"{job_id}: required_in must be a subset of profiles")
        for step in job.get("steps", []):
            argv = step.get("argv")
            if not isinstance(argv, list) or not all(isinstance(item, str) for item in argv):
                raise ReproductionError(f"{job_id}: argv must be an array of strings")
            rendered = " ".join(argv).lower()
            for token in FORBIDDEN_TOKENS:
                if token in rendered:
                    raise ReproductionError(
                        f"{job_id}: forbidden operation {token!r} in {' '.join(argv)}"
                    )
        if job_id == "performance-traversal":
            if job.get("steps"):
                raise ReproductionError(
                    "performance-traversal has no shared-CI command; reference-host evidence is separate"
                )
            if job.get("designated_runner") is not True:
                raise ReproductionError("performance-traversal must remain a designated runner")
        if job_id == "product-supervisor" and job.get("designated_runner") is not True:
            raise ReproductionError("product-supervisor must remain a designated runner")
    missing = sorted(required_ids - seen)
    extra = sorted(seen - required_ids)
    if missing or extra:
        raise ReproductionError(f"job set differs: missing={missing!r}, extra={extra!r}")
    return document


def expand_token(token: str, context: Mapping[str, str]) -> str:
    """Replace ${name} placeholders from a bounded context map."""

    def replace(match: re.Match[str]) -> str:
        name = match.group(1)
        if name not in context:
            raise ReproductionError(f"unknown command placeholder ${{{name}}}")
        return context[name]

    return re.sub(r"\$\{([a-z_]+)\}", replace, token)


def expand_argv(argv: Sequence[str], context: Mapping[str, str]) -> list[str]:
    return [expand_token(item, context) for item in argv]


def normalize_text(text: str, workspace: Path) -> str:
    """Make command output path-independent and LF-normalized."""

    normalized = text.replace("\r\n", "\n").replace("\r", "\n")
    replacements = {
        str(workspace.resolve()): "${workspace}",
        workspace.resolve().as_posix(): "${workspace}",
        str(workspace): "${workspace}",
        workspace.as_posix(): "${workspace}",
    }
    # Longer paths first so drive-prefixed Windows paths win over relative forms.
    for raw, replacement in sorted(replacements.items(), key=lambda item: len(item[0]), reverse=True):
        if not raw:
            continue
        normalized = normalized.replace(raw, replacement)
        normalized = normalized.replace(raw.replace("\\", "/"), replacement)
    return normalized


def run_command(
    argv: Sequence[str],
    *,
    cwd: Path,
    environment: Mapping[str, str] | None,
    timeout: int,
) -> tuple[int, str]:
    """Run one documented command and return (exit_code, combined output)."""

    process_environment = os.environ.copy()
    process_environment["PYTHONDONTWRITEBYTECODE"] = "1"
    process_environment["PYTHONUTF8"] = "1"
    process_environment["SOURCE_DATE_EPOCH"] = "0"
    if environment:
        process_environment.update(environment)
    completed = subprocess.run(
        list(argv),
        cwd=cwd,
        env=process_environment,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return completed.returncode, completed.stdout


def job_for_profile(job: Mapping[str, Any], profile: str) -> bool:
    return profile in job["profiles"]


def required_for_profile(job: Mapping[str, Any], profile: str) -> bool:
    return profile in job["required_in"]


def record_step(
    *,
    argv: Sequence[str],
    executed: bool,
    exit_code: int | None,
    output: str,
    reason: str | None,
) -> dict[str, Any]:
    if executed and exit_code == 0:
        status = "pass"
    elif executed:
        status = "fail"
    else:
        status = "not-run"
    if status == "pass" and not executed:
        raise ReproductionError("pass requires an executed command")
    return {
        "argv": list(argv),
        "executed": executed,
        "exit_code": exit_code,
        "reason": reason,
        "status": status,
        "output": output,
    }


def execute_job(
    job: Mapping[str, Any],
    *,
    profile: str,
    workspace: Path,
    context: Mapping[str, str],
    timeout: int,
) -> dict[str, Any]:
    """Run or skip one fixture job and return a machine-readable result."""

    included = job_for_profile(job, profile)
    steps_out: list[dict[str, Any]] = []
    if not included:
        status = "not-run"
        reason = f"job is not part of profile {profile}"
    elif job["id"] == "performance-traversal":
        status = "not-run"
        reason = "reference-host 10-minute traversal is not a shared CI command"
    elif not job.get("steps"):
        status = "not-run"
        reason = "no documented command"
    else:
        reason = None
        status = "pass"
        for step in job["steps"]:
            argv = expand_argv(step["argv"], context)
            env = step.get("environment") if isinstance(step.get("environment"), dict) else job.get("environment")
            try:
                exit_code, output = run_command(
                    argv,
                    cwd=workspace,
                    environment=env if isinstance(env, dict) else None,
                    timeout=timeout,
                )
            except subprocess.TimeoutExpired as error:
                output = f"command timed out after {timeout}s: {' '.join(argv)}\n{error}"
                exit_code = 124
            except OSError as error:
                output = f"command could not start: {' '.join(argv)}\n{error}"
                exit_code = 127
            step_record = record_step(
                argv=argv,
                executed=True,
                exit_code=exit_code,
                output=normalize_text(output, workspace),
                reason=None,
            )
            steps_out.append(step_record)
            if exit_code != 0:
                status = "fail"
                reason = f"command exited {exit_code}: {' '.join(argv)}"
                break
    if not steps_out and status == "not-run":
        steps_out.append(
            record_step(
                argv=[],
                executed=False,
                exit_code=None,
                output="",
                reason=reason,
            )
        )
    return {
        "designated_runner": bool(job.get("designated_runner")),
        "evidence_class": job["evidence_class"],
        "id": job["id"],
        "kind": job["kind"],
        "notes": job.get("notes"),
        "production": bool(job.get("production")),
        "profile_included": included,
        "reason": reason,
        "required": required_for_profile(job, profile),
        "status": status,
        "steps": steps_out,
    }


def write_report(
    output_dir: Path,
    *,
    contract: Mapping[str, Any],
    profile: str,
    matrix: Mapping[str, Any],
    jobs: Sequence[Mapping[str, Any]],
    workspace: Path,
    complete_profile: bool,
) -> dict[str, Any]:
    """Write a normalized report and SHA-256 sidecar. Never claims unverified passes."""

    if output_dir.exists() and any(output_dir.iterdir()):
        raise ReproductionError(f"output directory must be empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)
    logs_dir = output_dir / "jobs"
    logs_dir.mkdir()
    files: list[dict[str, Any]] = []
    for job in jobs:
        log_name = f"{job['id']}.log"
        chunks = []
        for step in job["steps"]:
            argv = " ".join(step["argv"]) if step["argv"] else "(not-run)"
            chunks.append(f"$ {argv}\n")
            chunks.append(step.get("output") or "")
            if not str(chunks[-1]).endswith("\n") and chunks[-1]:
                chunks.append("\n")
        payload = normalize_text("".join(chunks), workspace).encode("utf-8")
        if payload and not payload.endswith(b"\n"):
            payload += b"\n"
        (logs_dir / log_name).write_bytes(payload)
        files.append(
            {
                "bytes": len(payload),
                "media_type": "text/plain; charset=utf-8",
                "name": f"jobs/{log_name}",
                "sha256": sha256_bytes(payload),
            }
        )
        for step in job["steps"]:
            step.pop("output", None)

    required = [job for job in jobs if job["required"] and job["profile_included"]]
    expected_required = {
        item["id"]
        for item in contract["jobs"]
        if profile in item["required_in"]
    }
    reported_ids = {job["id"] for job in jobs}
    if any(job["status"] != "pass" for job in required):
        outcome = "fail"
    elif not complete_profile:
        outcome = "incomplete"
    elif expected_required - reported_ids:
        raise ReproductionError(
            "complete profile is missing required jobs: "
            f"{sorted(expected_required - reported_ids)!r}"
        )
    else:
        outcome = "pass"
    if any(job["status"] == "pass" and not job["profile_included"] for job in jobs):
        raise ReproductionError("a skipped job was marked pass")
    if any(
        job["status"] == "pass" and not any(step.get("executed") for step in job["steps"])
        for job in jobs
    ):
        raise ReproductionError("pass requires an executed documented command")

    report = {
        "budgets": contract["budgets"],
        "claims_d10_frozen": False,
        "claims_d2_working_set_gate": False,
        "commit_bound_docs_evidence": False,
        "files": files,
        "jobs": jobs,
        "kind": EVIDENCE_KIND,
        "linux_is_independent_root_gate": True,
        "matrix": {
            "continue_on_error": matrix["continue_on_error"],
            "id": matrix["id"],
            "os": matrix["os"],
            "root_gate": matrix["root_gate"],
            "target": matrix["target"],
        },
        "outcome": outcome,
        "product_entry": "task play",
        "profile": profile,
        "schema": 1,
        "tools": contract["tools"],
        "windows_rustc_crash_policy": contract["windows_rustc_crash_policy"],
    }
    report_bytes = canonical_json_bytes(report)
    report_path = output_dir / "v1-reproduction-report.json"
    report_path.write_bytes(report_bytes)
    digest = sha256_bytes(report_bytes)
    sidecar = output_dir / "v1-reproduction-report.sha256"
    sidecar.write_text(f"{digest}  v1-reproduction-report.json\n", encoding="utf-8", newline="\n")
    verify_report_directory(output_dir)
    return report


def verify_report_directory(directory: Path) -> None:
    """Verify the reproduction report hashes without treating existence as pass."""

    report_path = directory / "v1-reproduction-report.json"
    sidecar_path = directory / "v1-reproduction-report.sha256"
    try:
        report_bytes = report_path.read_bytes()
        report = json.loads(report_bytes)
        sidecar = sidecar_path.read_text(encoding="utf-8")
    except (OSError, json.JSONDecodeError) as error:
        raise ReproductionError(f"cannot read reproduction report: {error}") from error
    if not isinstance(report, dict) or report.get("kind") != EVIDENCE_KIND:
        raise ReproductionError("reproduction report kind is invalid")
    expected_sidecar = f"{sha256_bytes(report_bytes)}  v1-reproduction-report.json\n"
    if sidecar != expected_sidecar:
        raise ReproductionError("reproduction report SHA-256 sidecar differs")
    if report.get("claims_d10_frozen") or report.get("claims_d2_working_set_gate"):
        raise ReproductionError("report must not claim frozen performance gates")
    if report.get("commit_bound_docs_evidence"):
        raise ReproductionError("report must not claim commit-bound docs evidence")
    if report.get("budgets") != FROZEN_ADR_0026_BUDGETS:
        raise ReproductionError("report budgets drifted from ADR 0026")
    files = report.get("files")
    if not isinstance(files, list):
        raise ReproductionError("report file list is missing")
    for item in files:
        _require_exact_keys(item, {"bytes", "media_type", "name", "sha256"}, "file")
        name = item["name"]
        if not isinstance(name, str) or ".." in Path(name).parts:
            raise ReproductionError(f"unsafe report filename: {name!r}")
        path = directory / name
        if path.stat().st_size != item["bytes"] or sha256_file(path) != item["sha256"]:
            raise ReproductionError(f"report artifact hash differs: {name}")
        if not SHA256_PATTERN.fullmatch(item["sha256"]):
            raise ReproductionError(f"invalid artifact digest: {name}")
    for job in report.get("jobs", []):
        if job.get("status") == "pass":
            if not job.get("profile_included"):
                raise ReproductionError(f"{job.get('id')}: skipped job marked pass")
            if not any(step.get("executed") and step.get("exit_code") == 0 for step in job.get("steps", [])):
                raise ReproductionError(f"{job.get('id')}: pass without executed command")


def pin_workflow_actions(workflow: Path, contract: Mapping[str, Any]) -> None:
    """Require the v1 workflow to use the same action SHAs as the fixture."""

    text = workflow.read_text(encoding="utf-8")
    for name, spec in contract["actions"].items():
        pin = spec["pin"]
        if pin not in text:
            raise ReproductionError(f"workflow {workflow} does not pin {name} at {pin}")
        if re.search(rf"uses:\s*{re.escape(name)}@(?!{pin})", text):
            raise ReproductionError(f"workflow {workflow} uses an unpinned {name}")


def parse_arguments(arguments: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate", help="validate the fixture, pins, and frozen budgets")
    list_parser = subparsers.add_parser("list", help="list jobs for one profile")
    list_parser.add_argument("--profile", required=True, choices=sorted(PROFILES))
    run_parser = subparsers.add_parser("run", help="execute documented jobs for one profile")
    run_parser.add_argument("--profile", required=True, choices=sorted(PROFILES))
    run_parser.add_argument("--output-dir", required=True, type=Path)
    run_parser.add_argument("--matrix-id", default=os.environ.get("V1_MATRIX_ID"))
    run_parser.add_argument("--job", action="append", dest="jobs")
    run_parser.add_argument("--timeout-seconds", type=int, default=10_800)
    verify_parser = subparsers.add_parser("verify", help="verify a reproduction report directory")
    verify_parser.add_argument("--evidence-dir", required=True, type=Path)
    return parser.parse_args(arguments)


def main(arguments: Sequence[str] | None = None) -> int:
    parsed = parse_arguments(sys.argv[1:] if arguments is None else arguments)
    root = workspace_root()
    try:
        if parsed.command == "verify":
            verify_report_directory(parsed.evidence_dir.resolve())
            print(f"verified v1 reproduction report in {parsed.evidence_dir.resolve()}")
            return 0
        contract = load_contract(root / CONTRACT_PATH)
        workflow = root / ".github" / "workflows" / "v1-release-gate.yml"
        if workflow.is_file():
            pin_workflow_actions(workflow, contract)
        if parsed.command == "validate":
            print(
                "validated v1 reproduction fixture, ADR 0026 budgets, and action pins"
            )
            return 0
        profile = parsed.profile
        selected_jobs = [
            job
            for job in contract["jobs"]
            if parsed.command == "list" or parsed.jobs is None or job["id"] in parsed.jobs
        ]
        if parsed.command == "list":
            for job in selected_jobs:
                flag = "required" if required_for_profile(job, profile) else "optional"
                inclusion = "included" if job_for_profile(job, profile) else "excluded"
                print(f"{job['id']}\t{inclusion}\t{flag}\t{job['evidence_class']}")
            return 0
        if profile == "reference-host":
            raise ReproductionError(
                "reference-host performance evidence is not produced by this harness"
            )
        matrix = host_matrix_entry(contract, parsed.matrix_id)
        python = sys.executable
        context = {
            "python": python,
            "workspace": str(root),
            "compose_bin": str(compose_bin(root)),
            "play_bin": str(play_bin(root)),
            "engine_bin": str(engine_bin(root)),
        }
        output_dir = parsed.output_dir
        if not output_dir.is_absolute():
            output_dir = root / output_dir
        results = [
            execute_job(
                job,
                profile=profile,
                workspace=root,
                context=context,
                timeout=parsed.timeout_seconds,
            )
            for job in selected_jobs
        ]
        report = write_report(
            output_dir.resolve(),
            contract=contract,
            profile=profile,
            matrix=matrix,
            jobs=results,
            workspace=root,
            complete_profile=parsed.jobs is None,
        )
        print(f"v1 reproduction outcome: {report['outcome']}")
        print(f"report: {output_dir.resolve() / 'v1-reproduction-report.json'}")
        return 0 if report["outcome"] in {"pass", "incomplete"} else 1
    except (ReproductionError, OSError, subprocess.TimeoutExpired) as error:
        print(f"v1 reproduction failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
