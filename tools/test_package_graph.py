"""Package-level cycles matter even when the underlying crate graph is acyclic."""
import unittest
import shutil
import tempfile
from pathlib import Path
from package_graph import ROOT, dependency_order, package_graph, package_manifests, read_toml


class PackageGraphTests(unittest.TestCase):
    def test_a_cycle_created_by_ownership_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'build cycle'):
            dependency_order({'@example/a': {'@example/b'}, '@example/b': {'@example/a'}})

    def test_dependencies_precede_consumers(self):
        order = dependency_order({'app': {'world'}, 'world': {'kernel'}, 'kernel': set()})
        self.assertLess(order.index('kernel'), order.index('world'))
        self.assertLess(order.index('world'), order.index('app'))

    def test_actual_workspace_has_no_package_cycle(self):
        graph = package_graph()
        self.assertEqual(set(dependency_order(graph)), set(graph))

    def test_generic_host_cannot_pull_in_a_terrenia_implementation(self):
        graph = package_graph()
        pending, closure = ['@latticeaxiom/host'], set()
        while pending:
            package = pending.pop()
            if package not in closure:
                closure.add(package)
                pending.extend(graph[package])
        self.assertFalse(any(name.startswith('@terrenia/') for name in closure))

    def test_cargo_cannot_introduce_an_undeclared_package_dependency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [ROOT / 'Cargo.toml']
            paths.extend(ROOT / member / 'Cargo.toml'
                         for member in read_toml(ROOT / 'Cargo.toml')['workspace']['members'])
            paths.extend(path for path, _ in package_manifests().values())
            for source in paths:
                target = root / source.relative_to(ROOT)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, target)
            manifest = root / 'packages/latticeaxiom/host/latticeaxiom-package.toml'
            text = manifest.read_text(encoding='utf-8').split('[rust.dependencies]')[0]
            manifest.write_text(text, encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'manifest build edges disagree with Cargo'):
                package_graph(root)


if __name__ == '__main__':
    unittest.main()
