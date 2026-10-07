#!/usr/bin/env python3
"""Small regression checks for catalog validation and safe page URLs."""

import copy
import json
import unittest

from validate import CATALOG_DIR, CatalogError, validate_catalog


class CatalogValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schema = json.loads((CATALOG_DIR / "schema.json").read_text(encoding="utf-8"))
        cls.catalog = json.loads((CATALOG_DIR / "apps.json").read_text(encoding="utf-8"))

    def test_starter_catalog(self):
        self.assertEqual(validate_catalog(self.catalog, self.schema), 8)

    def test_rejects_duplicate_identity(self):
        data = copy.deepcopy(self.catalog)
        data["entries"][1]["id"] = data["entries"][0]["id"]
        with self.assertRaisesRegex(CatalogError, "duplicate ID"):
            validate_catalog(data, self.schema)

    def test_rejects_id_with_trailing_newline(self):
        data = copy.deepcopy(self.catalog)
        data["entries"][0]["id"] = "steam\n"
        with self.assertRaisesRegex(CatalogError, "invalid ID"):
            validate_catalog(data, self.schema)

    def test_rejects_unverified_compatibility_claim(self):
        data = copy.deepcopy(self.catalog)
        data["entries"][0]["compatibilityStatus"] = "working"
        with self.assertRaisesRegex(CatalogError, "compatibilityStatus"):
            validate_catalog(data, self.schema)

    def test_rejects_unsafe_or_malformed_urls(self):
        for url in (
            "http://example.org/download",
            "https://user@example.org/download",
            "https://example.org:8443/download",
            "https://127.0.0.1/download",
            "https://example.org/down\nload",
        ):
            with self.subTest(url=url):
                data = copy.deepcopy(self.catalog)
                data["entries"][0]["downloadPageURL"] = url
                with self.assertRaises(CatalogError):
                    validate_catalog(data, self.schema)


if __name__ == "__main__":
    unittest.main()
