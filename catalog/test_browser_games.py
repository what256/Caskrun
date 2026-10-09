import copy
import json
import unittest

from validate_browser_games import CATALOG, checked_url, validate


class BrowserCatalogTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.catalog = json.loads(CATALOG.read_text(encoding="utf-8"))

    def test_curated_links_include_explicit_own_files_port(self):
        self.assertEqual(validate(self.catalog), len(self.catalog["entries"]))
        self.assertGreater(len(self.catalog["entries"]), 20)
        vice = next(item for item in self.catalog["entries"] if item["id"] == "vice-city-community-port")
        self.assertTrue(vice["requiresOwnFiles"])
        self.assertIn("own game files", vice["detail"])

    def test_optional_cover_metadata_keeps_legacy_entries_valid(self):
        data = copy.deepcopy(self.catalog)
        for entry in data["entries"]:
            for field in ("artworkURL", "artworkRedirectURLs", "kind"):
                entry.pop(field, None)
        self.assertEqual(validate(data), len(data["entries"]))
        data["entries"][0]["artworkURL"] = "https://games.example.org/cover.jpg"
        data["entries"][0]["artworkRedirectURLs"] = ["https://images.example.org/cover.jpg"]
        self.assertEqual(validate(data), len(data["entries"]))

    def test_artwork_rejects_credentials_fragments_and_invalid_redirects(self):
        values = (
            {"artworkURL": "http://games.example.org/a.png"},
            {"artworkURL": "https://games.example.org/a.png?token=secret"},
            {"artworkURL": "https://games.example.org/a.png#"},
            {"artworkRedirectURLs": ["https://images.example.org/a.png"]},
            {"artworkURL": "https://games.example.org/a.png", "artworkRedirectURLs": ["https://games.example.org/a.png"]},
            {"artworkURL": "https://games.example.org/a.png", "artworkRedirectURLs": ["https://images.example.org/a.png"] * 2},
            {"artworkURL": "https://games.example.org/a.png", "artworkRedirectURLs": [f"https://images.example.org/{i}.png" for i in range(4)]},
            {"artworkURL": "https://games.example.org/a.png", "artworkRedirectURLs": ["https://images.example.org/a.png?password=secret"]},
            {"kind": "installer"},
            {"kind": ["game"]},
        )
        for value in values:
            data = copy.deepcopy(self.catalog)
            for field in ("artworkURL", "artworkRedirectURLs", "kind"):
                data["entries"][0].pop(field, None)
            data["entries"][0].update(value)
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate(data)

    def test_schema_and_unknown_fields_refused(self):
        for version in (0, 2, True, "1"):
            data = copy.deepcopy(self.catalog)
            data["schemaVersion"] = version
            with self.assertRaises(ValueError):
                validate(data)
        data = copy.deepcopy(self.catalog)
        data["entries"][0]["installer"] = "anything.exe"
        with self.assertRaises(ValueError):
            validate(data)

    def test_identity_duplicate_bounds_and_flag(self):
        for field, value in (("id", "Bad ID"), ("name", "x" * 161), ("name", "hidden\nname"), ("requiresOwnFiles", "false")):
            data = copy.deepcopy(self.catalog)
            data["entries"][0][field] = value
            with self.assertRaises(ValueError):
                validate(data)
        for field in ("id", "url"):
            data = copy.deepcopy(self.catalog)
            data["entries"][1][field] = data["entries"][0][field]
            with self.assertRaises(ValueError):
                validate(data)

    def test_website_policy_blocks_non_browser_authorities_and_escapes(self):
        for value in ("file:///game", "http://games.example.org/", "https://" + "name:secret@games.example.org/", "https://games.example.org:443/", "https://127.0.0.1/", "https://[::1]/", "https://localhost/", "https://game.local/", "https://game.test/", "https://" + "games.example.org\\@evil.org/", "https://games.example.org/%00", "https://games.example.org/%0a", "https://games.example.org/%5c", "https://games.example.org/%", "https://games.example.org/a b", "https://gáme.example.org/"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                checked_url(value)
        self.assertEqual(checked_url("https://games.example.org/play?q=one%20two#level"), "https://games.example.org/play?q=one%20two#level")

    def test_sensitive_query_and_fragment_names_refused(self):
        for suffix in ("?token=private", "?ACCESS_TOKEN=private", "?%74%6f%6b%65%6e=private", "?access%5ftoken=private", "?session[id]=private", "?email=person", "?api-key=private", "?auth_token=private", "?AUTH%5fTOKEN=private", "?bearer_token=private", "?authorization_code=private", "?code=private", "?client_secret=private", "?jwt=private", "?oauth_token=private", "?oauth_token_secret=private", "?oauth_verifier=private", "#id_token=private", "#auth[token]=private", "#client-secret=private", "#/play?authorization=private", "?game=1;Password=private"):
            with self.subTest(suffix=suffix), self.assertRaises(ValueError):
                checked_url("https://games.example.org/" + suffix)
        self.assertTrue(checked_url("https://games.example.org/?game=1&level=2#play"))


if __name__ == "__main__":
    unittest.main()
