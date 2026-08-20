#!/usr/bin/env python3
"""Fixture, boundary, and fault tests for release_evidence.py."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest


# Keep local and CI smoke runs from polluting the source tree.
sys.dont_write_bytecode = True

MODULE_PATH = Path(__file__).with_name("release_evidence.py")
SPEC = importlib.util.spec_from_file_location("release_evidence", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
release_evidence = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release_evidence
SPEC.loader.exec_module(release_evidence)


def configuration() -> release_evidence.Configuration:
    return release_evidence.Configuration(
        identifier="windows-x86_64-release",
        target="x86_64-pc-windows-msvc",
        profile="release",
        package="root",
        manifest_path=release_evidence.PurePosixPath("crates/root/Cargo.toml"),
        default_features=False,
        features=("portable",),
    )


def fixture_workspace(base: Path, *, license_expression: str | None = "AGPL-3.0-only"):
    workspace = base / "workspace"
    manifest = workspace / "crates" / "root" / "Cargo.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text("[package]\nname='root'\nversion='0.1.0'\n", encoding="utf-8")
    root_id = f"path+file:///{manifest.parent.as_posix()}#0.1.0"
    dep_id = "registry+https://github.com/rust-lang/crates.io-index#dep@1.2.3"
    dev_id = "registry+https://github.com/rust-lang/crates.io-index#dev-only@9.0.0"
    checksum = "a" * 64
    metadata = {
        "version": 1,
        "packages": [
            {
                "id": root_id,
                "name": "root",
                "version": "0.1.0",
                "source": None,
                "manifest_path": str(manifest),
                "license": license_expression,
            },
            {
                "id": dep_id,
                "name": "dep",
                "version": "1.2.3",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "manifest_path": str(base / "registry" / "dep" / "Cargo.toml"),
                "license": "MIT",
            },
            {
                "id": dev_id,
                "name": "dev-only",
                "version": "9.0.0",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "manifest_path": str(base / "registry" / "dev" / "Cargo.toml"),
                "license": "MIT",
            },
        ],
        "workspace_members": [root_id],
        "resolve": {
            "nodes": [
                {
                    "id": root_id,
                    "features": ["portable"],
                    "deps": [
                        {
                            "pkg": dep_id,
                            "dep_kinds": [{"kind": None, "target": None}],
                        },
                        {
                            "pkg": dev_id,
                            "dep_kinds": [{"kind": "dev", "target": None}],
                        },
                    ],
                },
                {"id": dep_id, "features": ["std"], "deps": []},
                {"id": dev_id, "features": [], "deps": []},
            ]
        },
    }
    lock = workspace / "Cargo.lock"
    lock.write_text(
        "\n".join(
            [
                "version = 4",
                "",
                "[[package]]",
                'name = "root"',
                'version = "0.1.0"',
                "",
                "[[package]]",
                'name = "dep"',
                'version = "1.2.3"',
                'source = "registry+https://github.com/rust-lang/crates.io-index"',
                f'checksum = "{checksum}"',
                "",
                "[[package]]",
                'name = "dev-only"',
                'version = "9.0.0"',
                'source = "registry+https://github.com/rust-lang/crates.io-index"',
                f'checksum = "{"b" * 64}"',
                "",
            ]
        ),
        encoding="utf-8",
    )
    return workspace, metadata, lock, root_id, dep_id, dev_id, checksum


def derive_fixture(base: Path):
    workspace, metadata, lock, root_id, dep_id, dev_id, checksum = fixture_workspace(base)
    closure = release_evidence.derive_closure(
        metadata, lock, workspace, configuration()
    )
    receipt, refs = release_evidence.build_closure_receipt(
        closure, workspace, configuration(), release_evidence.sha256_file(lock)
    )
    return workspace, closure, receipt, refs, root_id, dep_id, dev_id, checksum


def sample_sbom(root_id: str, dep_id: str, checksum: str):
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:11111111-1111-4111-8111-111111111111",
        "version": 1,
        "metadata": {
            "timestamp": "2030-01-01T00:00:00Z",
            "tools": [
                {
                    "vendor": "CycloneDX",
                    "name": "cargo-cyclonedx",
                    "version": "0.5.9",
                }
            ],
            "component": {
                "type": "application",
                "bom-ref": root_id,
                "name": "root",
                "version": "0.1.0",
                "purl": "pkg:cargo/root@0.1.0?download_url=file://.",
                "components": [
                    {
                        "type": "application",
                        "bom-ref": root_id + " bin-target-0",
                        "name": "root",
                        "version": "0.1.0",
                        "purl": "pkg:cargo/root@0.1.0?download_url=file://.#src/main.rs",
                    }
                ],
            },
            "properties": [
                {
                    "name": "cdx:rustc:sbom:target:triple",
                    "value": "x86_64-pc-windows-msvc",
                }
            ],
        },
        "components": [
            {
                "type": "library",
                "bom-ref": dep_id,
                "name": "dep",
                "version": "1.2.3",
                "hashes": [{"alg": "SHA-256", "content": checksum}],
                "licenses": [{"expression": "MIT"}],
                "purl": "pkg:cargo/dep@1.2.3",
            }
        ],
        "dependencies": [
            {"ref": dep_id, "dependsOn": []},
            {"ref": root_id, "dependsOn": [dep_id]},
        ],
    }


def package_for_about(closure, package_id):
    package = closure.package_by_id[package_id]
    return {
        "id": package_id,
        "name": package["name"],
        "version": package["version"],
        "source": package["source"],
    }


def sample_about(closure, root_id, dep_id):
    root = package_for_about(closure, root_id)
    dep = package_for_about(closure, dep_id)
    return {
        "overview": [],
        "crates": [
            {"package": root, "license": "AGPL-3.0-only"},
            {"package": dep, "license": "MIT"},
        ],
        "licenses": [
            {
                "id": "AGPL-3.0-only",
                "name": "GNU Affero General Public License v3.0 only",
                "first_of_kind": True,
                "text": "AGPL fixture text\n",
                "source_path": "/machine/root/LICENSE",
                "used_by": [{"crate": root, "path": "/machine/root"}],
            },
            {
                "id": "MIT",
                "name": "MIT License",
                "first_of_kind": True,
                "text": "MIT fixture text\r\n",
                "source_path": "/machine/registry/dep/LICENSE",
                "used_by": [{"crate": dep, "path": "/machine/registry/dep"}],
            },
        ],
    }


class ContractTests(unittest.TestCase):
    def test_repository_contract_is_valid(self):
        root = MODULE_PATH.resolve().parents[2]
        contract = release_evidence.load_contract(
            root / "supply-chain" / "release-evidence.toml"
        )
        self.assertEqual(
            [item.identifier for item in contract.configurations],
            ["linux-x86_64-release", "windows-x86_64-release"],
        )
        self.assertEqual(contract.tools["cargo-cyclonedx"], "0.5.9")

    def test_about_policy_keeps_private_attribution_and_reviewed_dyn_eq(self):
        root = MODULE_PATH.resolve().parents[2]
        policy = tomllib.loads(
            (root / "supply-chain" / "about.toml").read_text(encoding="utf-8")
        )
        self.assertFalse(policy["private"]["ignore"])
        self.assertFalse(policy["ignore-build-dependencies"])
        self.assertEqual(policy["dyn-eq"]["accepted"], ["MPL-2.0"])
    def test_missing_workspace_about_policy_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            _, metadata, _, *_ = fixture_workspace(base)
            policy = base / "about.toml"
            policy.write_text(
                """accepted = ["MIT"]
ignore-build-dependencies = false
ignore-dev-dependencies = true
ignore-transitive-dependencies = false
private = { ignore = false }

[root]
accepted = ["AGPL-3.0-only"]
""",
                encoding="utf-8",
            )
            release_evidence.validate_about_workspace_policy(metadata, policy)
            policy.write_text(
                policy.read_text(encoding="utf-8").replace(
                    "\n[root]\naccepted = [\"AGPL-3.0-only\"]\n", "\n"
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                release_evidence.EvidenceError, "policy is incomplete"
            ):
                release_evidence.validate_about_workspace_policy(metadata, policy)
    def test_rejects_duplicate_configuration_and_unpinned_tool(self):
        source = """
schema = 1
[tools]
cargo-cyclonedx = "0.5"
cargo-about = "0.9.1"
cargo-deny = "0.20.2"
cyclonedx-spec = "1.5"
[[configurations]]
id = "same"
target = "x86_64-pc-windows-msvc"
profile = "release"
package = "root"
manifest-path = "crates/root/Cargo.toml"
default-features = false
features = []
"""
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "contract.toml"
            path.write_text(source, encoding="utf-8")
            with self.assertRaisesRegex(release_evidence.EvidenceError, "exact version"):
                release_evidence.load_contract(path)

    def test_rejects_unsafe_manifest_and_unsorted_features(self):
        source = """
schema = 1
[tools]
cargo-cyclonedx = "0.5.9"
cargo-about = "0.9.1"
cargo-deny = "0.20.2"
cyclonedx-spec = "1.5"
[[configurations]]
id = "same"
target = "x86_64-pc-windows-msvc"
profile = "release"
package = "root"
manifest-path = "../Cargo.toml"
default-features = false
features = ["z", "a"]
"""
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "contract.toml"
            path.write_text(source, encoding="utf-8")
            with self.assertRaisesRegex(release_evidence.EvidenceError, "unsafe"):
                release_evidence.load_contract(path)


class ClosureTests(unittest.TestCase):
    def test_excludes_dev_edge_and_records_features_from_metadata(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, receipt, _, root_id, dep_id, dev_id, _ = derive_fixture(Path(temp))
            self.assertEqual(closure.ids, {root_id, dep_id})
            self.assertNotIn(dev_id, closure.ids)
            self.assertEqual(closure.edges, {(root_id, dep_id)})
            root_record = next(
                item
                for item in receipt["receipt"]["packages"]
                if item["name"] == "root"
            )
            self.assertEqual(root_record["features"], ["portable"])

    def test_missing_workspace_license_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, metadata, lock, *_ = fixture_workspace(
                Path(temp), license_expression=None
            )
            with self.assertRaisesRegex(
                release_evidence.EvidenceError, "must declare AGPL-3.0-only"
            ):
                release_evidence.derive_closure(
                    metadata, lock, workspace, configuration()
                )

    def test_registry_checksum_and_unique_lock_entry_are_required(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, metadata, lock, *_ = fixture_workspace(Path(temp))
            lock.write_text(
                lock.read_text(encoding="utf-8").replace('checksum = "' + "a" * 64 + '"', ""),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(release_evidence.EvidenceError, "checksum"):
                release_evidence.derive_closure(
                    metadata, lock, workspace, configuration()
                )


class CargoTreeTests(unittest.TestCase):
    def test_depth_tree_records_actual_packages_edges_and_features(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, metadata, _, root_id, dep_id, _, _ = fixture_workspace(
                Path(temp)
            )
            root_path = workspace / "crates" / "root"
            output = (
                f"0|root v0.1.0 ({root_path})|portable\n"
                "1|dep v1.2.3|std\n"
            )

            ids, edges, features = release_evidence.parse_cargo_tree(
                output, metadata, workspace, configuration()
            )

            self.assertEqual(ids, {root_id, dep_id})
            self.assertEqual(edges, {(root_id, dep_id)})
            self.assertEqual(features[root_id], ("portable",))
            self.assertEqual(features[dep_id], ("std",))

    def test_depth_jump_and_unknown_package_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, metadata, _, _, _, _, _ = fixture_workspace(Path(temp))
            root_path = workspace / "crates" / "root"
            root_line = f"0|root v0.1.0 ({root_path})|portable\n"
            with self.assertRaisesRegex(
                release_evidence.EvidenceError, "depth jumps"
            ):
                release_evidence.parse_cargo_tree(
                    root_line + "2|dep v1.2.3|std\n",
                    metadata,
                    workspace,
                    configuration(),
                )
            with self.assertRaisesRegex(
                release_evidence.EvidenceError, "not uniquely mapped"
            ):
                release_evidence.parse_cargo_tree(
                    root_line + "1|not-locked v9.9.9|\n",
                    metadata,
                    workspace,
                    configuration(),
                )

class SbomTests(unittest.TestCase):
    def test_package_edge_checksum_target_and_tool_parity(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, _, _, root_id, dep_id, _, checksum = derive_fixture(Path(temp))
            sbom = sample_sbom(root_id, dep_id, checksum)
            release_evidence.validate_sbom_against_closure(
                sbom, closure, configuration(), "0.5.9"
            )

    def test_selects_actual_tree_from_generator_superset(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, _, _, root_id, dep_id, dev_id, checksum = derive_fixture(
                Path(temp)
            )
            sbom = sample_sbom(root_id, dep_id, checksum)
            sbom["components"].append(
                {
                    "type": "library",
                    "bom-ref": dev_id,
                    "name": "dev-only",
                    "version": "9.0.0",
                }
            )
            sbom["dependencies"].append({"ref": dev_id, "dependsOn": []})
            sbom["dependencies"][1]["dependsOn"].append(dev_id)

            selected = release_evidence.select_sbom_closure(sbom, closure)

            release_evidence.validate_sbom_against_closure(
                selected, closure, configuration(), "0.5.9"
            )
            self.assertNotIn(dev_id, json.dumps(selected))

    def test_raw_generator_must_cover_actual_tree_edges(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, _, _, root_id, dep_id, _, checksum = derive_fixture(Path(temp))
            sbom = sample_sbom(root_id, dep_id, checksum)
            sbom["dependencies"][1]["dependsOn"] = []
            with self.assertRaisesRegex(
                release_evidence.EvidenceError, "omits edges"
            ):
                release_evidence.select_sbom_closure(sbom, closure)
    def test_missing_package_fails_bidirectional_check(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, _, _, root_id, dep_id, _, checksum = derive_fixture(Path(temp))
            sbom = sample_sbom(root_id, dep_id, checksum)
            sbom["components"] = []
            with self.assertRaisesRegex(release_evidence.EvidenceError, "package closure"):
                release_evidence.validate_sbom_against_closure(
                    sbom, closure, configuration(), "0.5.9"
                )

    def test_extra_or_missing_edge_fails(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, _, _, root_id, dep_id, _, checksum = derive_fixture(Path(temp))
            sbom = sample_sbom(root_id, dep_id, checksum)
            sbom["dependencies"][1]["dependsOn"] = []
            with self.assertRaisesRegex(release_evidence.EvidenceError, "edges differ"):
                release_evidence.validate_sbom_against_closure(
                    sbom, closure, configuration(), "0.5.9"
                )

    def test_wrong_checksum_target_and_tool_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            _, closure, _, _, root_id, dep_id, _, checksum = derive_fixture(Path(temp))
            for mutation, message in [
                (
                    lambda sbom: sbom["components"][0]["hashes"][0].update(
                        content="c" * 64
                    ),
                    "checksum differs",
                ),
                (
                    lambda sbom: sbom["metadata"]["properties"][0].update(
                        value="x86_64-unknown-linux-gnu"
                    ),
                    "target property differs",
                ),
                (
                    lambda sbom: sbom["metadata"]["tools"][0].update(version="0.5.8"),
                    "generator version differs",
                ),
            ]:
                sbom = sample_sbom(root_id, dep_id, checksum)
                mutation(sbom)
                with self.assertRaisesRegex(release_evidence.EvidenceError, message):
                    release_evidence.validate_sbom_against_closure(
                        sbom, closure, configuration(), "0.5.9"
                    )

    def test_normalization_is_path_order_time_and_uuid_independent(self):
        outputs = []
        for suffix in ["first", "second"]:
            with tempfile.TemporaryDirectory(suffix=suffix) as temp:
                workspace, closure, receipt, refs, root_id, dep_id, _, checksum = derive_fixture(
                    Path(temp)
                )
                sbom = sample_sbom(root_id, dep_id, checksum)
                sbom["components"].reverse()
                sbom["dependencies"].reverse()
                sbom["metadata"]["timestamp"] = f"2030-01-0{len(outputs) + 1}T00:00:00Z"
                sbom["serialNumber"] = (
                    f"urn:uuid:{len(outputs) + 1}1111111-1111-4111-8111-111111111111"
                )
                normalized = release_evidence.normalize_sbom(
                    sbom,
                    refs,
                    configuration(),
                    "d" * 64,
                    receipt["receipt_sha256"],
                    workspace,
                )
                rendered = release_evidence.canonical_json_bytes(normalized)
                self.assertNotIn(str(workspace).encode(), rendered)
                self.assertNotIn(b"path+file:", rendered)
                self.assertNotIn(b"timestamp", rendered)
                self.assertNotIn(b"serialNumber", rendered)
                self.assertNotIn(b"download_url=file", rendered)
                outputs.append(rendered)
        self.assertEqual(outputs[0], outputs[1])


class NoticeTests(unittest.TestCase):
    def test_notice_coverage_and_order_are_deterministic(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, closure, _, refs, root_id, dep_id, _, _ = derive_fixture(Path(temp))
            about = sample_about(closure, root_id, dep_id)
            first = release_evidence.validate_and_render_notices(
                about,
                closure,
                workspace,
                refs,
                configuration(),
                "d" * 64,
            )
            reordered = copy.deepcopy(about)
            reordered["crates"].reverse()
            reordered["licenses"].reverse()
            second = release_evidence.validate_and_render_notices(
                reordered,
                closure,
                workspace,
                refs,
                configuration(),
                "d" * 64,
            )
            self.assertEqual(first, second)
            self.assertIn(b"dep 1.2.3", first)
            self.assertIn(b"MIT fixture text", first)
            self.assertNotIn(b"/machine/", first)
            self.assertIn(b"content assets are not included", first)

    def test_unknown_ignored_and_empty_license_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, closure, _, refs, root_id, dep_id, _, _ = derive_fixture(Path(temp))
            for bad_value in ["Unknown", "Ignore", ""]:
                about = sample_about(closure, root_id, dep_id)
                about["crates"][1]["license"] = bad_value
                with self.assertRaisesRegex(
                    release_evidence.EvidenceError, "unresolved license"
                ):
                    release_evidence.validate_and_render_notices(
                        about,
                        closure,
                        workspace,
                        refs,
                        configuration(),
                        "d" * 64,
                    )
            about = sample_about(closure, root_id, dep_id)
            about["licenses"][1]["text"] = ""
            with self.assertRaisesRegex(release_evidence.EvidenceError, "incomplete"):
                release_evidence.validate_and_render_notices(
                    about,
                    closure,
                    workspace,
                    refs,
                    configuration(),
                    "d" * 64,
                )

    def test_missing_and_out_of_closure_attribution_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace, closure, _, refs, root_id, dep_id, _, _ = derive_fixture(Path(temp))
            about = sample_about(closure, root_id, dep_id)
            about["licenses"][1]["used_by"] = []
            with self.assertRaisesRegex(release_evidence.EvidenceError, "lack license-text"):
                release_evidence.validate_and_render_notices(
                    about,
                    closure,
                    workspace,
                    refs,
                    configuration(),
                    "d" * 64,
                )
            about = sample_about(closure, root_id, dep_id)
            about["licenses"][1]["used_by"].append(
                {"crate": {"id": "registry+unknown#bad@1.0.0"}, "path": None}
            )
            with self.assertRaisesRegex(release_evidence.EvidenceError, "out-of-closure"):
                release_evidence.validate_and_render_notices(
                    about,
                    closure,
                    workspace,
                    refs,
                    configuration(),
                    "d" * 64,
                )


class ManifestTests(unittest.TestCase):
    def test_manifest_hashes_every_payload_and_detects_tampering(self):
        contract = release_evidence.Contract(
            tools={
                "cargo-about": "0.9.1",
                "cargo-cyclonedx": "0.5.9",
                "cargo-deny": "0.20.2",
                "cyclonedx-spec": "1.5",
            },
            configurations=(configuration(),),
        )
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "evidence"
            release_evidence._write_evidence(
                output,
                configuration(),
                b"{}\n",
                b"notices\n",
                b"{}\n",
                contract,
                "a" * 64,
                "b" * 64,
            )
            release_evidence.verify_evidence_directory(output)
            manifest_path = next(output.glob("release-evidence-manifest-*.json"))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertFalse(manifest["content_assets_included"])
            self.assertFalse(manifest["binary_included"])
            self.assertEqual(len(manifest["files"]), 3)
            payload = output / manifest["files"][0]["name"]
            payload.write_bytes(payload.read_bytes() + b"tamper")
            with self.assertRaisesRegex(release_evidence.EvidenceError, "differs"):
                release_evidence.verify_evidence_directory(output)


if __name__ == "__main__":
    unittest.main(verbosity=2)
