#!/usr/bin/env python3
"""Schema, budget, and verdict tests for the ADR 0026 evidence harness."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


sys.dont_write_bytecode = True

MODULE_PATH = Path(__file__).with_name("performance_evidence.py")
SPEC = importlib.util.spec_from_file_location("performance_evidence", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
performance_evidence = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = performance_evidence
SPEC.loader.exec_module(performance_evidence)


WORKSPACE = Path(__file__).resolve().parents[2]


def observation_fixture(**overrides: object) -> dict[object, object]:
    document: dict[object, object] = {
        "schema": performance_evidence.OBSERVATION_SCHEMA,
        "schema_version": 1,
        "mode": "smoke",
        "scenario": "headless-manual-time",
        "measurement_tool": performance_evidence.MEASUREMENT_TOOL,
        "histogram_version": performance_evidence.HISTOGRAM_VERSION,
        "protocol": {
            "warmup_ticks": 16,
            "measure_ticks": 128,
            "completed_warmup_ticks": 16,
            "completed_measure_ticks": 128,
            "fixed_hz": 60,
            "max_wall_seconds": 90,
            "wall_millis": 1200,
            "truncated_by_wall_cap": False,
        },
        "fingerprints": {
            "product_lock_hash": "a" * 64,
            "registration_semantic_hash": "b" * 64,
            "registration_image_hash": "c" * 64,
            "engine_build_id": None,
            "lock_file_sha256": "d" * 64,
            "shell_lock_file_sha256": None,
            "realization_target": "x86_64-pc-windows-msvc",
            "world_seed": 42,
        },
        "streaming_profile": {
            "schema": "latticeaxiom:schema/streaming-profile-evidence@1",
            "matches_adr_0026_world_space_coverage": False,
            "claims_d2_working_set_gate": False,
            "counts": {"requested": 12},
        },
        "histograms": {
            "fixed_update_cpu_ns": performance_evidence.summarize_samples([1_000_000] * 128),
            "resident_chunks": performance_evidence.summarize_samples([12] * 128),
            "active_chunks": performance_evidence.summarize_samples([8] * 128),
            "visible_chunks": performance_evidence.summarize_samples([8] * 128),
            "in_flight_chunks": performance_evidence.summarize_samples([2] * 128),
            "reserved_bytes": performance_evidence.summarize_samples([1024] * 128),
        },
        "high_waters": {
            "resident_chunks": 12,
            "active_chunks": 8,
            "visible_chunks": 8,
            "requested_chunks": 12,
            "in_flight_chunks": 2,
            "mesh_jobs": 4,
            "collider_jobs": 2,
            "combined_jobs": 6,
            "combined_reserved_bytes": 1024,
            "ram_bytes": None,
        },
        "queues": {
            "cancel_requests": 0,
            "apply_stopped_jobs": 0,
            "apply_stopped_bytes": 0,
            "apply_stopped_wall_clock": 0,
        },
        "path": {
            "start_translation_mm": [0, 16000, 0],
            "end_translation_mm": [8000, 16000, 0],
            "max_horizontal_displacement_mm": 8000,
            "unique_player_chunks": 2,
            "turn_180_executed": True,
            "edit_attempts": 16,
            "returned_toward_start": True,
        },
        "faults": {"truncated": False, "incomplete_measure": False},
    }
    document.update(overrides)
    return document


def fingerprints() -> dict[str, object]:
    return {
        "git_commit": "0" * 40,
        "git_dirty": True,
        "cargo_lock_sha256": "e" * 64,
        "rust_toolchain_channel": "1.97.1",
        "rustc_release": "1.97.1",
        "cargo_profile": "release",
        "lto": "thin",
        "codegen_units": 1,
        "cargo_features": [],
        "development_dynamic_linking": False,
        "os": "Windows",
        "arch": "AMD64",
        "product_lock_hash": "a" * 64,
        "registration_semantic_hash": "b" * 64,
        "registration_image_hash": "c" * 64,
        "engine_build_id": None,
        "lock_file_sha256": "d" * 64,
        "shell_lock_file_sha256": None,
        "realization_target": "x86_64-pc-windows-msvc",
        "world_seed": 42,
    }


class PerformanceEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.profile = performance_evidence.load_profile(
            WORKSPACE / "scripts/performance/desktop-reference-v1.toml"
        )

    def test_nearest_rank_known_vector(self) -> None:
        samples = list(range(1, 101))
        self.assertEqual(performance_evidence.nearest_rank(samples, 50), 50)
        self.assertEqual(performance_evidence.nearest_rank(samples, 95), 95)
        self.assertEqual(performance_evidence.nearest_rank(samples, 99), 99)
        self.assertIsNone(performance_evidence.nearest_rank([], 95))

    def test_profile_keeps_adr_0026_numbers(self) -> None:
        performance_evidence._assert_frozen_budgets(
            self.profile["budgets"], self.profile["protocol"]
        )
        self.assertEqual(self.profile["budgets"]["render"]["presented_frame_p95_ns"], 16_670_000)
        self.assertEqual(self.profile["protocol"]["measure_seconds"], 600)
        self.assertLess(
            self.profile["protocol"]["smoke_measure_ticks"],
            self.profile["protocol"]["measure_ticks"],
        )

    def test_easier_render_budget_is_rejected(self) -> None:
        budgets = copy.deepcopy(self.profile["budgets"])
        budgets["render"]["presented_frame_p95_ns"] = 33_340_000
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence._assert_frozen_budgets(budgets, self.profile["protocol"])

    def test_missing_and_unknown_observation_fields(self) -> None:
        missing = observation_fixture()
        del missing["histograms"]
        path = Path(tempfile.mkdtemp()) / "obs.json"
        path.write_text(json.dumps(missing), encoding="utf-8")
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.load_observation(path)
        extra = observation_fixture(unexpected=True)
        path.write_text(json.dumps(extra), encoding="utf-8")
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.load_observation(path)

    def test_queue_boundary_plus_one_fails(self) -> None:
        observed = observation_fixture()
        high = dict(observed["high_waters"])
        high["mesh_jobs"] = 129
        observed["high_waters"] = high
        evidence = performance_evidence.evaluate_observation(
            self.profile,
            observed,
            fingerprints(),
            ram_bytes=None,
            observation_sha256="f" * 64,
            reference_host_run=False,
            run_count=1,
        )
        self.assertEqual(evidence["metrics"]["mesh_queue_jobs"]["run_verdict"], "fail")
        self.assertEqual(evidence["overall"]["gate_verdict"], "fail")
        performance_evidence.validate_evidence(evidence)

    def test_hard_cap_exactly_at_budget_passes_run_metric(self) -> None:
        observed = observation_fixture()
        high = dict(observed["high_waters"])
        high["mesh_jobs"] = 128
        observed["high_waters"] = high
        evidence = performance_evidence.evaluate_observation(
            self.profile,
            observed,
            fingerprints(),
            ram_bytes=None,
            observation_sha256="f" * 64,
            reference_host_run=False,
            run_count=1,
        )
        self.assertEqual(evidence["metrics"]["mesh_queue_jobs"]["run_verdict"], "pass")
        self.assertEqual(evidence["overall"]["gate_verdict"], "insufficient-evidence")

    def test_twenty_percent_overrun_is_fail(self) -> None:
        observed = observation_fixture()
        hist = performance_evidence.summarize_samples([10_000_000] * 36000)
        histograms = dict(observed["histograms"])
        histograms["fixed_update_cpu_ns"] = hist
        observed["histograms"] = histograms
        observed["mode"] = "traversal"
        protocol = dict(observed["protocol"])
        protocol["measure_ticks"] = 36000
        protocol["completed_measure_ticks"] = 36000
        observed["protocol"] = protocol
        evidence = performance_evidence.evaluate_observation(
            self.profile,
            observed,
            fingerprints(),
            ram_bytes=None,
            observation_sha256="f" * 64,
            reference_host_run=False,
            run_count=1,
        )
        self.assertEqual(evidence["metrics"]["fixed_update_cpu"]["run_verdict"], "fail")
        self.assertIn("20%", evidence["metrics"]["fixed_update_cpu"]["reason"])

    def test_smoke_and_headless_cannot_pass_normative_or_render(self) -> None:
        evidence = performance_evidence.evaluate_observation(
            self.profile,
            observation_fixture(),
            fingerprints(),
            ram_bytes=None,
            observation_sha256="f" * 64,
            reference_host_run=False,
            run_count=1,
        )
        self.assertEqual(evidence["lifecycle_status"], "d2-provisional")
        self.assertEqual(evidence["metrics"]["presented_render_frame"]["run_verdict"], "unsupported")
        self.assertEqual(evidence["metrics"]["gpu_memory"]["run_verdict"], "unsupported")
        self.assertEqual(evidence["overall"]["gate_verdict"], "insufficient-evidence")
        self.assertFalse(evidence["normative_render_pass_permitted"])
        performance_evidence.validate_evidence(evidence)
        illegal = copy.deepcopy(evidence)
        illegal["metrics"]["presented_render_frame"]["run_verdict"] = "pass"
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.validate_evidence(illegal)
        frozen = copy.deepcopy(evidence)
        frozen["lifecycle_status"] = "d10-frozen"
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.validate_evidence(frozen)
        smoke_pass = copy.deepcopy(evidence)
        smoke_pass["overall"]["gate_verdict"] = "pass"
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.validate_evidence(smoke_pass)

    def test_zero_gpu_memory_is_not_a_pass(self) -> None:
        evidence = performance_evidence.evaluate_observation(
            self.profile,
            observation_fixture(),
            fingerprints(),
            ram_bytes=None,
            observation_sha256="f" * 64,
            reference_host_run=False,
            run_count=1,
        )
        self.assertIsNone(evidence["metrics"]["gpu_memory"]["high_water"])
        self.assertNotEqual(evidence["metrics"]["gpu_memory"]["high_water"], 0)
        self.assertEqual(evidence["metrics"]["gpu_memory"]["run_verdict"], "unsupported")

    def test_schema_command_accepts_workspace_profile(self) -> None:
        self.assertEqual(
            performance_evidence.main(["--workspace", str(WORKSPACE), "schema"]),
            0,
        )


if __name__ == "__main__":
    unittest.main()
