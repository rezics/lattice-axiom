"""Package-level cycles matter even when the underlying crate graph is acyclic."""
import unittest
from package_graph import dependency_order, package_graph


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


if __name__ == '__main__':
    unittest.main()
