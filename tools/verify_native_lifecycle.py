"""Exercise two native shell/world/save/quit sessions in a fresh isolated workspace."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess


def verify_report(report: dict, previous: dict | None) -> dict:
    outcome = report['outcome']
    if outcome['outcome'] != 'product-exited' or outcome.get('exit_kind') != 'shell-quit' or report['failures']:
        raise RuntimeError(f"native lifecycle failed: {outcome}; {report['failures']}")
    if [hop['role']['kind'] for hop in report['hops']] != ['shell', 'world', 'shell']:
        raise RuntimeError('expected shell -> world -> shell, followed by product exit')
    durable = report['last_durable_world']
    if not durable or durable['revision'] <= 0 or durable != report['last_written_world']:
        raise RuntimeError('exit lacks a matching physically durable world receipt')
    if previous and durable['world_id'] != previous['world_id']:
        raise RuntimeError('second run did not Continue the same saved world')
    if previous and durable['revision'] < previous['revision']:
        raise RuntimeError('second run regressed the durable revision')
    return durable


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True, help='built development latticeaxiom-play executable')
    parser.add_argument('--runtime', type=Path, required=True, help='prepared locks and catalog/CAS')
    parser.add_argument('--output', type=Path, required=True, help='new, non-existing QA output directory')
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
    if os.name == 'nt':
        rust_lib = subprocess.check_output(['rustc', '--print', 'target-libdir'], text=True).strip()
        environment['PATH'] = os.pathsep.join((rust_lib, str(binary.parent / 'deps'), environment['PATH']))
    previous = None
    for number in (1, 2):
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
        previous = verify_report(report, previous)
        if (output / 'run/launcher').exists():
            raise RuntimeError('completed launcher control state was not retired')
    print(f'Two native sessions passed; evidence: {output}')


if __name__ == '__main__':
    main()
