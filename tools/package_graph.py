"""Project the actual Rust build edges onto Lattice package ownership."""
from __future__ import annotations

import argparse
from graphlib import CycleError, TopologicalSorter
import json
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def read_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding='utf-8'))


def package_graph(root: Path = ROOT) -> dict[str, set[str]]:
    workspace = read_toml(root / 'Cargo.toml')['workspace']
    crates = {}
    for member in workspace['members']:
        manifest = root / member / 'Cargo.toml'
        cargo = read_toml(manifest)
        owner = read_toml(manifest.parents[2] / 'latticeaxiom-package.toml')['name']
        name = cargo['package']['name']
        if name in crates:
            raise ValueError(f'duplicate owned Cargo package: {name}')
        crates[name] = (owner, cargo)
    graph = {owner: set() for owner, _ in crates.values()}
    shared = workspace.get('dependencies', {})
    for owner, cargo in crates.values():
        scopes = [cargo, *cargo.get('target', {}).values()]
        for scope in scopes:
            for section in ('dependencies', 'build-dependencies'):
                for alias, dependency in scope.get(section, {}).items():
                    if isinstance(dependency, dict) and dependency.get('workspace'):
                        dependency = shared[alias]
                    name = dependency.get('package', alias) if isinstance(dependency, dict) else alias
                    if name in crates and crates[name][0] != owner:
                        graph[owner].add(crates[name][0])
    return graph


def dependency_order(graph: dict[str, set[str]]) -> list[str]:
    try:
        return list(TopologicalSorter({key: sorted(value) for key, value in sorted(graph.items())}).static_order())
    except CycleError as error:
        raise ValueError('package ownership creates a build cycle: ' + ' -> '.join(error.args[1])) from error


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--json', action='store_true')
    args = parser.parse_args()
    graph = package_graph()
    order = dependency_order(graph)
    if args.json:
        print(json.dumps({'packages': {key: sorted(value) for key, value in sorted(graph.items())}, 'build_order': order}, indent=2))
    else:
        print(f'validated {len(graph)} package owners and {sum(map(len, graph.values()))} acyclic build edges')


if __name__ == '__main__':
    main()
