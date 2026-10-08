#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Original synthetic DXMT evidence checks. Never launches Wine or Windows."""
from __future__ import annotations

from copy import deepcopy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dxmt_runtime_harness", Path(__file__).with_name("test-dxmt-runtime.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
EXPECTED = {"d3d11.dll": "a" * 64, "dxgi.dll": "b" * 64}


def evidence(architecture="x64", *, lifecycle=False, identity_only=False, gated=False):
    records = [{"event": "probe", "schema": 3 if identity_only else 4 if gated else 2 if lifecycle else 1,
                "architecture": architecture, "watchdog_ms": 30000, "driver": "none" if identity_only else "hardware"}]
    if identity_only:
        records[0]["mode"] = "identity"
    elif lifecycle:
        records[0]["mode"] = "lifecycle"
    if gated:
        records[0]["identity_source"] = "host_mapped_files"
    for name, digest in EXPECTED.items():
        records.append({"event": "module", "name": name,
            "path": "C:\\windows\\system32\\" + name, "sha256": digest,
            "expected_sha256": digest, "matches_receipt": True})
    none_checks = runner.NONE_CHECKS.copy()
    level_checks = runner.LIFECYCLE_CHECKS if lifecycle else runner.LEVEL_CHECKS
    if identity_only:
        none_checks = runner.Counter({name: 1 for name in ("watchdog_event", "watchdog_thread", "load_d3d11_dxgi", "d3d11.dll", "dxgi.dll")})
        level_checks = runner.Counter()
    elif lifecycle:
        del none_checks["create_hwnd"]
    if gated:
        none_checks["identity_gate"] = 1
    for level, checks in (("none", none_checks), ("10_0", level_checks), ("11_0", level_checks)):
        for name, count in checks.items():
            records.extend({"event": "check", "feature_level": level, "name": name,
                            "passed": True, "hresult": "0x00000000"} for _ in range(count))
    for level in (() if identity_only else ("10_0", "11_0")):
        for name, count in (runner.LIFECYCLE_PIXELS if lifecycle else runner.PIXELS).items():
            expected = [0, 255, 0, 255] if name.endswith("triangle_center") else [32, 64, 128, 255]
            records.extend({"event": "pixel", "feature_level": level, "name": name,
                "actual_rgba": expected.copy(), "expected_rgba": expected.copy(),
                "tolerance": 3, "passed": True} for _ in range(count))
        if lifecycle:
            records.append({"event": "dimensions", "feature_level": level,
                "source": "resized_backbuffer", "width": 192, "height": 96})
    records.append({"event": "modules", "winemetal_loaded": not identity_only, "wined3d_loaded": identity_only})
    records.append({"event": "result", "passed": True, "failed_checks": 0, "exit_code": 0})
    return records


def encode(records):
    return ("Wine diagnostic retained locally\n" + "\n".join(json.dumps(row) for row in records) + "\n").encode()


class EvidenceTests(unittest.TestCase):
    def parse(self, records, architecture="x64", *, lifecycle=False):
        return runner.parse_probe(encode(records), architecture, EXPECTED, lifecycle=lifecycle)

    def test_complete_x64_and_x86_evidence(self):
        for architecture in ("x64", "x86"):
            result = self.parse(evidence(architecture), architecture)
            self.assertTrue(result["passed"])
            self.assertEqual(result["feature_levels"]["10_0"]["present_count"], 4)
            self.assertNotIn("path", json.dumps(result))

    def test_loaded_dll_substitution_refused(self):
        for key, value in (("sha256", "c" * 64), ("expected_sha256", "c" * 64),
                           ("matches_receipt", False), ("path", "C:\\windows\\system32\\other.dll")):
            records = evidence()
            records[1][key] = value
            with self.assertRaises(runner.HarnessError):
                self.parse(records)

    def test_wrong_architecture_refused(self):
        with self.assertRaises(runner.HarnessError):
            self.parse(evidence("x86"))

    def test_missing_or_duplicate_present_refused(self):
        original = evidence()
        index = next(index for index, row in enumerate(original) if row.get("name") == "present")
        missing = deepcopy(original)
        missing.pop(index)
        extra = deepcopy(original)
        extra.insert(index, deepcopy(extra[index]))
        for records in (missing, extra):
            with self.assertRaises(runner.HarnessError):
                self.parse(records)

    def test_claimed_pass_cannot_hide_incorrect_pixel(self):
        records = evidence()
        row = next(row for row in records if row.get("name") == "offscreen_triangle_center")
        row["actual_rgba"] = [32, 64, 128, 255]
        with self.assertRaises(runner.HarnessError):
            self.parse(records)

    def test_float_or_boolean_channels_refused(self):
        for invalid in (32.0, True):
            records = evidence()
            row = next(row for row in records if row.get("event") == "pixel")
            row["actual_rgba"][0] = invalid
            with self.assertRaises(runner.HarnessError):
                self.parse(records)

    def test_missing_level_and_failed_stage_refused(self):
        missing = [row for row in evidence() if row.get("feature_level") != "11_0"]
        failed = evidence()
        next(row for row in failed if row.get("event") == "check")["passed"] = False
        for records in (missing, failed):
            with self.assertRaises(runner.HarnessError):
                self.parse(records)

    def test_missing_bridge_and_duplicate_module_refused(self):
        bridge = evidence()
        bridge[-2]["winemetal_loaded"] = False
        duplicate = evidence()
        duplicate.insert(1, deepcopy(duplicate[1]))
        for records in (bridge, duplicate):
            with self.assertRaises(runner.HarnessError):
                self.parse(records)

    def test_wined3d_presence_is_reported_not_selection_proof(self):
        records = evidence()
        records[-2]["wined3d_loaded"] = True
        self.assertTrue(self.parse(records)["wined3d_loaded"])

    def test_unknown_duplicate_json_and_truncated_output_refused(self):
        records = evidence()
        records.insert(1, {"event": "unknown"})
        with self.assertRaises(runner.HarnessError):
            self.parse(records)
        for data in (encode(evidence()[:-1]), encode(evidence()) + b'{"event":"check","event":"result"}\n'):
            with self.assertRaises(runner.HarnessError):
                runner.parse_probe(data, "x64", EXPECTED)

    def test_output_limit(self):
        with self.assertRaises(runner.HarnessError):
            runner.parse_probe(b"x" * (runner.MAX_LOG + 1), "x64", EXPECTED)

    def test_complete_lifecycle_x64_and_x86_evidence(self):
        for architecture in ("x64", "x86"):
            result = self.parse(evidence(architecture, lifecycle=True), architecture, lifecycle=True)
            self.assertEqual(result["mode"], "lifecycle")
            for level in result["feature_levels"].values():
                self.assertEqual(level["present_count"], 7)
                self.assertEqual(level["resized_backbuffer"], [192, 96])
                self.assertTrue(level["recreated_on_same_hwnd"])
                self.assertTrue(level["release_after_hwnd_destroyed"])

    def test_lifecycle_resize_dimensions_and_evidence_required(self):
        for key, value in (("width", 128), ("height", 128), ("source", "unresized_backbuffer")):
            records = evidence(lifecycle=True)
            next(row for row in records if row["event"] == "dimensions")[key] = value
            with self.assertRaises(runner.HarnessError):
                self.parse(records, lifecycle=True)
        records = [row for row in evidence(lifecycle=True) if row["event"] != "dimensions"]
        with self.assertRaises(runner.HarnessError):
            self.parse(records, lifecycle=True)

    def test_lifecycle_recreate_destroy_and_survivor_are_required(self):
        for name in ("lifecycle_recreated_swapchain", "lifecycle_release_after_destroy", "lifecycle_survivor_present"):
            records = [row for row in evidence(lifecycle=True) if row.get("name") != name]
            with self.assertRaises(runner.HarnessError):
                self.parse(records, lifecycle=True)

    def test_basic_and_lifecycle_modes_cannot_be_confused(self):
        with self.assertRaises(runner.HarnessError):
            self.parse(evidence(lifecycle=True))
        with self.assertRaises(runner.HarnessError):
            self.parse(evidence(), lifecycle=True)

    def test_identity_only_never_claims_rendering(self):
        for architecture in ("x64", "x86"):
            result = runner.parse_probe(encode(evidence(architecture, identity_only=True)), architecture,
                EXPECTED, identity_only=True)
            self.assertEqual(result["rendering"], "not_tested")
            self.assertNotIn("feature_levels", result)
            self.assertNotIn("selected_module_sha256", result)

    def test_identity_cannot_hide_rendering_or_missing_module(self):
        extra = evidence(identity_only=True)
        extra.insert(-1, {"event": "check", "feature_level": "10_0", "name": "hardware_device",
                          "passed": True, "hresult": "0x00000000"})
        missing = [row for row in evidence(identity_only=True) if row.get("name") != "dxgi.dll"]
        for records in (extra, missing, evidence()):
            with self.assertRaises(runner.HarnessError):
                runner.parse_probe(encode(records), "x64", EXPECTED, identity_only=True)

    def test_gated_render_requires_matching_independent_mapped_identity(self):
        mapped = {"d3d11.dll": "c" * 64, "dxgi.dll": "d" * 64, "winemetal.so": "e" * 64}
        proof = {"mapping_kind": "txt", "process_verified": True, "sha256": mapped}
        result = runner.parse_probe(encode(evidence(gated=True)), "x64", EXPECTED,
            mapped_expected=mapped, mapping_evidence=proof)
        self.assertEqual(result["selected_module_sha256"], mapped)
        self.assertEqual(result["logical_module_sha256"], EXPECTED)
        for bad in (None, {}, {**proof, "mapping_kind": "open"}, {**proof, "process_verified": False},
                    {**proof, "sha256": EXPECTED}):
            with self.assertRaises(runner.HarnessError):
                runner.parse_probe(encode(evidence(gated=True)), "x64", EXPECTED,
                    mapped_expected=mapped, mapping_evidence=bad)

    def test_gated_identity_requires_gate_and_remains_non_rendering(self):
        proof = {"mapping_kind": "txt", "process_verified": True, "sha256": EXPECTED}
        records = evidence(identity_only=True, gated=True)
        result = runner.parse_probe(encode(records), "x64", EXPECTED, identity_only=True,
            mapped_expected=EXPECTED, mapping_evidence=proof)
        self.assertEqual(result["selected_module_sha256"], EXPECTED)
        self.assertEqual(result["rendering"], "not_tested")
        records = [row for row in records if row.get("name") != "identity_gate"]
        with self.assertRaises(runner.HarnessError):
            runner.parse_probe(encode(records), "x64", EXPECTED, identity_only=True,
                mapped_expected=EXPECTED, mapping_evidence=proof)

    def test_identity_and_rendering_modes_cannot_be_confused(self):
        with self.assertRaises(runner.HarnessError):
            self.parse(evidence(identity_only=True))
        with self.assertRaises(runner.HarnessError):
            runner.parse_probe(encode(evidence(identity_only=True)), "x64", EXPECTED, identity_only=True, lifecycle=True)

    def test_lsof_only_accepts_mapped_images_for_one_exact_process(self):
        data = b"p123\0\nftxt\0n/owned/d3d11.dll\0\nf4r\0n/owned/dxgi.dll\0\n"
        self.assertEqual(runner.mapped_text_paths(data, 123), {Path("/owned/d3d11.dll")})
        for bad in (data.replace(b"p123", b"p124"), b"ftxt\0n/owned/d3d11.dll\0", data + b"p123\0"):
            with self.assertRaises(runner.HarnessError):
                runner.mapped_text_paths(bad, 123)

    def test_exact_fixture_owner_requires_numeric_current_uid(self):
        good = b"p123\0u" + str(os.getuid()).encode() + b"\0\n"
        self.assertEqual(runner.mapped_fixture_owners(good), {123})
        for bad in (b"p123\0", b"p123\0uinvalid\0", b"p123\0u" + str(os.getuid() + 1).encode() + b"\0"):
            with self.assertRaises(runner.HarnessError):
                runner.mapped_fixture_owners(bad)


class OwnershipTests(unittest.TestCase):
    def setUp(self):
        build = runner.PROJECT / ".build"
        build.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="dxmt-runner-unit-", dir=build)
        self.project = Path(self.temporary.name)
        self.build = self.project / ".build"
        bin_directory = self.build / "fixture-runtime/bin"
        bin_directory.mkdir(parents=True)
        self.core = self.build / "fixture-core"
        for path in (self.core, *(bin_directory / name for name in ("wine", "wineboot", "wineserver"))):
            path.write_text("#!/bin/sh\nexit 0\n")
            path.chmod(0o700)
        self.session = runner.DXMTSession.setup(self.project, self.core, bin_directory / "wine")

    def tearDown(self):
        # No fixture executable was started: every process call below is mocked.
        self.temporary.cleanup()

    def test_environment_ignores_caller_credentials_and_caches(self):
        with mock.patch.dict(os.environ, {"UNRELATED_AUTH": "synthetic", "WINEPREFIX": "unselected"}):
            environment = self.session.environment()
        self.assertNotIn("UNRELATED_AUTH", environment)
        self.assertNotIn("WINEPREFIX", environment)
        self.assertEqual(environment["WINEDLLOVERRIDES"], runner.OVERRIDES)
        for name in ("HOME", "CASKRUN_HOME", "TMPDIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME",
                     "XDG_CACHE_HOME", "CFFIXED_USER_HOME", "DARWIN_USER_CACHE_DIR", "DXMT_SHADER_CACHE_PATH"):
            self.assertTrue(Path(environment[name]).is_relative_to(self.session.root), name)
        self.assertEqual(environment["DXMT_LOG_PATH"], "none")

    def test_receipt_refuses_unknown_layout_and_external_backend_roots(self):
        runtime = self.build / "fixture-runtime"
        receipt = {"format": "caskrun-runtime-components", "version": 1, "status": "experimental",
            "game_validated": False, "components": [{"name": "DXMT", "status": "built", "revision": "a" * 40}],
            "layout": "switchable-v1", "default_backend": "wined3d",
            "backend_roots": {"wined3d": "runtime", "dxmt": "components/dxmt/runtime"}}
        for changed in ({**receipt, "layout": "unreviewed"},
                        {**receipt, "backend_roots": {"wined3d": "runtime", "dxmt": "../external"}},
                        {**receipt, "artifacts": []}, {**receipt, "components": ["DXMT"]}, []):
            (runtime / "components.json").write_text(json.dumps(changed))
            with self.assertRaises(runner.HarnessError):
                runner.candidate_receipt(runtime, self.build)

    def mapped_fixture(self):
        fixture = self.session.root / "original-probe.exe"
        fixture.write_bytes(b"original fixture")
        artifacts = {name: self.session.root / name for name in EXPECTED}
        for name, path in artifacts.items():
            path.write_bytes(name.encode())
        wanted = {name: runner.harness.digest(path) for name, path in artifacts.items()}
        self.session.manifest["tracked"] = {"123": "fixture birth"}
        table = {123: {"birth": "fixture birth"}}
        output = b"p123\0\n" + b"".join(b"ftxt\0n" + os.fsencode(path) + b"\0\n"
            for path in (fixture, *artifacts.values()))
        return fixture, artifacts, wanted, table, output

    def discovery(self):
        return subprocess.CompletedProcess([], 0, stdout=b"p123\0u" + str(os.getuid()).encode() + b"\0\n", stderr=b"")

    def test_mapped_identity_requires_one_owned_live_probe_and_receipt_files(self):
        fixture, artifacts, wanted, table, output = self.mapped_fixture()
        with mock.patch.object(runner.harness, "process_table", return_value=table):
            with mock.patch.object(runner.subprocess, "run", side_effect=[self.discovery(), subprocess.CompletedProcess([], 0,
                    stdout=output, stderr=b"")]) as run:
                result = runner.verify_live_mappings(self.session, fixture, artifacts, wanted)
        self.assertEqual(result, {"mapping_kind": "txt", "process_verified": True, "sha256": wanted})
        self.assertEqual(run.call_args.args[0], ["/usr/sbin/lsof", "-nP", "-a", "-p", "123", "-d", "txt", "-F0pfn"])
        self.assertNotIn("path", json.dumps(result))

    def test_detached_wow64_child_is_identified_by_exact_mapped_owned_fixture(self):
        fixture, artifacts, wanted, table, output = self.mapped_fixture()
        self.session.manifest["tracked"] = {}
        with mock.patch.object(runner.harness, "process_table", return_value=table):
            with mock.patch.object(runner.subprocess, "run", side_effect=[self.discovery(), subprocess.CompletedProcess([], 0,
                    stdout=output, stderr=b"")]):
                result = runner.verify_live_mappings(self.session, fixture, artifacts, wanted)
        self.assertTrue(result["process_verified"])
        self.assertEqual(self.session._mapped_probe_processes, {"123": "fixture birth"})

    def test_open_files_logical_placeholders_and_other_processes_do_not_prove_mapping(self):
        fixture, artifacts, wanted, table, output = self.mapped_fixture()
        missing_fixture = output.replace(os.fsencode(fixture), os.fsencode(self.session.root / "another.exe"))
        logical_copy = output.replace(os.fsencode(artifacts["dxgi.dll"]), os.fsencode(self.session.root / "prefix-dxgi.dll"))
        ambiguous = output + b"ftxt\0n" + os.fsencode(self.session.root / "prefix" / "dxgi.dll") + b"\0\n"
        for data in (missing_fixture, logical_copy, ambiguous, output.replace(b"ftxt", b"f4r")):
            with mock.patch.object(runner.harness, "process_table", return_value=table):
                with mock.patch.object(runner.subprocess, "run", side_effect=[self.discovery(), subprocess.CompletedProcess([], 0,
                        stdout=data, stderr=b"")]):
                    with self.assertRaises(runner.HarnessError):
                        runner.verify_live_mappings(self.session, fixture, artifacts, wanted)

    def test_mapped_identity_refuses_changed_hash_or_reused_pid(self):
        fixture, artifacts, wanted, table, output = self.mapped_fixture()
        for expected, tables in (({**wanted, "d3d11.dll": "f" * 64}, [table]),
                                 (wanted, [table, {123: {"birth": "changed"}}])):
            with mock.patch.object(runner.harness, "process_table", side_effect=tables):
                with mock.patch.object(runner.subprocess, "run", side_effect=[self.discovery(), subprocess.CompletedProcess([], 0,
                        stdout=output, stderr=b"")]):
                    with self.assertRaises(runner.HarnessError):
                        runner.verify_live_mappings(self.session, fixture, artifacts, expected)

    def test_busy_switch_requires_running_guard_and_never_uses_stdin_cancellation(self):
        bottle = "11111111-1111-4111-8111-111111111111"
        directory = self.session.storage / "bottles" / bottle
        directory.mkdir(parents=True)
        preference = directory / "graphics.json"
        preference.write_bytes(b"original preference")
        denied = subprocess.CompletedProcess([], 1, stdout=b"", stderr=b"Close Windows apps in this bottle before changing it")
        with mock.patch.object(runner.subprocess, "run", return_value=denied) as run:
            self.session.verify_busy_switch_refused(bottle)
        self.assertNotIn("CASKRUN_CANCEL_STDIN", run.call_args.kwargs["env"])
        self.assertEqual(preference.read_bytes(), b"original preference")
        for result in (subprocess.CompletedProcess([], 0, stdout=b"{}", stderr=b""),
                       subprocess.CompletedProcess([], 130, stdout=b"", stderr=b"Canceled")):
            with mock.patch.object(runner.subprocess, "run", return_value=result):
                with self.assertRaises(runner.HarnessError):
                    self.session.verify_busy_switch_refused(bottle)

    def test_busy_refusal_detects_preference_mutation(self):
        bottle = "11111111-1111-4111-8111-111111111111"
        directory = self.session.storage / "bottles" / bottle
        directory.mkdir(parents=True)
        preference = directory / "graphics.json"
        preference.write_bytes(b"original preference")
        def changed(*args, **kwargs):
            preference.write_bytes(b"incorrect change")
            return subprocess.CompletedProcess([], 1, stdout=b"", stderr=b"Close Windows apps in this bottle before changing it")
        with mock.patch.object(runner.subprocess, "run", side_effect=changed):
            with self.assertRaises(runner.HarnessError):
                self.session.verify_busy_switch_refused(bottle)

    def test_stop_uses_only_owned_prefix_kill_then_wait(self):
        bottle = "11111111-1111-4111-8111-111111111111"
        prefix = self.session.storage / "bottles" / bottle / "prefix"
        prefix.mkdir(parents=True)
        self.session.manifest["dxmt_bottles"] = [bottle]
        with mock.patch.object(runner.subprocess, "run", side_effect=[
                subprocess.TimeoutExpired("owned-waiter", 1),
                subprocess.CompletedProcess([], 0, stdout=b"", stderr=b""),
                subprocess.CompletedProcess([], 0, stdout=b"", stderr=b"")]) as run:
            with mock.patch.object(self.session, "assert_idle") as idle:
                self.session.stop_owned_prefixes()
        self.assertEqual([call.args[0][-1] for call in run.call_args_list], ["-w", "-k", "-w"])
        for call in run.call_args_list:
            self.assertEqual(call.args[0][0], str(self.session.wine.parent / "wineserver"))
            self.assertEqual(call.kwargs["env"]["WINEPREFIX"], str(prefix))
            self.assertEqual(call.kwargs["env"]["HOME"], str(self.session.host))
        idle.assert_called_once_with(timeout=15)

    def test_repeated_idle_cleanup_never_signals(self):
        bottle = "11111111-1111-4111-8111-111111111111"
        (self.session.storage / "bottles" / bottle / "prefix").mkdir(parents=True)
        self.session.manifest["dxmt_bottles"] = [bottle]
        with mock.patch.object(runner.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            with mock.patch.object(self.session, "assert_idle") as idle:
                self.session.stop_owned_prefixes()
                self.session.stop_owned_prefixes()
        self.assertEqual([call.args[0][-1] for call in run.call_args_list], ["-w", "-w"])
        self.assertEqual(idle.call_count, 2)

    def test_race_to_idle_requires_post_wait_and_owner_checks(self):
        bottle = "11111111-1111-4111-8111-111111111111"
        (self.session.storage / "bottles" / bottle / "prefix").mkdir(parents=True)
        self.session.manifest["dxmt_bottles"] = [bottle]
        for final_code in (0, 1):
            with mock.patch.object(runner.subprocess, "run", side_effect=[
                    subprocess.TimeoutExpired("owned-waiter", 1),
                    subprocess.CompletedProcess([], 1, stdout=b"", stderr=b""),
                    subprocess.CompletedProcess([], final_code, stdout=b"", stderr=b"")]):
                with mock.patch.object(self.session, "assert_idle") as idle:
                    if final_code == 0:
                        self.session.stop_owned_prefixes()
                        idle.assert_called_once_with(timeout=15)
                    else:
                        with self.assertRaises(runner.HarnessError):
                            self.session.stop_owned_prefixes()
                        idle.assert_not_called()

    def test_bad_owner_prevents_all_server_signals(self):
        self.session.manifest["owner_token"] = "changed"
        with mock.patch.object(runner.subprocess, "run") as run:
            with self.assertRaises(runner.HarnessError):
                self.session.stop_owned_prefixes()
        run.assert_not_called()

    def test_stop_failure_prevents_idle_or_removal(self):
        bottle = "11111111-1111-4111-8111-111111111111"
        (self.session.storage / "bottles" / bottle / "prefix").mkdir(parents=True)
        self.session.manifest["dxmt_bottles"] = [bottle]
        with mock.patch.object(runner.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)):
            with mock.patch.object(self.session, "assert_idle") as idle:
                with self.assertRaises(runner.HarnessError):
                    self.session.stop_owned_prefixes()
        idle.assert_not_called()
        self.assertTrue(self.session.root.is_dir())


if __name__ == "__main__":
    unittest.main()
