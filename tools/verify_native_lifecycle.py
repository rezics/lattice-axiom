"""Verify two isolated domain lifecycle runs; this does not prove Web UI rendering."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import uuid


def verify_domain_evidence(evidence: dict, durable: dict) -> None:
    if (evidence.get('schema') != 'latticeaxiom.lifecycle-domain-evidence.v1'
            or evidence.get('scope') != 'domain-commands-and-state'):
        raise RuntimeError('missing domain lifecycle evidence schema')
    events = evidence.get('events', [])
    if any(event.get('event') == 'failed' for event in events):
        raise RuntimeError('domain lifecycle reported a failure')
    entered = next((i for i, event in enumerate(events) if event.get('event') == 'world-entered'), None)
    saving = next((i for i, event in enumerate(events) if event.get('event') == 'command-accepted'
                   and event.get('data', {}).get('method') == 'game.exit'), None)
    returned = next((i for i, event in enumerate(events) if event.get('event') == 'returned-to-shell'), None)
    completed = next((i for i, event in enumerate(events) if event.get('event') == 'completed'
                      and event.get('data', {}).get('command') == 'app.quit'), None)
    if any(index is None for index in (entered, saving, returned, completed)):
        raise RuntimeError('domain lifecycle lacks world -> save-return -> shell -> quit evidence')
    if not entered < saving < returned < completed:
        raise RuntimeError('domain lifecycle transitions occurred out of order')
    if events[entered]['data'].get('world') != durable['world_id']:
        raise RuntimeError('domain world differs from the physically durable receipt')
    if events[entered]['data'].get('mode') != 'game' or events[returned]['data'].get('mode') != 'shell':
        raise RuntimeError('domain lifecycle did not enter the game and return to the shell')


def verify_durable_probe(probe: dict, nonce: str, state: dict) -> dict:
    keys = {'schema', 'world_id', 'revision', 'written_revision', 'world_lock_hash',
            'return_to_shell', 'nonce', 'checksum'}
    if set(probe) != keys or probe['schema'] != 'latticeaxiom.in-process-durable-world.v1':
        raise RuntimeError('missing in-process durable world probe schema')
    payload = {key: value for key, value in probe.items() if key != 'checksum'}
    checksum = hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(',', ':'),
                                         ensure_ascii=False).encode('utf-8')).hexdigest()
    if probe['checksum'] != checksum:
        raise RuntimeError('durable world probe checksum does not match')
    if not nonce or probe['nonce'] != nonce:
        raise RuntimeError('durable world probe is not from this run')
    if (type(probe['revision']) is not int or probe['revision'] <= 0
            or type(probe['written_revision']) is not int
            or probe['written_revision'] != probe['revision']):
        raise RuntimeError('probe lacks matching positive physically durable and written revisions')
    if probe['return_to_shell'] is not True:
        raise RuntimeError('durable probe does not prove save and return to shell')
    if state.get('world') != probe['world_id'] or state.get('lock') != probe['world_lock_hash']:
        raise RuntimeError('durable probe differs from the observed world identity or lock')
    return {'world_id': probe['world_id'], 'revision': probe['revision']}


def verify_report(report: dict, previous: dict | None, domain: dict | None = None,
                  probe: dict | None = None, nonce: str | None = None,
                  state: dict | None = None) -> dict:
    outcome = report['outcome']
    if outcome['outcome'] != 'product-exited' or outcome.get('exit_kind') != 'shell-quit' or report['failures']:
        raise RuntimeError(f"native lifecycle failed: {outcome}; {report['failures']}")
    roles = [hop['role']['kind'] for hop in report['hops']]
    if roles not in (['shell'], ['shell', 'world', 'shell']):
        raise RuntimeError('expected one same-window shell process or the legacy shell/world/shell process sequence')
    if roles == ['shell']:
        if domain is None:
            raise RuntimeError('a same-window process requires domain transition evidence')
        if report.get('last_durable_world') or report.get('last_written_world'):
            raise RuntimeError('a shell report must not claim a world-process durability receipt')
        if probe is None or nonce is None or state is None:
            raise RuntimeError('same-window exit lacks a physically durable publication probe')
        durable = verify_durable_probe(probe, nonce, state)
    else:
        durable = report['last_durable_world']
        if not durable or durable['revision'] <= 0 or durable != report['last_written_world']:
            raise RuntimeError('exit lacks a matching physically durable world receipt')
    if previous and durable['world_id'] != previous['world_id']:
        raise RuntimeError('second run did not Continue the same saved world')
    if previous and durable['revision'] < previous['revision']:
        raise RuntimeError('second run regressed the durable revision')
    if domain is not None:
        verify_domain_evidence(domain, durable)
    return durable


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True, help='built development latticeaxiom-play executable')
    parser.add_argument('--runtime', type=Path, required=True, help='prepared locks and catalog/CAS')
    parser.add_argument('--output', type=Path, required=True, help='new, non-existing QA output directory')
    parser.add_argument('--replacement-lock', type=Path, help='replace the default after the first session; its original archive/CAS must be in runtime')
    parser.add_argument('--expected-mode', choices=('survival', 'creative'))
    parser.add_argument('--world-id', type=uuid.UUID, help='fixed QA-only identity for reproducible terrain seeds')
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    runtime = args.runtime.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / '.latticeaxiom-qa').write_text('Native lifecycle acceptance only.\n', encoding='utf-8')
    shutil.copytree(runtime / 'catalog', output / 'catalog')
    for relative in ('latticeaxiom.lock', 'run/shell/latticeaxiom.lock', 'run/client-resources/latticeaxiom.lock'):
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(runtime / relative, target)
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith('LATTICEAXIOM_CAPTURE_')}
    environment['LATTICEAXIOM_LIFECYCLE_QA'] = '1'
    # Remain active past the launcher's 30-second observation window.
    environment['LATTICEAXIOM_LIFECYCLE_HOLD_MS'] = '35000'
    if args.world_id:
        environment['LATTICEAXIOM_QA_WORLD_ID'] = str(args.world_id)
    if os.name == 'nt':
        rust_lib = subprocess.check_output(['rustc', '--print', 'target-libdir'], text=True).strip()
        environment['PATH'] = os.pathsep.join((rust_lib, str(binary.parent / 'deps'), environment['PATH']))
    previous = None
    previous_state = None
    for number in (1, 2):
        nonce = str(uuid.uuid4())
        environment['LATTICEAXIOM_LIFECYCLE_NONCE'] = nonce
        if number == 2 and args.replacement_lock:
            shutil.copy2(args.replacement_lock.resolve(strict=True), output / 'latticeaxiom.lock')
        # A successful second run must produce fresh evidence, never reuse run one.
        for name in ('lifecycle-report.json', 'lifecycle-world-state.json', 'lifecycle-domain-evidence.json', 'lifecycle-durable-world.json'):
            (output / name).unlink(missing_ok=True)
        with (output / f'lifecycle-{number}.log').open('w', encoding='utf-8') as log:
            process = subprocess.Popen([str(binary)], cwd=output, env=environment, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=os.name != 'nt')
            try:
                code = process.wait(timeout=300)
            except subprocess.TimeoutExpired:
                # Stop only the process tree spawned for this isolated QA run.
                if os.name == 'nt':
                    subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], check=False)
                else:
                    os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=15)
                raise
            if code:
                raise RuntimeError(f'native session {number} exited {code}; inspect its log in {output}')
        report_path = output / 'lifecycle-report.json'
        report = json.loads(report_path.read_text(encoding='utf-8'))
        shutil.copy2(report_path, output / f'lifecycle-{number}.json')
        domain_path = output / 'lifecycle-domain-evidence.json'
        domain = json.loads(domain_path.read_text(encoding='utf-8'))
        shutil.copy2(domain_path, output / f'lifecycle-domain-evidence-{number}.json')
        state_path = output / 'lifecycle-world-state.json'
        state = json.loads(state_path.read_text(encoding='utf-8'))
        probe_path = output / 'lifecycle-durable-world.json'
        probe = json.loads(probe_path.read_text(encoding='utf-8')) if probe_path.is_file() else None
        if probe is not None:
            shutil.copy2(probe_path, output / f'lifecycle-durable-world-{number}.json')
        previous = verify_report(report, previous, domain, probe, nonce, state)
        if state['world'] != previous['world_id']:
            raise RuntimeError('captured world identity differs from the durable exit receipt')
        if args.world_id and state['world'] != str(args.world_id):
            raise RuntimeError('client did not use the requested deterministic QA world identity')
        shutil.copy2(state_path, output / f'lifecycle-world-state-{number}.json')
        if args.expected_mode and state['mode'] != args.expected_mode:
            raise RuntimeError(f'world mode differs from expected {args.expected_mode}: {state}')
        if previous_state and (state['world'], state['lock'], state['mode']) != (previous_state['world'], previous_state['lock'], previous_state['mode']):
            raise RuntimeError('Continue changed the original world identity, lock or rules')
        if number == 1 and args.expected_mode == 'survival' and state['inventory_items'] != 0:
            raise RuntimeError('new survival world contains an unearned starter inventory')
        previous_state = state
        if (output / 'run/launcher').exists():
            raise RuntimeError('completed launcher control state was not retired')
    print(f'Two domain lifecycle sessions passed (no DOM/WebView visual proof); evidence: {output}')


if __name__ == '__main__':
    main()
