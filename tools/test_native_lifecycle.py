"""Checks lifecycle evidence rules without launching a native process."""
from __future__ import annotations
import copy
import hashlib
import json
import unittest
from verify_native_lifecycle import verify_report

NONCE = 'fresh-run'
STATE = {'world': 'world-one', 'lock': 'lock-one'}

def report(legacy=False):
    durable = {'world_id': 'world-one', 'revision': 3}
    roles = ('shell', 'world', 'shell') if legacy else ('shell',)
    return {'outcome': {'outcome': 'product-exited', 'exit_kind': 'shell-quit'},
            'failures': [], 'hops': [{'role': {'kind': role}} for role in roles],
            'last_durable_world': durable if legacy else None,
            'last_written_world': dict(durable) if legacy else None}

def signed_probe(**changes):
    result = {'schema': 'latticeaxiom.in-process-durable-world.v1', 'world_id': 'world-one',
              'revision': 3, 'written_revision': 3, 'world_lock_hash': 'lock-one',
              'return_to_shell': True, 'nonce': NONCE}
    result.update(changes)
    result['checksum'] = hashlib.sha256(json.dumps(result, sort_keys=True, separators=(',', ':'),
                                                  ensure_ascii=False).encode('utf-8')).hexdigest()
    return result

def domain():
    return {'schema': 'latticeaxiom.lifecycle-domain-evidence.v1', 'scope': 'domain-commands-and-state',
            'events': [
                {'event': 'world-entered', 'data': {'world': 'world-one', 'mode': 'game'}},
                {'event': 'command-accepted', 'data': {'method': 'game.exit'}},
                {'event': 'returned-to-shell', 'data': {'mode': 'shell'}},
                {'event': 'completed', 'data': {'command': 'app.quit'}},
            ]}

def verify(probe=None, evidence=None, previous=None, state=None):
    return verify_report(report(), previous, domain() if evidence is None else evidence,
                         signed_probe() if probe is None else probe, NONCE, STATE if state is None else state)

class NativeLifecycleEvidenceTests(unittest.TestCase):
    def test_same_window_requires_domain_and_genuine_durable_probe(self):
        self.assertEqual(verify()['revision'], 3)
        with self.assertRaisesRegex(RuntimeError, 'domain transition'):
            verify_report(report(), None)
        with self.assertRaisesRegex(RuntimeError, 'physically durable'):
            verify_report(report(), None, domain())

    def test_probe_checksum_nonce_and_publication_revision_are_required(self):
        candidate = signed_probe()
        candidate['revision'] = 4
        with self.assertRaisesRegex(RuntimeError, 'checksum'):
            verify(probe=candidate)
        for changes, message in [({'nonce': 'old-run'}, 'this run'),
                                 ({'revision': 0, 'written_revision': 0}, 'physically durable'),
                                 ({'written_revision': 4}, 'physically durable'),
                                 ({'return_to_shell': False}, 'save and return')]:
            with self.subTest(changes=changes), self.assertRaisesRegex(RuntimeError, message):
                verify(probe=signed_probe(**changes))

    def test_probe_must_match_observed_world_and_lock(self):
        for state in ({'world': 'other', 'lock': 'lock-one'}, {'world': 'world-one', 'lock': 'other'}):
            with self.assertRaisesRegex(RuntimeError, 'identity or lock'):
                verify(state=state)

    def test_incomplete_or_reordered_domain_journey_is_rejected(self):
        candidate = domain()
        candidate['events'].pop(2)
        with self.assertRaisesRegex(RuntimeError, 'lacks world'):
            verify(evidence=candidate)
        candidate = domain()
        candidate['events'][0], candidate['events'][1] = candidate['events'][1], candidate['events'][0]
        with self.assertRaisesRegex(RuntimeError, 'out of order'):
            verify(evidence=candidate)

    def test_world_mismatch_and_durable_regression_are_rejected(self):
        candidate = copy.deepcopy(domain())
        candidate['events'][0]['data']['world'] = 'another-world'
        with self.assertRaisesRegex(RuntimeError, 'differs'):
            verify(evidence=candidate)
        with self.assertRaisesRegex(RuntimeError, 'regressed'):
            verify(previous={'world_id': 'world-one', 'revision': 4})

    def test_legacy_process_sequence_keeps_original_durability_requirement(self):
        candidate = report(legacy=True)
        self.assertEqual(verify_report(candidate, None)['revision'], 3)
        candidate['last_durable_world'] = None
        with self.assertRaisesRegex(RuntimeError, 'physically durable'):
            verify_report(candidate, None)

if __name__ == '__main__':
    unittest.main()
