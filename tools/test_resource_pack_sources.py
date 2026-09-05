"""Check that shipped appearance packages are independently client-scoped."""
import json
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ResourcePackageBoundaryTests(unittest.TestCase):
    def test_resources_have_a_separate_client_graph_and_no_game_authority(self):
        profile = tomllib.loads((ROOT / 'profiles/resources.toml').read_text())
        world = tomllib.loads((ROOT / 'profiles/dev.toml').read_text())
        self.assertEqual(profile['projection_domains'], ['client'])
        self.assertTrue(set(profile['roots']).isdisjoint(world['roots']))
        kinds = set()
        for source in profile['sources']:
            folder = ROOT / source['path']
            manifest = tomllib.loads((folder / 'latticeaxiom-package.toml').read_text())
            self.assertEqual(manifest['domains'], ['client'])
            self.assertNotIn('rust', manifest)
            self.assertNotIn('dependencies', manifest)
            descriptor = json.loads((folder / 'data/resource-pack.json').read_text())
            kinds.add(descriptor['kind'])
            if descriptor['kind'] == 'shaders':
                self.assertTrue((folder / descriptor['water_shader']).is_file())
        self.assertEqual(kinds, {'textures', 'shaders'})


if __name__ == '__main__':
    unittest.main()
