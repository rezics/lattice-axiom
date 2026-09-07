"""Validate manifest-owned build edges against the internal Cargo implementation."""
from __future__ import annotations

import argparse
from graphlib import CycleError, TopologicalSorter
import json
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def read_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding='utf-8'))


def package_manifests(root: Path = ROOT) -> dict[str, tuple[Path, dict]]:
    packages = {}
    paths = set()
    for member in read_toml(root / 'Cargo.toml')['workspace']['members']:
        parts = Path(member).parts
        if len(parts) != 5 or parts[0] != 'packages' or parts[3] != 'crates' or '..' in parts:
            raise ValueError(f'non-local package member: {member}')
        paths.add(root / Path(member).parents[1] / 'latticeaxiom-package.toml')
    for path in sorted(paths):
        manifest = read_toml(path)
        if 'rust' not in manifest:
            continue
        name = manifest['name']
        if name in packages:
            raise ValueError(f'duplicate Lattice implementation package: {name}')
        packages[name] = (path, manifest)
    return packages


def cargo_package_graph(root: Path = ROOT) -> dict[str, set[str]]:
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


def package_graph(root: Path = ROOT) -> dict[str, set[str]]:
    packages = package_manifests(root)
    graph = {}
    for name, (_, manifest) in packages.items():
        dependencies = manifest['rust'].get('dependencies', {})
        graph[name] = set(dependencies)
        for target, version in dependencies.items():
            if target not in packages:
                raise ValueError(f'{name}: unknown Rust source owner {target}')
            if version != '=' + packages[target][1]['version']:
                raise ValueError(f'{name}: build dependency {target} must pin its source version')
    dependency_order(graph)
    observed = cargo_package_graph(root)
    if set(graph) != set(observed):
        raise ValueError('native package manifests and Cargo owners disagree')
    for name, edges in graph.items():
        if edges != observed[name]:
            raise ValueError(f'{name}: manifest build edges disagree with Cargo; '
                             f'undeclared={sorted(observed[name] - edges)}, '
                             f'unused={sorted(edges - observed[name])}')
    return graph


def source_package_manifests(root: Path = ROOT) -> dict[str, tuple[Path, dict]]:
    """Discover declared web source owners without inventing Cargo packages."""
    packages = package_manifests(root)
    for path in sorted((root / 'packages').glob('*/*/latticeaxiom-package.toml')):
        manifest = read_toml(path)
        if 'web' not in manifest:
            continue
        name = manifest['name']
        if name in packages and packages[name][0] != path:
            raise ValueError(f'duplicate Lattice source package: {name}')
        packages[name] = (path, manifest)
    return packages


def source_package_graph(root: Path = ROOT) -> dict[str, set[str]]:
    """Validate Cargo ownership first, then merge pinned web source edges."""
    graph = package_graph(root)
    packages = source_package_manifests(root)
    for name, (_, manifest) in packages.items():
        edges = graph.setdefault(name, set())
        web = manifest.get('web', {})
        if 'rust' not in manifest and not web.get('entry'):
            raise ValueError(f'{name}: web source owner requires an entry')
        for target, version in web.get('dependencies', {}).items():
            if target not in packages or not packages[target][1].get('web', {}).get('entry'):
                raise ValueError(f'{name}: unknown Web source owner {target}')
            if version != '=' + packages[target][1]['version']:
                raise ValueError(f'{name}: Web dependency {target} must pin its source version')
            edges.add(target)
    dependency_order(graph)
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
    graph = source_package_graph()
    order = dependency_order(graph)
    if args.json:
        print(json.dumps({'packages': {key: sorted(value) for key, value in sorted(graph.items())}, 'build_order': order}, indent=2))
    else:
        print(f'validated {len(graph)} package owners and {sum(map(len, graph.values()))} acyclic build edges')


if __name__ == '__main__':
    main()
