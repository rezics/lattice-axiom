"""Freeze manifest-selected native packages into an independent Cargo product workspace.

This is build tooling, not an ECS/plugin runtime or an OS sandbox. Admitted Cargo
build scripts retain producer authority. Runtime gameplay/resource locks are
verified separately by the selected application.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from package_graph import ROOT, dependency_order, package_graph, package_manifests, read_toml
from repository_check import text_error

SCHEMA = 'latticeaxiom.native-product.v1'
RESERVED = {'.git', 'target', '.standalone', '__pycache__'}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')


def regular_files(path: Path):
    if path.is_symlink() or getattr(path, 'is_junction', lambda: False)():
        raise ValueError(f'linked source is not admitted: {path}')
    if path.is_dir():
        for child in sorted(path.iterdir()):
            if child.name not in RESERVED:
                yield from regular_files(child)
    elif path.is_file():
        yield path
    else:
        raise ValueError(f'source is absent or not a regular file: {path}')


def source_closure(graph: dict[str, set[str]], root: str) -> list[str]:
    if root not in graph:
        raise ValueError(f'unknown native product root {root}')
    selected, pending = set(), [root]
    while pending:
        name = pending.pop()
        if name not in selected:
            selected.add(name)
            pending.extend(graph[name])
    return [name for name in dependency_order(graph) if name in selected]


def confined(base: Path, relative: str) -> Path:
    path = Path(relative)
    if path.is_absolute() or '..' in path.parts or not relative:
        raise ValueError(f'non-local source path: {relative}')
    result = (base / path).resolve(strict=True)
    if not result.is_relative_to(base.resolve()):
        raise ValueError(f'source escapes its owner: {relative}')
    return base / path


def workspace_manifest(original: str, members: list[str], entry: str) -> bytes:
    section = '[workspace]\nresolver = "3"\nmembers = ' + json.dumps(members) + '\n'
    section += 'default-members = ' + json.dumps([entry]) + '\n\n'
    rewritten, count = re.subn(r'(?ms)^\[workspace\].*?(?=^\[|\Z)', lambda _: section, original, count=1)
    if count != 1:
        raise ValueError('native product requires a virtual Cargo workspace')
    return rewritten.encode('utf-8')


def prepare(repository: Path, profile_path: Path, cache: Path) -> tuple[Path, dict]:
    repository = repository.resolve(strict=True)
    profile_bytes = profile_path.read_bytes()
    profile = read_toml(profile_path)
    required = {'schema_version', 'name', 'root', 'features', 'launch', 'build_profile'}
    if set(profile) != required or profile['schema_version'] != 1:
        raise ValueError('unsupported native product profile')
    if not re.fullmatch(r'[a-z0-9][a-z0-9-]*', profile['name']):
        raise ValueError('invalid product name')
    if profile['build_profile'] not in ('dev', 'release'):
        raise ValueError('unsupported Cargo build profile')
    packages = package_manifests(repository)
    selected = source_closure(package_graph(repository), profile['root'])
    root_manifest_path, root_manifest = packages[profile['root']]
    entry = confined(root_manifest_path.parent, root_manifest['rust']['entry'])
    cargo_entry = read_toml(entry)
    binaries = sorted(root_manifest['rust'].get('binaries', []))
    declared_targets = {target['name'] for target in cargo_entry.get('bin', [])}
    if not binaries or not set(binaries) <= declared_targets or profile['launch'] not in binaries:
        raise ValueError('the product must select manifest-exposed Cargo binaries')
    if not isinstance(profile['features'], list) or not set(profile['features']) <= set(cargo_entry.get('features', {})):
        raise ValueError('the product selected an unknown implementation feature')
    cargo = read_toml(repository / 'Cargo.toml')
    files: dict[str, bytes] = {}

    def capture(path: Path) -> None:
        for source in regular_files(path):
            relative = source.relative_to(repository).as_posix()
            data = source.read_bytes()
            if error := text_error(relative, data):
                raise ValueError(error)
            files[relative] = data

    members = []
    for name in selected:
        path, manifest = packages[name]
        native = manifest.get('realizations', {}).get('native-static', {})
        if (manifest.get('trust') != 'trusted-native' or native.get('kind') != 'native-static'
                or native.get('artifact', {}).get('kind') != 'source-build'):
            raise ValueError(f'{name}: native compilation must be explicitly declared and trusted')
        for include in manifest['source_inclusion']['include']:
            capture(confined(path.parent, include))
        for member in sorted(manifest['rust']['members']):
            cargo_manifest = confined(path.parent, member).relative_to(repository)
            if cargo_manifest.as_posix() not in files:
                raise ValueError(f'{name}: source inclusion omits {member}')
            members.append(cargo_manifest.parent.as_posix())
    for registry in cargo.get('patch', {}).values():
        for patch in registry.values():
            if 'path' in patch:
                vendor = confined(repository, patch['path'])
                if not vendor.relative_to(repository).as_posix().startswith('third_party/'):
                    raise ValueError('local upstream patches must be declared under third_party/')
                capture(vendor)
    for relative in ('rust-toolchain.toml', '.cargo/config.toml'):
        if (repository / relative).is_file():
            capture(repository / relative)
    seed = (repository / 'Cargo.lock').read_bytes()
    files['Cargo.lock.seed'] = seed
    files['native-profile.toml'] = profile_bytes
    files['Cargo.toml'] = workspace_manifest(
        (repository / 'Cargo.toml').read_text(encoding='utf-8'), sorted(members),
        entry.parent.relative_to(repository).as_posix())
    plan = {'schema': SCHEMA, 'profile': profile, 'packages': selected,
            'cargo_package': cargo_entry['package']['name'], 'binaries': binaries,
            'inputs': {name: digest(data) for name, data in sorted(files.items())}}
    plan_hash = digest(canonical(plan))
    # Keep paths below Windows' legacy path limit. The complete digest and plan
    # are still verified on reuse, so a shortened-directory collision is an error.
    output = cache.resolve() / plan_hash[:16]
    source_root = output / 'source'
    if output.exists():
        if (output / 'native-product.json').read_bytes() != canonical(plan):
            raise ValueError('cached native plan was modified')
        verify_sources(source_root, plan)
        return source_root, plan
    source_root.mkdir(parents=True, exist_ok=False)
    for relative, data in files.items():
        destination = source_root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    (source_root / 'Cargo.lock').write_bytes(seed)
    (output / 'native-product.json').write_bytes(canonical(plan))
    return source_root, plan


def verify_sources(source_root: Path, plan: dict) -> None:
    actual = {file.relative_to(source_root).as_posix(): digest(file.read_bytes())
              for file in regular_files(source_root) if file.relative_to(source_root).as_posix() != 'Cargo.lock'}
    if actual != plan['inputs']:
        raise ValueError('frozen native product source inputs were modified')


def registry_pins(lock: dict) -> set[tuple]:
    return {(row['name'], row['version'], row['source'], row.get('checksum'))
            for row in lock['package'] if row.get('source')}


def materialize_for_build(snapshot: Path, plan: dict) -> Path:
    """Keep Cargo source paths stable while the immutable snapshot remains the input."""
    verify_sources(snapshot, plan)
    cache = snapshot.parent.parent.resolve()
    directory = cache / ('work-' + digest(plan['profile']['name'].encode())[:8])
    source = directory / 'source'
    marker = directory / 'build-plan.json'
    if directory.exists():
        previous = json.loads(marker.read_bytes())
        if previous['schema'] != SCHEMA or previous['profile']['name'] != plan['profile']['name']:
            raise ValueError('build workspace is not owned by this product')
        verify_sources(source, previous)
    else:
        source.mkdir(parents=True, exist_ok=False)
    desired = set(plan['inputs']) | {'Cargo.lock'}
    for file in list(regular_files(source)):
        if file.relative_to(source).as_posix() not in desired:
            # Remove only obsolete, previously verified generated files.
            if not file.resolve().is_relative_to(source.resolve()):
                raise ValueError('obsolete build input escaped its workspace')
            file.unlink()
    for relative in sorted(desired):
        destination = source / relative
        data = (snapshot / relative).read_bytes()
        if not destination.is_file() or destination.read_bytes() != data:
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
    marker.write_bytes(canonical(plan))
    verify_sources(source, plan)
    return source


def resolve_cargo(source_root: Path, plan: dict) -> dict:
    verify_sources(source_root, plan)
    output = subprocess.check_output(['cargo', 'metadata', '--offline', '--format-version', '1',
                                     '--manifest-path', str(source_root / 'Cargo.toml')], cwd=source_root)
    metadata = json.loads(output)
    for package in metadata['packages']:
        if package['source'] is None and not Path(package['manifest_path']).resolve().is_relative_to(source_root.resolve()):
            raise ValueError(f"Cargo source escapes frozen product: {package['name']}")
    if not registry_pins(read_toml(source_root / 'Cargo.lock')) <= registry_pins(read_toml(source_root / 'Cargo.lock.seed')):
        raise ValueError('product resolution changed the pinned upstream dependency set')
    verify_sources(source_root, plan)
    return metadata


def cargo_arguments(source_root: Path, plan: dict, target: Path, action: str) -> list[str]:
    profile = plan['profile']
    arguments = ['cargo', action, '--frozen', '--offline', '--manifest-path', str(source_root / 'Cargo.toml'),
                 '--target-dir', str(target), '--package', plan['cargo_package'], '--no-default-features']
    if profile['features']:
        arguments += ['--features', ','.join(profile['features'])]
    if profile['build_profile'] == 'release':
        arguments.append('--release')
    for binary in ([profile['launch']] if action == 'run' else plan['binaries']):
        arguments += ['--bin', binary]
    return arguments


def build(source_root: Path, plan: dict, target: Path) -> dict:
    snapshot = source_root
    source_root = materialize_for_build(snapshot, plan)
    resolve_cargo(source_root, plan)
    lock_hash = digest((source_root / 'Cargo.lock').read_bytes())
    command = cargo_arguments(source_root, plan, target, 'build')
    cache_files, build_outputs = set(), set()
    process = subprocess.Popen(command + ['--message-format=json-render-diagnostics'],
                               cwd=source_root, stdout=subprocess.PIPE, text=True, encoding='utf-8')
    assert process.stdout is not None
    for line in process.stdout:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            print(line, file=sys.stderr, end='')
            continue
        if not isinstance(event, dict):
            print(line, file=sys.stderr, end='')
            continue
        if event.get('reason') == 'compiler-artifact':
            cache_files.update(event['filenames'])
        elif event.get('reason') == 'build-script-executed' and event.get('out_dir'):
            build_outputs.add(event['out_dir'])
        elif event.get('reason') == 'compiler-message':
            print(event['message'].get('rendered') or '', file=sys.stderr, end='')
    process.stdout.close()
    if process.wait():
        raise subprocess.CalledProcessError(process.returncode, command)
    verify_sources(source_root, plan)
    verify_sources(snapshot, plan)
    if digest((source_root / 'Cargo.lock').read_bytes()) != lock_hash:
        raise ValueError('Cargo changed the frozen product lock during build')
    profile = 'release' if plan['profile']['build_profile'] == 'release' else 'debug'
    artifacts = {}
    for name in plan['binaries']:
        file = target / profile / (name + ('.exe' if os.name == 'nt' else ''))
        artifacts[name] = {'path': str(file), 'sha256': digest(file.read_bytes())}
    receipt = {'schema': SCHEMA, 'plan_sha256': digest(canonical(plan)), 'cargo_lock_sha256': lock_hash,
               'snapshot': str(snapshot), 'materialized_source': str(source_root),
               'rustc': subprocess.check_output(['rustc', '-vV'], text=True),
               'cargo': subprocess.check_output(['cargo', '-Vv'], text=True),
               'build_environment_sha256': digest(canonical(dict(sorted(os.environ.items())))),
               'authority': 'trusted build scripts; declared source containment; no OS sandbox',
               'cache_files': sorted(cache_files), 'build_outputs': sorted(build_outputs),
               'artifacts': artifacts}
    (snapshot.parent / 'build-receipt.json').write_bytes(canonical(receipt))
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', type=Path, default=ROOT / 'products/terrenia-dev.toml')
    parser.add_argument('--cache', type=Path, default=ROOT / '.temp/native')
    parser.add_argument('--target-dir', type=Path, default=ROOT / 'target/native-products')
    parser.add_argument('--prepare-only', action='store_true')
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--runtime', type=Path, default=ROOT)
    args = parser.parse_args()
    source, plan = prepare(ROOT, args.profile.resolve(), args.cache)
    print(f'Frozen native product: {source}', flush=True)
    if args.prepare_only:
        resolve_cargo(source, plan)
    else:
        receipt = build(source, plan, args.target_dir.resolve())
    if args.run:
        if args.prepare_only:
            parser.error('--run cannot be combined with --prepare-only')
        subprocess.run(cargo_arguments(Path(receipt['materialized_source']), plan, args.target_dir.resolve(), 'run'),
                       cwd=args.runtime.resolve(strict=True), check=True)


if __name__ == '__main__':
    main()
