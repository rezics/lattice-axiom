"""Contract tests for the pinned local Logdy development tool."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import tempfile
import unittest

import logdy_dev


class LogdyManifestTests(unittest.TestCase):
    def test_checked_in_manifest_selects_every_supported_host(self) -> None:
        manifest = logdy_dev.load_manifest()
        expected = {
            ("windows", "x86_64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("darwin", "x86_64"),
            ("darwin", "aarch64"),
        }

        self.assertEqual(
            {(artifact.platform, artifact.architecture) for artifact in manifest.artifacts},
            expected,
        )
        for identity in expected:
            self.assertEqual(len(logdy_dev.select_artifact(manifest, identity).sha256), 64)

    def test_manifest_rejects_an_unproved_digest(self) -> None:
        source = Path(logdy_dev.__file__).with_name(logdy_dev.MANIFEST_NAME)
        document = json.loads(source.read_text(encoding="utf-8"))
        document["artifacts"][0]["sha256"] = "not-a-digest"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "manifest.json"
            path.write_text(json.dumps(document), encoding="utf-8")

            with self.assertRaisesRegex(logdy_dev.ObservabilityError, "sha256"):
                logdy_dev.load_manifest(path)

    def test_manifest_rejects_boolean_artifact_sizes(self) -> None:
        source = Path(logdy_dev.__file__).with_name(logdy_dev.MANIFEST_NAME)
        document = json.loads(source.read_text(encoding="utf-8"))
        document["artifacts"][0]["size"] = True
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "manifest.json"
            path.write_text(json.dumps(document), encoding="utf-8")

            with self.assertRaisesRegex(logdy_dev.ObservabilityError, "positive integer"):
                logdy_dev.load_manifest(path)

    def test_host_labels_are_normalized_before_selection(self) -> None:
        self.assertEqual(logdy_dev.normalized_host("win32", "AMD64"), ("windows", "x86_64"))
        self.assertEqual(logdy_dev.normalized_host("linux", "aarch64"), ("linux", "aarch64"))
        self.assertEqual(logdy_dev.normalized_host("darwin", "arm64"), ("darwin", "aarch64"))

    def test_ports_are_bounded_and_exit_codes_preserve_failures(self) -> None:
        self.assertEqual(logdy_dev.bounded_port("8080"), 8080)
        with self.assertRaisesRegex(argparse.ArgumentTypeError, "1024"):
            logdy_dev.bounded_port("80")
        self.assertEqual(logdy_dev.normalized_exit_code(7), 7)
        self.assertEqual(logdy_dev.normalized_exit_code(-2), 130)

    def test_old_rotated_logs_are_pruned_to_the_retention_bound(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "dev.jsonl").write_text("current", encoding="utf-8")
            for serial in range(8):
                (directory / f"dev.{serial}.jsonl").write_text(str(serial), encoding="utf-8")

            logdy_dev.prune_rotated_logs(directory, keep=5)

            self.assertTrue((directory / "dev.jsonl").is_file())
            self.assertEqual(len(list(directory.glob("dev.*.jsonl"))), 5)


if __name__ == "__main__":
    unittest.main()
