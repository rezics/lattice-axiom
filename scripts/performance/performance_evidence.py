#!/usr/bin/env python3
"""Capture and evaluate ADR 0026 performance evidence.

The runner records profile, toolchain, and lock fingerprints, invokes the
bounded headless harness, and emits `PerformanceEvidenceV1`. It never claims a
D10 freeze, a render-budget pass from a headless run, or a three-run
normative gate from a single artifact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import tomllib
from typing import Any, Mapping


PROFILE_PATH = Path("scripts/performance/desktop-reference-v1.toml")
SCHEMA_PATH = Path("scripts/performance/performance-evidence-v1.schema.json")
EVIDENCE_SCHEMA = "latticeaxiom:schema/performance-evidence-v1"
OBSERVATION_SCHEMA = "latticeaxiom:schema/performance-observation-v1"
MEASUREMENT_TOOL = "latticeaxiom-performance-evidence/v1"
HISTOGRAM_VERSION = "nearest-rank-v1"
PROFILE_NAME = "desktop-reference-v1"
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}\Z")

OBSERVATION_REQUIRED = {
    "schema",
    "schema_version",
    "mode",
    "scenario",
    "measurement_tool",
    "histogram_version",
    "protocol",
    "fingerprints",
    "streaming_profile",
    "histograms",
    "high_waters",
    "queues",
    "path",
    "faults",
}

OVERALL_REQUIRED = {"gate_verdict", "run_has_failing_metric", "reason"}
VERDICTS = frozenset({"pass", "fail", "insufficient-evidence", "unsupported"})


class EvidenceError(RuntimeError):
    """Raised when performance evidence cannot be assembled or validated."""


def _require_exact_keys(value: Mapping[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        raise EvidenceError(
            f"{context} keys differ: missing={sorted(expected - actual)!r}, "
            f"extra={sorted(actual - expected)!r}"
        )


def _require_mapping(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{context} must be an object")
    return value


def sha256_file(path: Path) -> str:
    """Return the SHA-256 hex digest of a file's exact bytes."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(payload: bytes) -> str:
    """Return the SHA-256 hex digest of `payload`."""

    return hashlib.sha256(payload).hexdigest()


def nearest_rank(sorted_samples: list[int], percentile: int) -> int | None:
    """Nearest-rank percentile: rank = max(1, ceil(p / 100 * n))."""

    if not sorted_samples or not 0 <= percentile <= 100:
        return None
    n = len(sorted_samples)
    rank = math.ceil(percentile / 100 * n)
    rank = max(1, min(n, rank))
    return sorted_samples[rank - 1]


def summarize_samples(samples: list[int]) -> dict[str, Any]:
    """Return the ADR 0026 histogram fields for one integer series."""

    if not samples:
        return {
            "count": 0,
            "p50": None,
            "p95": None,
            "p99": None,
            "max": None,
            "mean": None,
            "high_water": None,
        }
    ordered = sorted(samples)
    total = sum(ordered)
    maximum = ordered[-1]
    return {
        "count": len(ordered),
        "p50": nearest_rank(ordered, 50),
        "p95": nearest_rank(ordered, 95),
        "p99": nearest_rank(ordered, 99),
        "max": maximum,
        "mean": total // len(ordered),
        "high_water": maximum,
    }


def load_profile(path: Path = PROFILE_PATH) -> dict[str, Any]:
    """Load and pin the frozen `desktop-reference-v1` budgets."""

    try:
        document = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise EvidenceError(f"cannot read performance profile {path}: {error}") from error
    _require_exact_keys(
        document,
        {
            "schema",
            "profile",
            "document_id",
            "histogram_version",
            "measurement_tool",
            "lifecycle_until_reference_host_freeze",
            "hardware_class",
            "protocol",
            "budgets",
        },
        "performance profile",
    )
    if document["schema"] != 1:
        raise EvidenceError(f"unsupported profile schema: {document['schema']!r}")
    if document["profile"] != PROFILE_NAME:
        raise EvidenceError(f"unexpected profile name: {document['profile']!r}")
    if document["histogram_version"] != HISTOGRAM_VERSION:
        raise EvidenceError("profile histogram version drifted from nearest-rank-v1")
    if document["measurement_tool"] != MEASUREMENT_TOOL:
        raise EvidenceError("profile measurement tool drifted")
    if document["lifecycle_until_reference_host_freeze"] != "d2-provisional":
        raise EvidenceError("profile must remain d2-provisional until a D10 freeze")
    budgets = _require_mapping(document["budgets"], "profile budgets")
    _require_exact_keys(
        budgets,
        {"render", "fixed", "memory", "working_set", "latency", "queues", "dynamic"},
        "profile budgets",
    )
    _assert_frozen_budgets(budgets, _require_mapping(document["protocol"], "profile protocol"))
    return document


def _assert_frozen_budgets(budgets: Mapping[str, Any], protocol: Mapping[str, Any]) -> None:
    render = _require_mapping(budgets["render"], "render budgets")
    fixed = _require_mapping(budgets["fixed"], "fixed budgets")
    memory = _require_mapping(budgets["memory"], "memory budgets")
    working = _require_mapping(budgets["working_set"], "working-set budgets")
    latency = _require_mapping(budgets["latency"], "latency budgets")
    queues = _require_mapping(budgets["queues"], "queue budgets")
    if render["presented_frame_p95_ns"] != 16_670_000:
        raise EvidenceError("render P95 must remain ADR 0026 16.67 ms")
    if render["presented_frame_p99_ns"] != 25_000_000:
        raise EvidenceError("render P99 must remain ADR 0026 25 ms")
    if render["headless_may_declare_pass"] is not False:
        raise EvidenceError("headless must not be allowed to declare a render pass")
    if fixed["cpu_p95_ns"] != 8_000_000 or fixed["cpu_p99_ns"] != 12_000_000:
        raise EvidenceError("fixed-tick CPU budgets drifted from ADR 0026")
    if memory["client_ram_high_water_bytes"] != 4 * 1024**3:
        raise EvidenceError("RAM high-water must remain 4 GiB")
    if memory["gpu_memory_high_water_bytes"] != 3 * 1024**3:
        raise EvidenceError("VRAM high-water must remain 3 GiB")
    if working["active_chunks_max"] != 405 or working["resident_chunks_max"] != 1183:
        raise EvidenceError("working-set chunk caps drifted from ADR 0026")
    if working["visible_chunks_max"] != 512 or working["requested_prefetch_chunks_max"] != 1536:
        raise EvidenceError("visible/prefetch caps drifted from ADR 0026")
    if working["chunk_edge_voxels"] != 32:
        raise EvidenceError("ADR 0026 sizing premise is 32³")
    if latency["edit_to_visible_p95_ns"] != 100_000_000:
        raise EvidenceError("edit-to-visible P95 must remain 100 ms")
    if queues["mesh_jobs"] != 128 or queues["collider_jobs"] != 64:
        raise EvidenceError("mesh/collider job caps drifted from ADR 0026")
    if queues["combined_jobs"] != 512 or queues["combined_bytes"] != 384 * 1024 * 1024:
        raise EvidenceError("combined queue caps drifted from ADR 0026")
    if queues["apply_wall_ns"] != 2_000_000 or queues["apply_bytes"] != 16 * 1024 * 1024:
        raise EvidenceError("apply-slice caps drifted from ADR 0026")
    if protocol["warmup_ticks"] != 7200 or protocol["measure_ticks"] != 36000:
        raise EvidenceError("traversal tick counts drifted from 2 + 10 minutes at 60 Hz")
    if protocol["smoke_measure_ticks"] >= protocol["measure_ticks"]:
        raise EvidenceError("smoke mode must stay shorter than the 10-minute protocol")
    if protocol["hard_threshold_overrun_ratio_numer"] / protocol[
        "hard_threshold_overrun_ratio_denom"
    ] != 1.2:
        raise EvidenceError("hard-threshold overrun ratio must remain 20%")


def load_observation(path: Path) -> dict[str, Any]:
    """Load a harness observation and reject unknown or missing fields."""

    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read observation {path}: {error}") from error
    document = _require_mapping(payload, "observation")
    missing = OBSERVATION_REQUIRED - set(document)
    extra = set(document) - OBSERVATION_REQUIRED
    if missing or extra:
        raise EvidenceError(
            f"observation keys differ: missing={sorted(missing)!r}, extra={sorted(extra)!r}"
        )
    if document["schema"] != OBSERVATION_SCHEMA:
        raise EvidenceError(f"unexpected observation schema: {document['schema']!r}")
    if document["schema_version"] != 1:
        raise EvidenceError("unsupported observation schema version")
    if document["measurement_tool"] != MEASUREMENT_TOOL:
        raise EvidenceError("observation measurement tool drifted")
    if document["histogram_version"] != HISTOGRAM_VERSION:
        raise EvidenceError("observation histogram version drifted")
    if document["scenario"] != "headless-manual-time":
        raise EvidenceError("this runner only evaluates headless-manual-time observations")
    return document


def capture_fingerprints(
    workspace: Path,
    observation: Mapping[str, Any],
    *,
    cargo_profile: str = "bench",
    lto: str = "thin",
    codegen_units: int | None = 1,
) -> dict[str, Any]:
    """Record commit, lock, toolchain, and build-profile fingerprints."""

    cargo_lock = workspace / "Cargo.lock"
    toolchain_path = workspace / "rust-toolchain.toml"
    commit = _git(workspace, ["rev-parse", "HEAD"])
    dirty = bool(_git(workspace, ["status", "--porcelain", "--untracked-files=all"]))
    rustc = _run(["rustc", "-vV"], workspace)
    rustc_release = ""
    for line in rustc.splitlines():
        if line.startswith("release:"):
            rustc_release = line.split(":", 1)[1].strip()
            break
    toolchain = tomllib.loads(toolchain_path.read_text(encoding="utf-8"))
    observed = _require_mapping(observation["fingerprints"], "observation fingerprints")
    return {
        "git_commit": commit,
        "git_dirty": dirty,
        "cargo_lock_sha256": sha256_file(cargo_lock),
        "rust_toolchain_channel": toolchain.get("toolchain", {}).get("channel"),
        "rustc_release": rustc_release,
        "cargo_profile": cargo_profile,
        "lto": lto,
        "codegen_units": codegen_units,
        "cargo_features": ["performance-evidence"],
        "development_dynamic_linking": False,
        "os": platform.system(),
        "arch": platform.machine(),
        "product_lock_hash": observed.get("product_lock_hash"),
        "registration_semantic_hash": observed.get("registration_semantic_hash"),
        "registration_image_hash": observed.get("registration_image_hash"),
        "engine_build_id": observed.get("engine_build_id"),
        "lock_file_sha256": observed.get("lock_file_sha256"),
        "shell_lock_file_sha256": observed.get("shell_lock_file_sha256"),
        "realization_target": observed.get("realization_target"),
        "world_seed": observed.get("world_seed"),
    }


def _git(workspace: Path, args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=workspace,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if result.returncode != 0:
        raise EvidenceError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def _run(command: list[str], workspace: Path) -> str:
    result = subprocess.run(
        command,
        cwd=workspace,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if result.returncode != 0:
        raise EvidenceError(f"{command[0]} failed: {result.stderr.strip()}")
    return result.stdout


def metric_record(
    *,
    metric_id: str,
    unit: str,
    histogram: Mapping[str, Any] | None,
    high_water: int | None,
    budget_p95: int | None,
    budget_p99: int | None,
    budget_high_water: int | None,
    required_samples: int,
    eligible: bool,
    unsupported_reason: str | None = None,
) -> dict[str, Any]:
    """Compare one observed series against a frozen budget."""

    count = 0 if histogram is None else int(histogram.get("count") or 0)
    if high_water is not None:
        count = max(count, 1)
    if not eligible:
        verdict = "unsupported"
        reason = unsupported_reason or "scenario cannot observe this metric"
    elif count < required_samples:
        verdict = "insufficient-evidence"
        reason = f"count {count} < required {required_samples}"
    else:
        verdict = "pass"
        reason = "within frozen budget"
        comparisons = (
            ("p95", histogram.get("p95") if histogram else None, budget_p95),
            ("p99", histogram.get("p99") if histogram else None, budget_p99),
            ("high_water", high_water, budget_high_water),
        )
        for name, observed, budget in comparisons:
            if observed is None or budget is None:
                continue
            if int(observed) > int(budget):
                verdict = "fail"
                overrun_20 = int(observed) * 5 > int(budget) * 6
                reason = (
                    f"{name} {observed} exceeds budget {budget}"
                    + (" by more than 20%" if overrun_20 else "")
                )
                break
    return {
        "id": metric_id,
        "unit": unit,
        "count": count,
        "p50": None if histogram is None else histogram.get("p50"),
        "p95": None if histogram is None else histogram.get("p95"),
        "p99": None if histogram is None else histogram.get("p99"),
        "max": None if histogram is None else histogram.get("max"),
        "mean": None if histogram is None else histogram.get("mean"),
        "high_water": high_water if high_water is not None else (
            None if histogram is None else histogram.get("high_water")
        ),
        "required_samples": required_samples,
        "budget_p95": budget_p95,
        "budget_p99": budget_p99,
        "budget_high_water": budget_high_water,
        "run_verdict": verdict,
        "reason": reason,
    }


def evaluate_observation(
    profile: Mapping[str, Any],
    observation: Mapping[str, Any],
    fingerprints: Mapping[str, Any],
    *,
    ram_bytes: int | None,
    observation_sha256: str,
    reference_host_run: bool,
    run_count: int,
) -> dict[str, Any]:
    """Assemble PerformanceEvidenceV1. One run cannot close the D10 gate."""

    protocol_cfg = _require_mapping(profile["protocol"], "profile protocol")
    budgets = _require_mapping(profile["budgets"], "profile budgets")
    histograms = _require_mapping(observation["histograms"], "observation histograms")
    high_waters = _require_mapping(observation["high_waters"], "observation high-waters")
    observed_protocol = _require_mapping(observation["protocol"], "observation protocol")
    streaming = observation.get("streaming_profile")
    matches_coverage = False
    claims_working_set = False
    if isinstance(streaming, dict):
        matches_coverage = bool(streaming.get("matches_adr_0026_world_space_coverage"))
        claims_working_set = bool(streaming.get("claims_d2_working_set_gate"))

    sample_min = int(protocol_cfg["edit_mesh_collider_load_sample_min"])
    activation_min = int(protocol_cfg["world_activation_save_checkpoint_runs_min"])
    frame_min = int(protocol_cfg["measure_ticks"])
    headless = observation["scenario"] == "headless-manual-time"

    metrics = {
        "presented_render_frame": metric_record(
            metric_id="presented_render_frame",
            unit="ns",
            histogram=None,
            high_water=None,
            budget_p95=int(budgets["render"]["presented_frame_p95_ns"]),
            budget_p99=int(budgets["render"]["presented_frame_p99_ns"]),
            budget_high_water=None,
            required_samples=frame_min,
            eligible=not headless,
            unsupported_reason="headless manual-time cannot declare a render-budget pass",
        ),
        "fixed_update_cpu": metric_record(
            metric_id="fixed_update_cpu",
            unit="ns",
            histogram=_require_mapping(histograms["fixed_update_cpu_ns"], "fixed_update_cpu_ns"),
            high_water=None,
            budget_p95=int(budgets["fixed"]["cpu_p95_ns"]),
            budget_p99=int(budgets["fixed"]["cpu_p99_ns"]),
            budget_high_water=None,
            required_samples=frame_min,
            eligible=True,
        ),
        "physics_step": metric_record(
            metric_id="physics_step",
            unit="ns",
            histogram=None,
            high_water=None,
            budget_p95=int(budgets["fixed"]["physics_p95_ns"]),
            budget_p99=int(budgets["fixed"]["physics_p99_ns"]),
            budget_high_water=None,
            required_samples=frame_min,
            eligible=False,
            unsupported_reason="physics share is not split from the combined headless tick",
        ),
        "client_ram": metric_record(
            metric_id="client_ram",
            unit="bytes",
            histogram=None,
            high_water=ram_bytes if ram_bytes is not None else high_waters.get("ram_bytes"),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["memory"]["client_ram_high_water_bytes"]),
            required_samples=1,
            eligible=ram_bytes is not None or high_waters.get("ram_bytes") is not None,
            unsupported_reason="process RSS was not sampled",
        ),
        "gpu_memory": metric_record(
            metric_id="gpu_memory",
            unit="bytes",
            histogram=None,
            high_water=None,
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["memory"]["gpu_memory_high_water_bytes"]),
            required_samples=1,
            eligible=False,
            unsupported_reason="GPU memory counter is unsupported on this headless backend",
        ),
        "resident_chunks": metric_record(
            metric_id="resident_chunks",
            unit="chunks",
            histogram=_require_mapping(histograms["resident_chunks"], "resident_chunks"),
            high_water=int(high_waters["resident_chunks"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["working_set"]["resident_chunks_max"]),
            required_samples=1,
            eligible=True,
        ),
        "active_chunks": metric_record(
            metric_id="active_chunks",
            unit="chunks",
            histogram=_require_mapping(histograms["active_chunks"], "active_chunks"),
            high_water=int(high_waters["active_chunks"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["working_set"]["active_chunks_max"]),
            required_samples=1,
            eligible=True,
        ),
        "visible_chunks": metric_record(
            metric_id="visible_chunks",
            unit="chunks",
            histogram=_require_mapping(histograms["visible_chunks"], "visible_chunks"),
            high_water=int(high_waters["visible_chunks"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["working_set"]["visible_chunks_max"]),
            required_samples=1,
            eligible=not headless,
            unsupported_reason="headless prepared geometry does not measure GPU/frustum visibility" if headless else None,
        ),
        "mesh_queue_jobs": metric_record(
            metric_id="mesh_queue_jobs",
            unit="jobs",
            histogram=None,
            high_water=int(high_waters["mesh_jobs"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["queues"]["mesh_jobs"]),
            required_samples=1,
            eligible=True,
        ),
        "collider_queue_jobs": metric_record(
            metric_id="collider_queue_jobs",
            unit="jobs",
            histogram=None,
            high_water=int(high_waters["collider_jobs"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["queues"]["collider_jobs"]),
            required_samples=1,
            eligible=True,
        ),
        "combined_queue_jobs": metric_record(
            metric_id="combined_queue_jobs",
            unit="jobs",
            histogram=None,
            high_water=int(high_waters["combined_jobs"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["queues"]["combined_jobs"]),
            required_samples=1,
            eligible=True,
        ),
        "combined_queue_bytes": metric_record(
            metric_id="combined_queue_bytes",
            unit="bytes",
            histogram=_require_mapping(histograms["reserved_bytes"], "reserved_bytes"),
            high_water=int(high_waters["combined_reserved_bytes"]),
            budget_p95=None,
            budget_p99=None,
            budget_high_water=int(budgets["queues"]["combined_bytes"]),
            required_samples=1,
            eligible=True,
        ),
        "edit_to_visible": metric_record(
            metric_id="edit_to_visible",
            unit="ns",
            histogram=None,
            high_water=None,
            budget_p95=int(budgets["latency"]["edit_to_visible_p95_ns"]),
            budget_p99=int(budgets["latency"]["edit_to_visible_p99_ns"]),
            budget_high_water=None,
            required_samples=sample_min,
            eligible=False,
            unsupported_reason="headless host has no presented mesh timestamp",
        ),
        "save_durable": metric_record(
            metric_id="save_durable",
            unit="ns",
            histogram=None,
            high_water=None,
            budget_p95=int(budgets["latency"]["save_durable_p95_ns"]),
            budget_p99=int(budgets["latency"]["save_durable_p99_ns"]),
            budget_high_water=None,
            required_samples=activation_min,
            eligible=False,
            unsupported_reason="this harness does not open a world writer",
        ),
    }

    working_set_gate = "insufficient-evidence"
    working_reason = "live fixture does not claim the ADR 0026 world-space working-set gate"
    if claims_working_set:
        working_set_gate = "fail"
        working_reason = "host claimed the D2 working-set gate without this runner's authorization"
    elif matches_coverage:
        working_set_gate = "insufficient-evidence"
        working_reason = "coverage matches but D10 freeze still requires three reference-host runs"

    failing = [item["id"] for item in metrics.values() if item["run_verdict"] == "fail"]
    faults = _require_mapping(observation["faults"], "observation faults")
    if faults.get("truncated") or faults.get("incomplete_measure"):
        failing.append("protocol_complete")
    if observation['mode'] == 'traversal' and observation['path']['unique_player_chunks'] < 2:
        failing.append('traversal_not_observed')

    gate_verdict = "insufficient-evidence"
    reason = (
        "single-run headless evidence cannot close the ADR 0026 three-run "
        "reference-host gate"
    )
    if failing:
        gate_verdict = "fail"
        reason = "run failed a budget, protocol, or traversal check: " + ", ".join(failing)
    elif (
        reference_host_run
        and run_count >= int(protocol_cfg["normative_run_count"])
        and observation["mode"] == "traversal"
        and not headless
        and all(item["run_verdict"] == "pass" for item in metrics.values())
    ):
        gate_verdict = "pass"
        reason = "reference-host traversal met every required budget"
    if gate_verdict == "pass" and profile["lifecycle_until_reference_host_freeze"] != "d10-frozen":
        raise EvidenceError("refusing to mark d10-frozen without an accepted freeze")

    lifecycle = "d2-provisional"
    overall = {
        "gate_verdict": gate_verdict,
        "run_has_failing_metric": bool(failing),
        "reason": reason,
        "working_set_gate": working_set_gate,
        "working_set_reason": working_reason,
        "normative_run_count_observed": run_count,
        "normative_run_count_required": int(protocol_cfg["normative_run_count"]),
    }
    _require_exact_keys(
        {key: overall[key] for key in OVERALL_REQUIRED},
        OVERALL_REQUIRED,
        "overall required fields",
    )
    if overall["gate_verdict"] not in VERDICTS:
        raise EvidenceError(f"invalid gate verdict {overall['gate_verdict']!r}")

    return {
        "schema": EVIDENCE_SCHEMA,
        "schema_version": 1,
        "profile": PROFILE_NAME,
        "lifecycle_status": lifecycle,
        "scenario": observation["scenario"],
        "mode": observation["mode"],
        "measurement_tool": MEASUREMENT_TOOL,
        "histogram_version": HISTOGRAM_VERSION,
        "normative_render_pass_permitted": False,
        "reference_host_run": reference_host_run,
        "fingerprints": dict(fingerprints),
        "protocol": observed_protocol,
        "metrics": metrics,
        "high_waters": dict(high_waters),
        "queues": observation["queues"],
        "path": observation["path"],
        "faults": dict(faults),
        "streaming_profile": streaming,
        "observation_sha256": observation_sha256,
        "overall": overall,
    }


def validate_evidence(document: Mapping[str, Any]) -> None:
    """Reject unknown fields, missing fields, and illegal pass claims."""

    required = {
        "schema",
        "schema_version",
        "profile",
        "lifecycle_status",
        "scenario",
        "mode",
        "measurement_tool",
        "histogram_version",
        "normative_render_pass_permitted",
        "reference_host_run",
        "fingerprints",
        "protocol",
        "metrics",
        "high_waters",
        "queues",
        "path",
        "faults",
        "streaming_profile",
        "observation_sha256",
        "overall",
    }
    _require_exact_keys(document, required, "performance evidence")
    if document["schema"] != EVIDENCE_SCHEMA:
        raise EvidenceError("evidence schema identity drifted")
    if document["lifecycle_status"] not in {"d2-provisional", "d10-frozen"}:
        raise EvidenceError("invalid lifecycle status")
    if document["lifecycle_status"] == "d10-frozen":
        raise EvidenceError("D10 freeze is not authorized by this harness")
    if document["normative_render_pass_permitted"] is not False:
        raise EvidenceError("headless evidence must not permit a render pass")
    if document["scenario"] == "headless-manual-time":
        presented = _require_mapping(
            _require_mapping(document["metrics"], "metrics")["presented_render_frame"],
            "presented_render_frame",
        )
        if presented["run_verdict"] == "pass":
            raise EvidenceError("headless run declared a render-budget pass")
    if not SHA256_PATTERN.fullmatch(str(document["observation_sha256"])):
        raise EvidenceError("observation_sha256 is not a SHA-256 hex digest")
    overall = _require_mapping(document["overall"], "overall")
    if overall["gate_verdict"] == "pass" and document["mode"] == "smoke":
        raise EvidenceError("smoke mode cannot pass the normative gate")


def write_json(path: Path, document: Mapping[str, Any]) -> None:
    """Write canonical UTF-8 JSON with a trailing newline."""

    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(document, indent=2, sort_keys=True) + "\n"
    path.write_text(encoded, encoding="utf-8")


def find_bench_executable(workspace: Path, *, smoke: bool) -> Path:
    """Build the harness without default features and return its path.

    Smoke uses the workspace `dev` profile so local verification is not blocked
    by the documented Windows rustc crash on `codegen-units = 1`. Traversal
    keeps the bench profile (release, thin LTO, `codegen-units = 1`).
    """

    if smoke:
        command = [
            "cargo",
            "build",
            "-p",
            "latticeaxiom-engine",
            "--bench",
            "performance_evidence",
            "--features",
            "performance-evidence",
            "--no-default-features",
            "--locked",
            "--message-format=json",
        ]
    else:
        command = [
            "cargo",
            "bench",
            "-p",
            "latticeaxiom-engine",
            "--bench",
            "performance_evidence",
            "--features",
            "performance-evidence",
            "--no-default-features",
            "--locked",
            "--no-run",
            "--message-format=json",
        ]
    result = subprocess.run(
        command,
        cwd=workspace,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if result.returncode != 0:
        raise EvidenceError(f"cargo bench --no-run failed:\n{result.stderr}")
    executable = None
    for line in result.stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target") or {}
        if target.get("name") != "performance_evidence":
            continue
        if "bench" not in target.get("kind", []):
            continue
        path = message.get("executable")
        if path:
            executable = Path(path)
    if executable is None or not executable.is_file():
        raise EvidenceError("cargo did not emit a performance_evidence bench executable")
    return executable


def sample_rss_bytes(pid: int) -> int | None:
    """Return current working-set/RSS for `pid`, or None when unsupported."""

    if os.name == "nt":
        return _windows_working_set(pid)
    status = Path(f"/proc/{pid}/status")
    if not status.is_file():
        return None
    for line in status.read_text(encoding="utf-8").splitlines():
        if line.startswith("VmRSS:"):
            parts = line.split()
            if len(parts) >= 2:
                return int(parts[1]) * 1024
    return None


def _windows_working_set(pid: int) -> int | None:
    import ctypes
    from ctypes import wintypes

    class ProcessMemoryCounters(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    process_query_information = 0x0400
    process_vm_read = 0x0010
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    handle = kernel32.OpenProcess(process_query_information | process_vm_read, False, pid)
    if not handle:
        return None
    try:
        counters = ProcessMemoryCounters()
        counters.cb = ctypes.sizeof(ProcessMemoryCounters)
        if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
            return None
        return int(counters.PeakWorkingSetSize)
    finally:
        kernel32.CloseHandle(handle)


def run_harness(
    workspace: Path,
    *,
    mode: str,
    output: Path,
    profile: Mapping[str, Any],
) -> tuple[dict[str, Any], int | None]:
    """Spawn the bounded bench and sample peak RSS."""

    protocol = _require_mapping(profile["protocol"], "profile protocol")
    if mode == "smoke":
        warmup = int(protocol["smoke_warmup_ticks"])
        measure = int(protocol["smoke_measure_ticks"])
        max_wall = int(protocol["smoke_max_wall_seconds"])
    elif mode == "traversal":
        warmup = int(protocol["warmup_ticks"])
        measure = int(protocol["measure_ticks"])
        max_wall = int(protocol["traversal_max_wall_seconds"])
    else:
        raise EvidenceError(f"unknown mode {mode!r}")
    if warmup > int(protocol["warmup_ticks"]) or measure > int(protocol["measure_ticks"]):
        raise EvidenceError("refusing to run beyond the frozen 2 + 10 minute bound")

    executable = find_bench_executable(workspace, smoke=mode == "smoke")
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [
        str(executable),
        "--mode",
        mode,
        "--warmup-ticks",
        str(warmup),
        "--measure-ticks",
        str(measure),
        "--max-wall-seconds",
        str(max_wall),
        "--workspace",
        str(workspace),
        "--output",
        str(output),
    ]
    peak_rss = None
    with subprocess.Popen(
        command,
        cwd=workspace,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    ) as process:
        try:
            while True:
                rss = sample_rss_bytes(process.pid)
                if rss is not None:
                    peak_rss = rss if peak_rss is None else max(peak_rss, rss)
                try:
                    process.wait(timeout=0.25)
                    break
                except subprocess.TimeoutExpired:
                    continue
        except Exception:
            process.kill()
            raise
        stderr = process.stderr.read() if process.stderr is not None else ""
        if process.returncode != 0:
            raise EvidenceError(
                f"performance harness exited {process.returncode}: {stderr.strip()}"
            )
    observation = load_observation(output)
    return observation, peak_rss


def emit_evidence(
    workspace: Path,
    *,
    mode: str,
    output_dir: Path,
    reference_host_run: bool,
) -> Path:
    """Run one bounded mode and write observation plus evidence JSON."""

    profile = load_profile(workspace / PROFILE_PATH)
    observation_path = output_dir / f"performance-observation-{mode}.json"
    evidence_path = output_dir / f"performance-evidence-{mode}.json"
    observation, ram_bytes = run_harness(
        workspace,
        mode=mode,
        output=observation_path,
        profile=profile,
    )
    payload = observation_path.read_bytes()
    fingerprints = capture_fingerprints(
        workspace,
        observation,
        cargo_profile="dev" if mode == "smoke" else "bench",
        lto="off" if mode == "smoke" else "thin",
        codegen_units=None if mode == "smoke" else 1,
    )
    evidence = evaluate_observation(
        profile,
        observation,
        fingerprints,
        ram_bytes=ram_bytes,
        observation_sha256=sha256_bytes(payload),
        reference_host_run=reference_host_run,
        run_count=1,
    )
    validate_evidence(evidence)
    write_json(evidence_path, evidence)
    return evidence_path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Parse the evidence CLI."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--workspace",
        type=Path,
        default=Path.cwd(),
        help="implementation workspace root",
    )
    sub = parser.add_subparsers(dest="command", required=True)
    smoke = sub.add_parser("smoke", help="short local verification run")
    smoke.add_argument("--output-dir", type=Path)
    smoke.add_argument("--reference-host", action="store_true")
    run = sub.add_parser("run", help="bounded traversal or smoke observation")
    run.add_argument("--mode", choices=("smoke", "traversal"), default="traversal")
    run.add_argument("--output-dir", type=Path, required=True)
    run.add_argument("--reference-host", action="store_true")
    evaluate = sub.add_parser("evaluate", help="evaluate an existing observation")
    evaluate.add_argument("--observation", type=Path, required=True)
    evaluate.add_argument("--output", type=Path, required=True)
    evaluate.add_argument("--reference-host", action="store_true")
    sub.add_parser("schema", help="validate the frozen profile and JSON schema")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """CLI entry."""

    args = parse_args(argv)
    workspace = args.workspace.resolve()
    try:
        if args.command == "schema":
            load_profile(workspace / PROFILE_PATH)
            if not (workspace / SCHEMA_PATH).is_file():
                raise EvidenceError(f"missing {SCHEMA_PATH}")
            print("desktop-reference-v1 profile and schema are present")
            return 0
        if args.command == "evaluate":
            profile = load_profile(workspace / PROFILE_PATH)
            observation = load_observation(args.observation)
            evidence = evaluate_observation(
                profile,
                observation,
                capture_fingerprints(workspace, observation),
                ram_bytes=observation["high_waters"].get("ram_bytes"),
                observation_sha256=sha256_file(args.observation),
                reference_host_run=args.reference_host,
                run_count=1,
            )
            validate_evidence(evidence)
            write_json(args.output, evidence)
            print(args.output)
            return 0
        output_dir = args.output_dir
        if output_dir is None:
            output_dir = Path(tempfile.mkdtemp(prefix="latticeaxiom-perf-"))
        output_dir = output_dir.resolve()
        mode = "smoke" if args.command == "smoke" else args.mode
        if args.reference_host:
            raise EvidenceError(
                "this runner does not accept --reference-host; a designated "
                "reference-host workflow must supply that identity"
            )
        evidence_path = emit_evidence(
            workspace,
            mode=mode,
            output_dir=output_dir,
            reference_host_run=False,
        )
        print(evidence_path)
        return 0
    except EvidenceError as error:
        print(error, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
