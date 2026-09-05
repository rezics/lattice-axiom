"""Compiler-source closure tests using a dependency-free native fixture."""
from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest

from native_product import build, prepare, source_closure, verify_sources


def fixture(root: Path) -> Path:
    files = {
        'Cargo.toml': '''[workspace]
resolver = "3"
members = ["packages/example/app/crates/application"]
''',
        'Cargo.lock': '''version = 4
[[package]]
name = "native-fixture"
version = "0.1.0"
''',
        'packages/example/app/latticeaxiom-package.toml': '''name = "@example/app"
version = "0.1.0"
trust = "trusted-native"
[realizations.native-static]
kind = "native-static"
artifact = { kind = "source-build" }
[source_inclusion]
include = ["latticeaxiom-package.toml", "crates"]
[rust]
entry = "crates/application/Cargo.toml"
members = ["crates/application/Cargo.toml"]
binaries = ["native-fixture"]
''',
        'packages/example/app/crates/application/Cargo.toml': '''[package]
name = "native-fixture"
version = "0.1.0"
edition = "2024"
[[bin]]
name = "native-fixture"
path = "src/main.rs"
[package.metadata.latticeaxiom]
owner = "@example/app"
''',
        'packages/example/app/crates/application/src/main.rs': 'fn main() { println!("frozen fixture"); }\n',
        'product.toml': '''schema_version = 1
name = "native-fixture"
root = "@example/app"
features = []
launch = "native-fixture"
build_profile = "dev"
''',
    }
    for name, content in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding='utf-8')
    return root / 'product.toml'


class NativeProductTests(unittest.TestCase):
    def test_selection_is_the_manifest_dependency_closure(self):
        self.assertEqual(source_closure({'app': {'core'}, 'core': set(), 'unused': set()}, 'app'),
                         ['core', 'app'])

    def test_source_changes_cannot_rewrite_a_frozen_product(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / 'repo'
            profile = fixture(root)
            source, plan = prepare(root, profile, base / 'cache')
            main = Path('packages/example/app/crates/application/src/main.rs')
            (root / main).write_text('compile_error!("mutable source used");', encoding='utf-8')
            # The producer compiles only the frozen owner closure, not the checkout.
            receipt = build(source, plan, base / 'target')
            executable = receipt['artifacts']['native-fixture']['path']
            output = subprocess.check_output([executable], text=True)
            self.assertEqual(output.strip(), 'frozen fixture')
            (source / main).write_text('fn main() {}', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'frozen native product source inputs were modified'):
                verify_sources(source, plan)

    def test_source_inclusion_cannot_escape_its_package(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'repo'
            profile = fixture(root)
            manifest = root / 'packages/example/app/latticeaxiom-package.toml'
            text = manifest.read_text(encoding='utf-8').replace('"crates"]', '"../outside"]')
            manifest.write_text(text, encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'non-local source path'):
                prepare(root, profile, Path(directory) / 'cache')


if __name__ == '__main__':
    unittest.main()
