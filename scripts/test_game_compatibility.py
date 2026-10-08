import copy
import hashlib
import importlib.util
import json
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("game_compatibility_generator", ROOT / "scripts/generate-game-compatibility.py")
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


class CompatibilitySnapshotTests(unittest.TestCase):
    def setUp(self):
        temporary_root = ROOT / ".build/compatibility-checks"
        temporary_root.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="source-", dir=temporary_root)
        self.source = pathlib.Path(self.temporary.name)
        self.game = {"id": "example-game", "steam_appid": 620, "title": "Example Game",
                     "notes": "Discard this narrative", "provenance": "Discard identities"}
        self.report = {"issue": 92, "date": "2026-09-16T19:30:05Z", "steam_appid": "620", "rating": "5",
                       "renderer": "dxmt", "chip": "Apple M4", "macos": "26.0.0",
                       "engine": "x64-sikarugir10.0_6-r3", "user": "Discard contributor",
                       "model_identifier": "Discard identifier", "notes": "Discard narrative",
                       "issues": "Discard issue text"}
        self.write_source()

    def tearDown(self):
        self.temporary.cleanup()

    def write_source(self, reports=None, games=None):
        entries = []
        files = {"db/games/example-game.json": json.dumps(self.game if games is None else games),
                 "db/reports/example-game.jsonl": "\n".join(json.dumps(r) for r in (reports or [self.report])) + "\n"}
        for name, contents in files.items():
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            raw = contents.encode("utf-8")
            path.write_bytes(raw)
            digest = hashlib.sha1(b"blob " + str(len(raw)).encode("ascii") + b"\0" + raw).hexdigest()
            entries.append({"path": name, "sha": digest, "type": "blob"})
        (self.source / "tree.json").write_text(json.dumps({"sha": GENERATOR.SOURCE_REVISION, "truncated": False, "tree": entries}))

    def test_only_allowlisted_metadata_enters_snapshot(self):
        value = GENERATOR.build(self.source)
        GENERATOR.validate_snapshot(value)
        encoded = json.dumps(value)
        self.assertNotIn("Discard", encoded)
        self.assertNotIn("user", value["games"][0]["reports"][0])
        self.assertIsNone(value["games"][0]["reports"][0]["memory_gib"])

    def test_changed_source_blob_is_refused(self):
        (self.source / "db/games/example-game.json").write_text(json.dumps({**self.game, "title": "Changed"}))
        with self.assertRaisesRegex(ValueError, "Git tree"):
            GENERATOR.build(self.source)

    def test_incomplete_source_tree_is_refused(self):
        (self.source / "db/games/example-game.json").unlink()
        with self.assertRaisesRegex(ValueError, "missing or adds"):
            GENERATOR.build(self.source)

    def test_unreviewed_revision_is_refused(self):
        path = self.source / "tree.json"
        tree = json.loads(path.read_text())
        tree["sha"] = "a" * 40
        path.write_text(json.dumps(tree))
        with self.assertRaisesRegex(ValueError, "reviewed revision"):
            GENERATOR.build(self.source)

    def test_unstructured_hardware_becomes_unknown(self):
        self.report.update(chip="a personal computer label", macos="a narrative")
        self.write_source()
        report = GENERATOR.build(self.source)["games"][0]["reports"][0]
        self.assertIsNone(report["chip"])
        self.assertIsNone(report["macos"])

    def test_engine_id_is_extracted_without_narrative(self):
        self.report["engine"] = "wine-10.0 (Sikarugir) revision 6 · x64-sikarugir10.0_6-r7"
        self.write_source()
        self.assertEqual(GENERATOR.build(self.source)["games"][0]["reports"][0]["engine"], "x64-sikarugir10.0_6-r7")

    def test_no_issue_uses_exact_pinned_report_line(self):
        self.report["issue"] = None
        self.write_source()
        value = GENERATOR.build(self.source)
        GENERATOR.validate_snapshot(value)
        self.assertEqual(value["games"][0]["reports"][0]["source_url"],
                         GENERATOR.REPOSITORY + "/blob/" + GENERATOR.SOURCE_REVISION + "/db/reports/example-game.jsonl#L1")

    def test_unrated_and_non_steam_reports_are_not_invented(self):
        unrated = dict(self.report, rating=None)
        non_steam = dict(self.report, issue=93, steam_appid=None)
        self.write_source(reports=[unrated, non_steam])
        self.assertEqual(GENERATOR.build(self.source)["games"][0]["reports"], [])

    def test_invalid_dates_and_unknown_renderers_are_refused(self):
        for key, value in [("date", "2026-02-30"), ("renderer", "run-a-script")]:
            with self.subTest(field=key):
                original = copy.deepcopy(self.report)
                self.report[key] = value
                self.write_source()
                with self.assertRaises(ValueError):
                    GENERATOR.build(self.source)
                self.report = original

    def test_duplicate_source_issues_are_refused(self):
        self.write_source(reports=[self.report, self.report])
        with self.assertRaisesRegex(ValueError, "duplicate community issue"):
            GENERATOR.build(self.source)

    def test_snapshot_refuses_added_identity_and_wrong_url(self):
        base = GENERATOR.build(self.source)
        for key, value in [("user", "not allowed"), ("source_url", "https://example.invalid/92")]:
            with self.subTest(field=key):
                altered = copy.deepcopy(base)
                altered["games"][0]["reports"][0][key] = value
                with self.assertRaises(ValueError):
                    GENERATOR.validate_snapshot(altered)

    def test_checked_in_snapshot_and_license_are_pinned(self):
        value = json.loads((ROOT / "catalog/game-compatibility.json").read_text())
        GENERATOR.validate_snapshot(value)
        self.assertEqual(len(value["games"]), 373)
        self.assertEqual(sum(len(game["reports"]) for game in value["games"]), 71)
        self.assertTrue((ROOT / "catalog/licenses/highball-db-CC0.txt").read_text().startswith("Creative Commons Legal Code"))


if __name__ == "__main__":
    unittest.main()
