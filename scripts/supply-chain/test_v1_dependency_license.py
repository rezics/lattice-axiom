#!/usr/bin/env python3
"""Fixture and fault tests for v1_dependency_license.py."""

from __future__ import annotations

from datetime import date
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


sys.dont_write_bytecode = True

MODULE_PATH = Path(__file__).with_name("v1_dependency_license.py")
SPEC = importlib.util.spec_from_file_location("v1_dependency_license", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
policy = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = policy
SPEC.loader.exec_module(policy)


def sample_policy(*, reason: str, advisory_id: str = "RUSTSEC-2026-0247") -> dict:
    return {
        "advisories": {
            "ignore": [
                {
                    "id": advisory_id,
                    "reason": reason,
                }
            ]
        }
    }


ACTIVE_REASON = (
    "package=bitmaps@3.2.1; owner=core-runtime-maintainer; added=2026-08-20; "
    "expires=2026-09-19; mitigation=fail-closed Nickel worker; "
    "issue=https://rustsec.org/advisories/RUSTSEC-2026-0247; "
    "replace-when=Nickel removes bitmaps"
)


class AdvisoryPolicyTests(unittest.TestCase):
    def test_active_exception_is_accepted(self):
        count = policy.validate_advisory_exceptions(
            sample_policy(reason=ACTIVE_REASON),
            {"bitmaps@3.2.1"},
            today=date(2026, 8, 23),
        )
        self.assertEqual(count, 1)

    def test_expired_and_overlong_exceptions_fail(self):
        with self.assertRaisesRegex(policy.PolicyError, "not active"):
            policy.validate_advisory_exceptions(
                sample_policy(reason=ACTIVE_REASON),
                {"bitmaps@3.2.1"},
                today=date(2026, 9, 19),
            )
        overlong = ACTIVE_REASON.replace("expires=2026-09-19", "expires=2026-10-20")
        with self.assertRaisesRegex(policy.PolicyError, "exceeds 30 days"):
            policy.validate_advisory_exceptions(
                sample_policy(reason=overlong),
                {"bitmaps@3.2.1"},
                today=date(2026, 8, 23),
            )

    def test_unknown_package_and_http_issue_fail(self):
        with self.assertRaisesRegex(policy.PolicyError, "not in locked metadata"):
            policy.validate_advisory_exceptions(
                sample_policy(reason=ACTIVE_REASON),
                {"other@1.0.0"},
                today=date(2026, 8, 23),
            )
        http = ACTIVE_REASON.replace("https://", "http://")
        with self.assertRaisesRegex(policy.PolicyError, "HTTPS"):
            policy.validate_advisory_exceptions(
                sample_policy(reason=http),
                {"bitmaps@3.2.1"},
                today=date(2026, 8, 23),
            )

    def test_workspace_license_and_escaping_path_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            manifest = workspace / "Cargo.toml"
            manifest.write_text("[package]\nname='root'\n", encoding="utf-8")
            metadata = {
                "packages": [
                    {
                        "id": "root",
                        "name": "root",
                        "version": "0.1.0",
                        "source": None,
                        "manifest_path": str(manifest),
                        "license": "MIT",
                    }
                ],
                "workspace_members": ["root"],
            }
            with self.assertRaisesRegex(policy.PolicyError, "AGPL-3.0-only"):
                policy.validate_workspace_licenses(metadata)
            metadata["packages"][0]["license"] = "AGPL-3.0-only"
            policy.validate_workspace_licenses(metadata)
            metadata["packages"][0]["manifest_path"] = str(
                Path(temp).resolve().parent / "outside" / "Cargo.toml"
            )
            with self.assertRaisesRegex(policy.PolicyError, "escapes workspace"):
                policy.validate_path_dependencies(metadata, workspace)


class RepositoryContractTests(unittest.TestCase):
    def test_deny_toml_parses_and_pins_match_fixture(self):
        root = MODULE_PATH.resolve().parents[2]
        loaded = policy.load_deny_policy(root / "deny.toml")
        self.assertIn("licenses", loaded)
        fixture = (root / "fixtures" / "release" / "v1-reproduction.json").read_text(
            encoding="utf-8"
        )
        self.assertIn(policy.CARGO_DENY_VERSION, fixture)


if __name__ == "__main__":
    unittest.main(verbosity=2)
