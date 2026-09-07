"""Compiler-source closure tests using a dependency-free native fixture."""
from __future__ import annotations

from pathlib import Path
import json
import shutil
import subprocess
import tempfile
import unittest

from native_product import build, build_web, prepare, publish_web_assets, source_closure, verify_sources
from package_graph import source_package_graph


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


def web_fixture(root: Path) -> Path:
    profile = fixture(root)
    app = root / 'packages/example/app/latticeaxiom-package.toml'
    app.write_text(app.read_text(encoding='utf-8') + '\n[web.dependencies]\n"@example/ui" = "=0.1.0"\n', encoding='utf-8')
    web = root / 'packages/example/ui'
    web.mkdir(parents=True)
    (web / 'latticeaxiom-package.toml').write_text('''name = "@example/ui"
version = "0.1.0"
trust = "trusted-native"
[realizations.native-static]
kind = "native-static"
artifact = { kind = "source-build" }
[source_inclusion]
include = ["latticeaxiom-package.toml", "index.html", "build.mjs", "package.json", "package-lock.json"]
[web]
entry = "index.html"
package = "package.json"
lock = "package-lock.json"
output = "dist"
''', encoding='utf-8')
    (web / 'index.html').write_text('<!doctype html><title>Frozen UI</title>', encoding='utf-8')
    (web / 'package.json').write_text(json.dumps({'name': 'native-web-fixture', 'version': '0.1.0', 'private': True,
                                               'scripts': {'build': 'node build.mjs'}}), encoding='utf-8')
    (web / 'package-lock.json').write_text(json.dumps({'name': 'native-web-fixture', 'version': '0.1.0', 'lockfileVersion': 3,
                                                    'packages': {'': {'name': 'native-web-fixture', 'version': '0.1.0'}}}), encoding='utf-8')
    (web / 'build.mjs').write_text("import{mkdirSync,copyFileSync}from'node:fs';mkdirSync('dist',{recursive:true});copyFileSync('index.html','dist/index.html');", encoding='utf-8')
    return profile


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

    def test_web_source_selection_and_prepare_do_not_install_or_build(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / 'repo'
            profile = web_fixture(root)
            source, plan = prepare(root, profile, base / 'cache')
            self.assertEqual(plan['packages'], ['@example/ui', '@example/app'])
            self.assertEqual(plan['web_application'], '@example/ui')
            self.assertIn('packages/example/ui/index.html', plan['inputs'])
            self.assertFalse(list(source.rglob('node_modules')))
            self.assertFalse(list(source.rglob('dist')))
            verify_sources(source, plan)

    @unittest.skipUnless(shutil.which('node') and shutil.which('npm'), 'Node/npm are build-time requirements')
    def test_web_build_uses_frozen_copy_and_receipts_offline_runtime_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / 'repo'
            profile = web_fixture(root)
            source, plan = prepare(root, profile, base / 'cache')
            (root / 'packages/example/ui/index.html').write_text('MUTATED CHECKOUT', encoding='utf-8')
            receipt = build_web(source, plan, base / 'release/client-ui')
            self.assertEqual((base / 'release/client-ui/index.html').read_text(), '<!doctype html><title>Frozen UI</title>')
            self.assertIn('index.html', receipt['files'])
            self.assertEqual(receipt['application'], '@example/ui')
            self.assertFalse(list(source.rglob('node_modules')))
            self.assertFalse(list(source.rglob('dist')))
            verify_sources(source, plan)

    def test_web_version_edges_are_exact_and_unknown_owners_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            web_fixture(root)
            manifest = root / 'packages/example/app/latticeaxiom-package.toml'
            original = manifest.read_text(encoding='utf-8')
            manifest.write_text(original.replace('"=0.1.0"', '"^0.1.0"'), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'must pin its source version'):
                source_package_graph(root)
            manifest.write_text(original.replace('@example/ui', '@missing/ui'), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'unknown Web source owner'):
                source_package_graph(root)

    def test_publishing_refuses_unrelated_or_changed_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, destination = root / 'dist', root / 'client-ui'
            source.mkdir()
            (source / 'index.html').write_text('build', encoding='utf-8')
            publish_web_assets(source, destination)
            (destination / 'index.html').write_text('user edit', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'modified outside'):
                publish_web_assets(source, destination)


if __name__ == '__main__':
    unittest.main()
