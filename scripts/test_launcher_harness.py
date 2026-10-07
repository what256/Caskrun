#!/usr/bin/env python3
"""Disposable fake-core checks; never runs a vendor installer or live Wine."""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

sys.dont_write_bytecode = True
SCRIPT = Path(__file__).with_name("test-launchers.py")
spec = importlib.util.spec_from_file_location("launcher_harness", SCRIPT)
harness = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = harness
spec.loader.exec_module(harness)

FAKE_CORE = r'''#!/usr/bin/env python3
import hashlib, json, os
from pathlib import Path
import subprocess, sys, time
home = Path(os.environ["CASKRUN_HOME"])
root = home.parent
calls = root / "core-calls.json"
audit = json.loads(calls.read_text()) if calls.exists() else []
audit.append({"argv": sys.argv[1:], "home": str(home), "environment": dict(os.environ)})
calls.write_text(json.dumps(audit))
command = sys.argv[1]
if command == "stall":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(0.7)"])
    sys.stdin.buffer.read()
    child.wait()
    print("Setup canceled", file=sys.stderr)
    sys.exit(130)
if command == "install-catalog" and (root / "cancel-fixture").exists():
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(0.5)"])
    sys.stdin.buffer.read()
    child.wait()
    sys.exit(130)
if command == "ignore-cancel":
    time.sleep(0.8)
    print("{}")
    sys.exit(0)
if command == "stderr-private":
    print(str(root) + " network failed confidential", file=sys.stderr)
    sys.exit(1)
if command == "quit-all-bottles":
    if (root / "shutdown-fail").exists():
        print(json.dumps({"complete": False, "failures": [{"name": "private", "error": str(home)}]}))
    else:
        print(json.dumps({"complete": True, "failures": [], "stopped_bottles": 1}))
    sys.exit(0)
if command in {"install-catalog", "retry-catalog"}:
    catalog = sys.argv[2]
    bottle = "11111111-1111-4111-8111-111111111111"
    destination = home / "bottles" / bottle
    (destination / "prefix").mkdir(parents=True, exist_ok=True)
    (destination / "prefix" / "system.reg").write_text("fake")
    (destination / "installers").mkdir(exist_ok=True)
    data = b"fake installer bytes"
    (destination / "installers" / "installer.exe").write_bytes(data)
    attempt = {"bottle_id": bottle, "catalog_id": catalog, "stage": "installed", "installer_path": "installers/installer.exe", "installer_sha256": hashlib.sha256(data).hexdigest()}
    (root / "attempt.json").write_text(json.dumps(attempt))
    print(json.dumps({"bottle": {"id": bottle}, "launch": {"pid": os.getpid(), "log_path": str(root)}}))
    sys.exit(0)
if command == "overview":
    attempt = json.loads((root / "attempt.json").read_text()) if (root / "attempt.json").exists() else None
    launchers = {attempt["bottle_id"]: [{"id": "22222222-2222-4222-8222-222222222222", "name": "private", "path": str(root)}]} if attempt else {}
    print(json.dumps({"catalog_attempts": [attempt] if attempt else [], "launchers": launchers, "bottles": []}))
    sys.exit(0)
if command == "run-app":
    print(json.dumps({"pid": os.getpid(), "log_path": str(root)}))
    sys.exit(0)
print("{}")
'''

FAKE_SERVER = r'''#!/usr/bin/env python3
import os
from pathlib import Path
import sys
prefix = Path(os.environ.get("WINEPREFIX", "."))
if (prefix / "server-fail").exists():
    sys.exit(1)
if (prefix / "server-stall").exists():
    import time
    time.sleep(2)
sys.exit(0)
'''

FAKE_INSPECTOR = r'''#!/usr/bin/env python3
import os
print("p" + str(os.getppid()))
print("cpython")
'''


class HarnessTests(unittest.TestCase):
    def setUp(self):
        build = SCRIPT.parent.parent / ".build"
        build.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="launcher-harness-checks-", dir=build)
        self.project = Path(self.temporary.name)
        self.build = self.project / ".build"
        self.runtime = self.build / "fixture-runtime" / "bin"
        self.runtime.mkdir(parents=True)
        self.core = self.build / "fixture-core"
        self.core.write_text(FAKE_CORE)
        self.core.chmod(0o700)
        self.wine = self.runtime / "wine"
        self.wine.write_text("#!/bin/sh\nexit 0\n")
        self.wine.chmod(0o700)
        self.server = self.runtime / "wineserver"
        self.server.write_text(FAKE_SERVER)
        self.server.chmod(0o700)
        boot = self.runtime / "wineboot"
        boot.write_text("#!/bin/sh\nexit 0\n")
        boot.chmod(0o700)
        self.inspector = self.build / "fixture-inspector"
        self.inspector.write_text(FAKE_INSPECTOR)
        self.inspector.chmod(0o700)
        self.session = harness.Session.setup(self.project, self.core, self.wine)

    def tearDown(self):
        # All children used in timeout tests have bounded sub-second lifetime;
        # tests wait for them before the fixture TemporaryDirectory is removed.
        self.temporary.cleanup()

    def lsof_fixture(self):
        return mock.patch.object(harness.shutil, "which", return_value=str(self.inspector))

    def test_catalog_matches_the_tracked_entries(self):
        # A catalog entry the harness does not know cannot be tested at all, and
        # status collection would reject it as unexpected test storage.
        entries = json.loads((harness.PROJECT / "catalog/apps.json").read_text())["entries"]
        self.assertEqual(sorted(harness.CATALOG), sorted(entry["id"] for entry in entries))

    def test_setup_never_invokes_core_and_home_isolated(self):
        session = self.session
        self.assertFalse((session.root / "core-calls.json").exists())
        with mock.patch.dict(os.environ, {"WINEPREFIX": "external", "DYLD_LIBRARY_PATH": "external", "GITHUB_TOKEN": "synthetic"}):
            environment = session.environment()
        self.assertEqual(environment["CASKRUN_HOME"], str(session.root / "storage"))
        self.assertEqual(environment["HOME"], str(session.root / "host"))
        self.assertNotEqual(environment["HOME"], os.environ.get("HOME"))
        self.assertTrue(all((session.host / name).is_dir() for name in ("Desktop", "Documents", "Downloads", "Library")))
        for key in ("WINEPREFIX", "DYLD_LIBRARY_PATH", "GITHUB_TOKEN"):
            self.assertNotIn(key, environment)
        self.assertEqual(session.summary["gameplay"], "untested")

    def test_separate_install_status_run_and_sanitized_observation(self):
        session = self.session
        with session.locked():
            result = session.install("epic-games-store", 5)
            self.assertEqual(result.exit_code, 0)
            session.collect_status()
            self.assertEqual(session.run("epic-games-store").exit_code, 0)
            session.record("epic-games-store", "rendered", "none")
        entry = session.summary["launchers"]["epic-games-store"]
        self.assertEqual(entry["setup_stage"], "installed")
        self.assertTrue(entry["receipt_matches"])
        self.assertEqual(entry["installer_bytes"], len(b"fake installer bytes"))
        self.assertEqual(entry["observed_ui"], "rendered")
        receipt = session.receipt.read_text()
        self.assertNotIn(str(session.root), receipt)
        self.assertNotIn("private", receipt)
        calls = json.loads((session.root / "core-calls.json").read_text())
        self.assertEqual(calls[-1]["argv"][0], "run-app")
        self.assertTrue(all(call["home"] == str(session.storage) for call in calls))

    def test_existing_attempt_is_retried_instead_of_duplicate_install(self):
        self.session.install("gog-galaxy", 5)
        self.session.install("gog-galaxy", 5)
        calls = json.loads((self.session.root / "core-calls.json").read_text())
        self.assertEqual(sum(call["argv"][0] == "install-catalog" for call in calls), 1)
        self.assertEqual(sum(call["argv"][0] == "retry-catalog" for call in calls), 1)

    def test_timeout_closes_stdin_and_tracks_descendant_until_reaped(self):
        result = self.session.invoke(["stall"], deadline=0.1, cancel_grace=2)
        self.assertEqual(result.exit_code, 130)
        self.assertTrue(result.timed_out)
        self.assertFalse(result.unresolved)
        self.assertGreaterEqual(len(self.session.manifest["tracked"]), 2)
        table = harness.process_table()
        self.assertFalse(any(table.get(int(pid), {}).get("birth") == birth for pid, birth in self.session.manifest["tracked"].items()))

    def test_uncooperative_timeout_preserves_fixtures_and_does_not_kill(self):
        result = self.session.invoke(["ignore-cancel"], deadline=0.05, cancel_grace=0.05)
        self.assertTrue(result.unresolved)
        with self.assertRaisesRegex(harness.HarnessError, "core invocation remains active"):
            self.session.cleanup()
        self.assertTrue(self.session.root.exists())
        # Wait for this explicitly created, bounded fixture child, with no kill.
        import time
        time.sleep(1)
        with self.lsof_fixture():
            self.session.cleanup()
        self.assertFalse(self.session.root.exists())

    def test_error_output_kept_local_and_receipt_only_contains_reason(self):
        result = self.session.invoke(["stderr-private"])
        self.assertEqual(result.reason, "network")
        self.assertNotIn(str(self.session.root), self.session.receipt.read_text())
        self.assertNotIn("confidential", self.session.receipt.read_text())
        self.assertIn("confidential", next(self.session.raw.glob("*.stderr")).read_text())

    def test_cleanup_only_owned_session_and_retains_sanitized_receipt(self):
        outside = self.build / "unowned-test-storage"
        outside.mkdir()
        untouched = outside / "keep.txt"
        untouched.write_text("not owned")
        self.session.install("battle-net", 5)
        with self.lsof_fixture(), self.session.locked():
            self.session.cleanup()
        self.assertFalse(self.session.root.exists())
        self.assertEqual(untouched.read_text(), "not owned")
        summary = json.loads(self.session.receipt.read_text())
        self.assertEqual(summary["cleanup"], "removed")
        self.assertNotIn(str(self.project), json.dumps(summary))
        self.assertFalse(list(self.build.glob("launcher-validation-*")))

    def test_shutdown_failure_refuses_to_delete(self):
        (self.session.root / "shutdown-fail").touch()
        with self.assertRaisesRegex(harness.HarnessError, "shutdown failed"):
            self.session.cleanup()
        self.assertTrue(self.session.root.exists())
        self.assertEqual(self.session.summary["cleanup"], "blocked")

    def test_wine_server_failure_refuses_to_delete(self):
        self.session.install("ea-app", 5)
        prefix = next((self.session.storage / "bottles").iterdir()) / "prefix"
        (prefix / "server-fail").touch()
        with self.assertRaisesRegex(harness.HarnessError, "could not be verified idle"):
            self.session.cleanup()
        self.assertTrue(self.session.root.exists())

    def test_live_owned_process_blocks_cleanup_without_signaling(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(0.4)"])
        try:
            table = harness.process_table()
            self.session.manifest["tracked"][str(child.pid)] = table[child.pid]["birth"]
            with self.assertRaisesRegex(harness.HarnessError, "Owned test processes remain"):
                self.session.assert_idle()
            self.assertIsNone(child.poll())
        finally:
            child.wait(timeout=2)

    def test_reused_pid_birth_stamp_does_not_block(self):
        self.session.manifest["tracked"][str(os.getpid())] = "synthetic old birth stamp"
        with self.lsof_fixture():
            self.session.assert_idle()

    def test_external_open_handle_blocks_cleanup(self):
        self.inspector.write_text("#!/bin/sh\nprintf 'p999999\\ncexternal\\n'\n")
        with self.lsof_fixture(), self.assertRaisesRegex(harness.HarnessError, "still holds test files"):
            self.session.cleanup()
        self.assertTrue(self.session.root.exists())

    def test_missing_or_inconclusive_file_inspector_blocks_cleanup(self):
        with mock.patch.object(harness.shutil, "which", return_value=None):
            with self.assertRaisesRegex(harness.HarnessError, "inspection is unavailable"):
                self.session.cleanup()
        self.inspector.write_text("#!/bin/sh\nexit 0\n")
        with self.lsof_fixture(), self.assertRaisesRegex(harness.HarnessError, "inconclusive"):
            self.session.cleanup()
        self.assertTrue(self.session.root.exists())

    def test_changed_marker_root_or_executable_fails_closed(self):
        marker = self.session.root / ".caskrun-test-owner.json"
        value = json.loads(marker.read_text())
        value["token"] = "0" * 64
        marker.write_text(json.dumps(value))
        with self.assertRaisesRegex(harness.HarnessError, "ownership"):
            harness.Session(self.project, self.session.session_id)
        value["token"] = self.session.manifest["owner_token"]
        marker.write_text(json.dumps(value))
        self.core.write_text(FAKE_CORE + "\n# changed\n")
        with self.assertRaisesRegex(harness.HarnessError, "executable changed"):
            harness.Session(self.project, self.session.session_id)

    def test_symlinked_storage_and_traversal_sessions_refused(self):
        self.session.storage.rmdir()
        outside = self.build / "outside"
        outside.mkdir()
        self.session.storage.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(harness.HarnessError, "symbolic-link"):
            harness.Session(self.project, self.session.session_id)
        with self.assertRaisesRegex(harness.HarnessError, "identifier"):
            harness.Session(self.project, "../outside")

    def test_internal_wine_dispatcher_link_keeps_invocation_basename(self):
        self.wine.rename(self.runtime / "_dispatch")
        self.wine.symlink_to("_dispatch")
        # Same bytes; the stored wine path must still end in wine.
        resumed = harness.Session(self.project, self.session.session_id)
        self.assertEqual(resumed.wine.name, "wine")
        self.assertEqual(Path(resumed.environment()["CASKRUN_WINE"]).name, "wine")

    def test_standard_wine_links_are_unlinked_without_following(self):
        outside = self.build / "external-files"
        outside.mkdir()
        keep = outside / "keep.txt"
        keep.write_text("keep")
        (self.session.host / "external-link").symlink_to(outside, target_is_directory=True)
        with self.lsof_fixture():
            self.session.cleanup()
        self.assertEqual(keep.read_text(), "keep")

    def test_installer_traversal_receipt_is_rejected(self):
        self.session.install("itch-io-app", 5)
        path = self.session.root / "attempt.json"
        receipt = json.loads(path.read_text())
        receipt["installer_path"] = "../../outside.exe"
        path.write_text(json.dumps(receipt))
        with self.assertRaisesRegex(harness.HarnessError, "escapes test storage"):
            self.session.collect_status()

    def test_progress_during_mutation_lock_is_read_only_and_sanitized(self):
        bottle = self.session.storage / "bottles" / "11111111-1111-4111-8111-111111111111"
        bottle.mkdir(parents=True)
        attempt = {"catalog_id": "rockstar-games-launcher", "stage": "running", "last_error": str(self.session.root), "last_log_name": "private.log"}
        (bottle / "catalog-attempt.json").write_text(json.dumps(attempt))
        manifest_before = (self.session.root / "session.json").read_bytes()
        receipt_before = self.session.receipt.read_bytes()
        with self.session.locked():
            resumed = harness.Session(self.project, self.session.session_id)
            progress = resumed.progress()
        self.assertEqual(progress["launchers"]["rockstar-games-launcher"]["setup_stage"], "running")
        self.assertTrue(progress["launchers"]["rockstar-games-launcher"]["setup_has_error"])
        self.assertNotIn(str(self.session.root), json.dumps(progress))
        self.assertEqual(manifest_before, (self.session.root / "session.json").read_bytes())
        self.assertEqual(receipt_before, self.session.receipt.read_bytes())
        self.assertFalse((self.session.root / "core-calls.json").exists())

    def test_explicit_cancel_requests_only_current_invocations_eof(self):
        (self.session.root / "cancel-fixture").touch()
        results = []
        errors = []
        def invoke():
            try:
                results.append(self.session.invoke(["install-catalog", "battle-net"], deadline=5, cancel_grace=2))
            except Exception as error:
                errors.append(error)
        worker = threading.Thread(target=invoke)
        worker.start()
        deadline = time.monotonic() + 2
        while not (self.session.root / "core-calls.json").exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue((self.session.root / "core-calls.json").exists())
        resumed = harness.Session(self.project, self.session.session_id)
        with mock.patch.object(os, "kill", side_effect=AssertionError("No process signals allowed")):
            resumed.cancel()
            worker.join(timeout=3)
        self.assertFalse(worker.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(results[0].exit_code, 130)
        self.assertFalse(results[0].timed_out)
        self.assertTrue(self.session.summary["commands"][-1]["cancellation_requested"])
        self.assertIsNone(self.session.manifest["active_invocation"])
        with self.assertRaisesRegex(harness.HarnessError, "No foreground setup"):
            harness.Session(self.project, self.session.session_id).cancel()

    def test_log_sizes_digests_and_shell_links_omit_names_and_targets(self):
        self.session.install("ubisoft-connect", 5)
        bottle = next((self.session.storage / "bottles").iterdir())
        logs = bottle / "logs"
        logs.mkdir()
        (logs / "private-diagnostic.log").write_bytes(b"private diagnostic bytes")
        account = bottle / "prefix" / "drive_c" / "users" / "private-account"
        account.mkdir(parents=True)
        (account / "Desktop").symlink_to(self.session.host / "Desktop")
        (account / "Documents").symlink_to(self.project / "outside-documents")
        self.session.collect_status()
        evidence = self.session.summary["launchers"]["ubisoft-connect"]["evidence"]
        self.assertEqual(evidence["logs"]["files"], 1)
        self.assertEqual(evidence["logs"]["bytes"], len(b"private diagnostic bytes"))
        self.assertEqual(len(evidence["logs"]["sha256"][0]), 64)
        self.assertEqual(evidence["shell_links"], {"contained": 1, "external": 1})
        receipt = self.session.receipt.read_text()
        self.assertNotIn("private-account", receipt)
        self.assertNotIn("outside-documents", receipt)
        self.assertNotIn("private-diagnostic", receipt)

    def test_record_disallows_freeform_personal_text(self):
        with self.assertRaises(harness.HarnessError):
            self.session.record("gog-galaxy", "rendered", "private text")
        with self.assertRaises(harness.HarnessError):
            self.session.record("not-a-catalog-entry", "rendered", "none")


if __name__ == "__main__":
    unittest.main()
