"""Check package ownership and the text-only version-control boundary."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tomllib
from package_graph import dependency_order, source_package_graph

ROOT = Path(__file__).resolve().parents[1]
MEDIA = frozenset('.png .jpg .jpeg .webp .gif .bmp .ico .ktx .ktx2 .dds .exr .hdr .blend .glb .fbx .aseprite .psd .wav .ogg .mp3 .flac .ttf .otf .woff .woff2 .zip .7z .exe .dll .pdb'.split())


def text_error(path: str, data: bytes) -> str | None:
    if Path(path).suffix.lower() in MEDIA:
        return f'{path}: binary asset/artifact extension is not allowed'
    if b'\0' in data:
        return f'{path}: binary NUL bytes are not allowed'
    try:
        data.decode('utf-8-sig')
    except UnicodeDecodeError:
        return f'{path}: committed content must be UTF-8 text'
    return None


def check_layout(root: Path) -> list[str]:
    errors = []
    config = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
    for member in config['workspace']['members']:
        path = Path(member)
        parts = path.parts
        if len(parts) != 5 or parts[0] != 'packages' or parts[3] != 'crates':
            errors.append(f'{member}: owned crates must be package-local')
            continue
        manifest = root / path.parents[1] / 'latticeaxiom-package.toml'
        if not manifest.is_file():
            errors.append(f'{member}: owner manifest missing')
            continue
        package = tomllib.loads(manifest.read_text(encoding='utf-8'))
        owner = package['name']
        rust = package.get('rust', {})
        expected = str((root / member / 'Cargo.toml').relative_to(manifest.parent)).replace('\\', '/')
        if expected not in rust.get('members', []) or rust.get('entry') not in rust.get('members', []):
            errors.append(f'{member}: package Rust entry/membership is incomplete')
        cargo = tomllib.loads((root / member / 'Cargo.toml').read_text(encoding='utf-8'))
        if cargo['package'].get('metadata', {}).get('latticeaxiom', {}).get('owner') != owner:
            errors.append(f'{member}: Cargo owner does not match Lattice package identity')
        if (manifest.parent / 'Cargo.toml').exists():
            errors.append(f'{manifest.parent}: package root must not be a Cargo package/workspace')
    try:
        dependency_order(source_package_graph(root))
    except ValueError as error:
        errors.append(str(error))
    return errors


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged', action='store_true')
    args = parser.parse_args()
    command = ['git', 'diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z'] if args.staged else ['git', 'ls-files', '-z']
    paths = subprocess.check_output(command, cwd=ROOT).decode().split('\0')
    errors = check_layout(ROOT)
    checked = 0
    batch = subprocess.Popen(['git', 'cat-file', '--batch'], cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE) if args.staged else None
    try:
        for path in filter(None, paths):
            file = ROOT / path
            if not args.staged and not file.is_file():
                continue
            if batch:
                assert batch.stdin is not None and batch.stdout is not None
                batch.stdin.write((':' + path + '\n').encode())
                batch.stdin.flush()
                header = batch.stdout.readline().split()
                if len(header) != 3 or header[1] != b'blob':
                    raise SystemExit(f'{path}: cannot read staged blob')
                data = batch.stdout.read(int(header[2]))
                if batch.stdout.read(1) != b'\n':
                    raise SystemExit('invalid git cat-file framing')
            else:
                data = file.read_bytes()
            error = text_error(path, data)
            if error:
                errors.append(error)
            checked += 1
    finally:
        if batch:
            assert batch.stdin is not None
            batch.stdin.close()
            batch.wait()
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'validated package ownership and {checked} text files')


if __name__ == '__main__':
    main()
