#!/usr/bin/env python3
"""Measure/capture the real client using a disposable copy of a saved world."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--deps', type=Path, required=True)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--world', required=True)
    parser.add_argument('--lock', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=60)
    parser.add_argument('--timings-only', action='store_true', help='Use the existing lifecycle driver; compatible with the baseline client.')
    parser.add_argument('--overview', action='store_true', help='Fixed elevated camera over the streamed player region.')
    parser.add_argument('--held-item', help='Seed one catalog item into the disposable capture world.')
    parser.add_argument('--render-distance', type=int, choices=range(2, 33))
    args = parser.parse_args()
    if not 15 <= args.seconds <= 60:
        parser.error('seconds must be 15..60')
    runtime = args.runtime.resolve(strict=True)
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    world_file = runtime / 'run/worlds/worlds.redb'
    before = digest(world_file)
    for name in ('catalog', 'run'):
        shutil.copytree(runtime / name, output / name)
    shutil.copy2(runtime / 'catalog/locks' / f'{args.lock}.lock', output / 'latticeaxiom.lock')
    if args.render_distance is not None:
        settings_file = output / 'run/user/local-settings.v1.json'
        settings_file.parent.mkdir(parents=True, exist_ok=True)
        settings = json.loads(settings_file.read_text()) if settings_file.exists() else {
            'schema': 'latticeaxiom:schema/local-settings@1', 'schema_version': 1,
            'store_revision': 0, 'transaction_revision': 0, 'writer': 'local-user',
            'device': {}, 'user': {}, 'binding_profile': {'schema_version': 1, 'overrides': {}},
            'pending_restart': None}
        settings['user']['latticeaxiom:setting/view-distance'] = {'schema_version': 1, 'value': args.render_distance}
        settings['store_revision'] += 1
        settings['transaction_revision'] += 1
        settings_file.write_text(json.dumps(settings), encoding='utf-8')
    (output / '.latticeaxiom-qa').write_text('Disposable render acceptance world.\n', encoding='utf-8')
    environment = {k: v for k, v in os.environ.items() if not k.startswith('LATTICEAXIOM_')}
    environment.update(RUST_LOG='info,wgpu=warn,naga=warn', LATTICEAXIOM_WORLD_ID=args.world,
                       LATTICEAXIOM_CHILD_ROLE='world')
    if args.timings_only:
        environment.update(LATTICEAXIOM_LIFECYCLE_QA='1', LATTICEAXIOM_LIFECYCLE_HOLD_MS=str(args.seconds * 1000))
    else:
        environment.update(LATTICEAXIOM_CAPTURE_PATH=str(output / 'scene.png'),
                           LATTICEAXIOM_CAPTURE_SECONDS=str(args.seconds), LATTICEAXIOM_CAPTURE_SIZE='1280x720')
        if args.overview:
            environment['LATTICEAXIOM_CAPTURE_OVERVIEW'] = '1'
        if args.held_item:
            environment['LATTICEAXIOM_CAPTURE_ITEM'] = args.held_item
    rust_lib = subprocess.check_output(['rustc', '--print', 'target-libdir'], text=True).strip()
    environment['PATH'] = os.pathsep.join((str(args.deps.resolve(strict=True)), rust_lib, environment['PATH']))
    started = time.monotonic()
    with (output / 'client.log').open('w', encoding='utf-8') as log:
        process = subprocess.Popen([str(binary)], cwd=output, env=environment, stdout=log, stderr=subprocess.STDOUT)
        try:
            code = process.wait(timeout=args.seconds + 180)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=15)
            raise
    report = {'binary': str(binary), 'binary_sha256': digest(binary), 'world': args.world,
              'lock': args.lock, 'seconds': args.seconds, 'overview': args.overview,
              'held_item': args.held_item,
              'render_distance': args.render_distance, 'exit_code': code,
              'wall_seconds': time.monotonic() - started, 'original_world_unchanged': digest(world_file) == before}
    (output / 'run.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    if code or not report['original_world_unchanged']:
        raise RuntimeError(f'Render run failed: {report}')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
