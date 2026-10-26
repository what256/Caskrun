#!/usr/bin/env python3
"""Boundary tests for engine evidence and Wine builtin packaging; no Wine runs."""
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "engine_package", Path(__file__).with_name("package-wine-engine-components-dev.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def pe_bytes(machine=0x8664, signed=False):
    data = bytearray(512)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<H", data, 132, machine)
    struct.pack_into("<H", data, 148, 240 if machine == 0x8664 else 224)
    optional = 152
    directory = 112 if machine == 0x8664 else 96
    struct.pack_into("<H", data, optional, 0x20b if machine == 0x8664 else 0x10b)
    struct.pack_into("<I", data, optional + directory - 4, 16)
    if signed:
        struct.pack_into("<II", data, optional + directory + 4 * 8, 450, 16)
    data[400:412] = b"code-payload"
    return bytes(data)


class PackageBoundaries(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=MODULE.REPO / ".build", prefix="engine-package-test-")
        self.root = Path(self.temporary.name)
        self.tool = self.root / "unused-winebuild"
        self.tool.write_bytes(b"unused fixture")
        self.image = self.root / "fixture.dll"

    def tearDown(self):
        self.temporary.cleanup()

    def test_gpu_summary_pass_flags_without_bound_images_are_refused(self):
        summary = self.root / "summary.json"
        summary.write_text(json.dumps({"schema": 1, "implementation": "caskrun-dxvk-hybrid-v1",
            "loading_model": "wine-builtin-runtime-view-v1",
            **{key: True for key in ("runtime_modules_unchanged", "native_images_unchanged",
                "base_wine_d3d8_retained", "runtime_view_validation_passed",
                "prefix_graphics_dlls_unchanged", "d3d9_validation_passed",
                "d3d11_validation_passed", "app_directories_contain_only_original_probes")},
            "selected_runtime_module_sha256": {}, "native_sha256": {}}))
        with self.assertRaisesRegex(MODULE.P.PackageError, "bind the reviewed"):
            MODULE.dxvk_render_proof(summary, {summary}, {}, self.root)

    def test_component_rejects_single_revision_in_place_of_hybrid_source_pins(self):
        receipt = self.root / "component.json"
        receipt.write_text(json.dumps({"schema": 1, "component": "caskrun-dxvk-hybrid-v1",
            "revision": MODULE.DXVK_REVISION, "moltenvk_version": "1.4.2"}))
        with self.assertRaisesRegex(MODULE.P.PackageError, "source pins"):
            MODULE.dxvk_plan(receipt, self.root)

    def test_builtin_conversion_is_exact_for_both_windows_architectures(self):
        def mark(*args):
            value = bytearray(self.image.read_bytes())
            value[64:96] = MODULE.BUILTIN_MARKER
            self.image.write_bytes(value)
        for machine in (0x14c, 0x8664):
            before = pe_bytes(machine)
            self.image.write_bytes(before)
            with patch.object(MODULE.P, "run", side_effect=mark):
                record = MODULE.mark_wine_builtin(self.image, self.tool, machine)
            self.assertEqual(record["original_sha256"], hashlib.sha256(before).hexdigest())
            self.assertEqual(record["runtime_sha256"], hashlib.sha256(self.image.read_bytes()).hexdigest())
            self.assertEqual(self.image.read_bytes()[400:412], b"code-payload")

    def test_signed_pe_is_refused_before_any_tool_execution(self):
        before = pe_bytes(signed=True)
        self.image.write_bytes(before)
        with patch.object(MODULE.P, "run") as run:
            with self.assertRaisesRegex(MODULE.P.PackageError, "Authenticode"):
                MODULE.mark_wine_builtin(self.image, self.tool, 0x8664)
            run.assert_not_called()
        self.assertEqual(self.image.read_bytes(), before)

    def test_wrong_architecture_and_overlapping_headers_are_refused(self):
        for value in (pe_bytes(0x14c), b"MZ" + b"\0" * 510):
            self.image.write_bytes(value)
            with patch.object(MODULE.P, "run") as run:
                with self.assertRaises(MODULE.P.PackageError):
                    MODULE.mark_wine_builtin(self.image, self.tool, 0x8664)
                run.assert_not_called()

    def test_conversion_that_changes_code_or_flags_is_refused(self):
        before = pe_bytes()
        self.image.write_bytes(before)
        def corrupt(*args):
            value = bytearray(before)
            value[64:96] = MODULE.BUILTIN_MARKER
            value[400] ^= 1
            self.image.write_bytes(value)
        with patch.object(MODULE.P, "run", side_effect=corrupt):
            with self.assertRaisesRegex(MODULE.P.PackageError, "more than"):
                MODULE.mark_wine_builtin(self.image, self.tool, 0x8664)

    def proof(self):
        runs = []
        for backend in ("wined3d", "dxmt"):
            log = self.root / (backend + ".log")
            log.write_text("".join(f"{backend} WINEMSYNC={mode} {arch}: RESULT checks=37 failures=0\n"
                                   for mode in (0, 1) for arch in ("x86_64", "i686"))
                           + "MSync regression PASSED\n")
            runs.append({"backend": backend, "log": log.name, "sha256": MODULE.P.sha256(log)})
        value = {"schema": 1, "kind": "caskrun-engine-api-validation",
                 "base_identity": "sha256:" + MODULE.BASE_IDENTITY, "inputs": {"fixture": "abc"},
                 "msync_patch_sha256": MODULE.BUILD.MSYNC_SHA256,
                 "portability_patch_sha256": MODULE.BUILD.PORTABILITY_SHA256, "runs": runs}
        receipt = self.root / "proof.json"
        receipt.write_text(json.dumps(value))
        return receipt, value

    def test_validation_accepts_complete_bound_matrix_and_omits_log_paths(self):
        receipt, _ = self.proof()
        result = MODULE.synchronization_proof(receipt, {"fixture": "abc"})
        self.assertEqual(len(result["runs"]), 2)
        self.assertNotIn(str(self.root), json.dumps(result))
        self.assertNotIn(".log", json.dumps(result))

    def test_validation_refuses_changed_inputs_changed_logs_and_missing_views(self):
        receipt, value = self.proof()
        with self.assertRaises(MODULE.P.PackageError):
            MODULE.synchronization_proof(receipt, {"fixture": "different"})
        (self.root / "dxmt.log").write_text("MSync regression PASSED\n")
        with self.assertRaises(MODULE.P.PackageError):
            MODULE.synchronization_proof(receipt, {"fixture": "abc"})
        value["runs"] = value["runs"][:1]
        receipt.write_text(json.dumps(value))
        with self.assertRaises(MODULE.P.PackageError):
            MODULE.synchronization_proof(receipt, {"fixture": "abc"})

    def test_validation_refuses_path_escape_and_incomplete_or_failing_cells(self):
        receipt, value = self.proof()
        value["runs"][0]["log"] = "../outside.log"
        receipt.write_text(json.dumps(value))
        with self.assertRaises(MODULE.P.PackageError):
            MODULE.synchronization_proof(receipt, {"fixture": "abc"})
        receipt, value = self.proof()
        log = self.root / "dxmt.log"
        log.write_text(log.read_text().replace("checks=37 failures=0", "checks=37 failures=1", 1))
        value["runs"][1]["sha256"] = MODULE.P.sha256(log)
        receipt.write_text(json.dumps(value))
        with self.assertRaises(MODULE.P.PackageError):
            MODULE.synchronization_proof(receipt, {"fixture": "abc"})

    def test_secondary_server_uses_one_canonical_internal_target(self):
        base = self.root / "candidate/runtime/bin"
        secondary = self.root / "candidate/components/dxmt/runtime/bin"
        base.mkdir(parents=True)
        secondary.mkdir(parents=True)
        (base / "wineserver").write_bytes(b"new matching server")
        (secondary / "wineserver").write_bytes(b"old server")
        MODULE.shared_server_links(self.root / "candidate", ("components/dxmt/runtime",))
        self.assertTrue((secondary / "wineserver").is_symlink())
        self.assertEqual((secondary / "wineserver").resolve(), base / "wineserver")


if __name__ == "__main__":
    unittest.main()
