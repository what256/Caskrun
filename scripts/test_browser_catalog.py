#!/usr/bin/env python3
"""Offline checks for browser-game metadata; never load games or artwork."""
import copy
import importlib.util
import json
import unittest
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('browser_catalog_validator', ROOT / 'catalog/validate_browser_games.py')
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class BrowserCatalogTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads((ROOT / 'catalog/browser-games.json').read_text())

    def test_complete_individual_provider_catalog(self):
        entries = self.data['entries']
        self.assertEqual(validator.validate(self.data), len(entries))
        games = [e for e in entries if 'collectionGroup' in e]
        self.assertEqual(Counter(e['collectionGroup'] for e in games),
                         {'pc': 5, 'neogeo': 90, 'capcom': 62, 'arcade': 75, 'flash': 286})
        self.assertEqual(len({e['url'] for e in games}), 518)
        self.assertTrue(all(e['url'].startswith('https://wasmarcade.com/')
                            and e.get('artworkURL', '').startswith('https://wasmarcade.com/')
                            and e.get('kind', 'game') != 'collection' for e in games))
        self.assertTrue({'https://wasmarcade.com/gta-5', 'https://wasmarcade.com/retro-games/dragon-ball-z',
                         'https://wasmarcade.com/flash-games/happy-wheels'} <= {e['url'] for e in games})

    def test_old_bookmarks_keep_their_cover(self):
        urls = {e['url'] for e in self.data['entries']}
        self.assertIn('https://wasmarcade.com/pc-games', urls)
        self.assertIn('https://wasmarcade.com/retro-games/metal-slug', urls)

    def test_bad_collection_group_rejected(self):
        data = copy.deepcopy(self.data)
        data['entries'][0]['collectionGroup'] = ['invalid']
        with self.assertRaises(ValueError):
            validator.validate(data)


if __name__ == '__main__':
    unittest.main()
