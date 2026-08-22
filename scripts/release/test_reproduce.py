#!/usr/bin/env python3
"""Fixture, pin, budget, and report tests for reproduce.py."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest


sys.dont_write_bytecode = True

MODULE_PATH = Path(__file__).with_name("reproduce.py")
SPEC = importlib.util.spec_from_file_location("v1_reproduce", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
reproduce = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = reproduce
SPEC.loader.exec_module(reproduce)

ROOT = MODULE_PATH.resolve().parents[2]


def load_fixture() -> dict:
    return json.loads(
        (ROOT / "fixtures" / "release" / "v1-reproduction.json").read_text(encoding="utf-8")
    )


class ContractTests(unittest.TestCase):
    def test_repository_fixture_matches_frozen_adr_0026(self):
        contract = reproduce.load_contract(ROOT / "fixtures" / "release" / "v1-reproduction.json")
        self.assertEqual(contract["budgets"], reproduce.FROZEN_ADR_0026_BUDGETS)
        self.assertEqual(contract["budgets"]["frame_p95_ms"], 16.67)
        self.assertEqual(contract["budgets"]["resident_chunks"], 1183)
        self.assertEqual(contract["budgets"]["queues"]["combined"]["jobs"], 512)
        self.assertFalse(contract["claims_d10_frozen"])
        self.assertEqual(contract["product_entry"], "task play")
        job_ids = [job["id"] for job in contract["jobs"]]
        self.assertLess(job_ids.index("offline-frozen-lock"), job_ids.index("test"))
        self.assertLess(job_ids.index("offline-frozen-lock"), job_ids.index("headless-smoke"))

    def test_desktop_reference_profile_does_not_relax_adr_0026(self):
        path = ROOT / "scripts" / "performance" / "desktop-reference-v1.toml"
        profile = tomllib.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(profile["profile"], "desktop-reference-v1")
        self.assertEqual(profile["lifecycle_until_reference_host_freeze"], "d2-provisional")
        working = profile["budgets"]["working_set"]
        self.assertEqual(working["chunk_edge_voxels"], 32)
        self.assertEqual(working["active_chunks_max"], 405)
        self.assertEqual(working["resident_chunks_max"], 1183)
        self.assertEqual(working["visible_chunks_max"], 512)
        self.assertEqual(working["requested_prefetch_chunks_max"], 1536)
        queues = profile["budgets"]["queues"]
        self.assertEqual(queues["combined_jobs"], 512)
        self.assertEqual(queues["apply_wall_ns"], 2_000_000)
        self.assertEqual(profile["budgets"]["render"]["presented_frame_p95_ns"], 16_670_000)

    def test_easier_frame_budget_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "contract.json"
            fixture = load_fixture()
            fixture["budgets"]["frame_p95_ms"] = 20.0
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(reproduce.ReproductionError, "ADR 0026"):
                reproduce.load_contract(path)

    def test_reduced_resident_or_disabled_backpressure_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "contract.json"
            fixture = load_fixture()
            fixture["budgets"]["resident_chunks"] = 64
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(reproduce.ReproductionError, "ADR 0026"):
                reproduce.load_contract(path)
            fixture = load_fixture()
            fixture["budgets"]["queues"]["combined"]["jobs"] = 4096
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(reproduce.ReproductionError, "ADR 0026"):
                reproduce.load_contract(path)

    def test_fetch_publish_and_docs_evidence_commands_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "contract.json"
            fixture = load_fixture()
            fixture["jobs"][0]["steps"] = [{"argv": ["cargo", "fetch", "--locked"]}]
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(reproduce.ReproductionError, "forbidden"):
                reproduce.load_contract(path)

    def test_d10_claim_and_unpinned_action_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "contract.json"
            fixture = load_fixture()
            fixture["claims_d10_frozen"] = True
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(reproduce.ReproductionError, "D10"):
                reproduce.load_contract(path)
            fixture = load_fixture()
            fixture["actions"]["actions/checkout"]["pin"] = "v4.2.2"
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(reproduce.ReproductionError, "40-character SHA"):
                reproduce.load_contract(path)


class WorkflowPinTests(unittest.TestCase):
    def test_v1_workflow_uses_fixture_action_pins(self):
        contract = reproduce.load_contract(ROOT / "fixtures" / "release" / "v1-reproduction.json")
        workflow = ROOT / ".github" / "workflows" / "v1-release-gate.yml"
        reproduce.pin_workflow_actions(workflow, contract)
        text = workflow.read_text(encoding="utf-8")
        self.assertIn("Linux independent root gate", text)
        self.assertIn("continue-on-error: true", text)
        self.assertNotIn("cargo fetch", text)
        self.assertNotIn("gh release", text)
        self.assertNotIn("docs/delivery/evidence", text)
        existing = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        for name, spec in contract["actions"].items():
            if name in {"actions/checkout", "Swatinem/rust-cache"}:
                self.assertIn(spec["pin"], existing)
                self.assertIn(spec["pin"], text)


class NormalizationTests(unittest.TestCase):
    def test_logs_are_path_and_newline_independent(self):
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            outputs = []
            for directory in (first, second):
                workspace = Path(directory) / "workspace"
                workspace.mkdir()
                text = f"{workspace}\\target\\debug\\app.exe\r\n{workspace.as_posix()}/target\n"
                outputs.append(reproduce.normalize_text(text, workspace))
            self.assertEqual(outputs[0], outputs[1])
            self.assertNotIn("\r", outputs[0])
            self.assertIn("${workspace}", outputs[0])

    def test_pass_requires_executed_command(self):
        contract = reproduce.load_contract(ROOT / "fixtures" / "release" / "v1-reproduction.json")
        matrix = contract["matrix"][0]
        job = {
            "designated_runner": False,
            "evidence_class": "lint",
            "id": "fmt",
            "kind": "lint",
            "notes": None,
            "production": False,
            "profile_included": True,
            "reason": "file exists",
            "required": True,
            "status": "pass",
            "steps": [
                {
                    "argv": ["cargo", "fmt", "--all", "--check"],
                    "executed": False,
                    "exit_code": 0,
                    "reason": "file exists",
                    "status": "pass",
                }
            ],
        }
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaisesRegex(reproduce.ReproductionError, "executed"):
                reproduce.write_report(
                    Path(temp) / "out",
                    contract=contract,
                    profile="ci-headless",
                    matrix=matrix,
                    jobs=[job],
                    workspace=ROOT,
                    complete_profile=False,
                )

    def test_skipped_required_job_does_not_pass_the_profile(self):
        contract = reproduce.load_contract(ROOT / "fixtures" / "release" / "v1-reproduction.json")
        matrix = contract["matrix"][0]
        job = {
            "designated_runner": False,
            "evidence_class": "lint",
            "id": "fmt",
            "kind": "lint",
            "notes": None,
            "production": False,
            "profile_included": True,
            "reason": "not executed",
            "required": True,
            "status": "not-run",
            "steps": [
                {
                    "argv": [],
                    "executed": False,
                    "exit_code": None,
                    "reason": "not executed",
                    "status": "not-run",
                }
            ],
        }
        with tempfile.TemporaryDirectory() as temp:
            report = reproduce.write_report(
                Path(temp) / "out",
                contract=contract,
                profile="ci-headless",
                matrix=matrix,
                jobs=[job],
                workspace=ROOT,
                complete_profile=False,
            )
            self.assertEqual(report["outcome"], "fail")

    def test_report_sidecar_detects_tampering(self):
        contract = reproduce.load_contract(ROOT / "fixtures" / "release" / "v1-reproduction.json")
        matrix = contract["matrix"][0]
        job = {
            "designated_runner": False,
            "evidence_class": "lint",
            "id": "fmt",
            "kind": "lint",
            "notes": None,
            "production": False,
            "profile_included": True,
            "reason": None,
            "required": True,
            "status": "pass",
            "steps": [
                {
                    "argv": ["cargo", "fmt", "--all", "--check"],
                    "executed": True,
                    "exit_code": 0,
                    "output": "ok\n",
                    "reason": None,
                    "status": "pass",
                }
            ],
        }
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "out"
            reproduce.write_report(
                output,
                contract=contract,
                profile="ci-headless",
                matrix=matrix,
                jobs=[copy.deepcopy(job)],
                workspace=ROOT,
                complete_profile=False,
            )
            reproduce.verify_report_directory(output)
            payload = output / "jobs" / "fmt.log"
            payload.write_bytes(payload.read_bytes() + b"tamper")
            with self.assertRaisesRegex(reproduce.ReproductionError, "hash differs"):
                reproduce.verify_report_directory(output)


if __name__ == "__main__":
    unittest.main(verbosity=2)
