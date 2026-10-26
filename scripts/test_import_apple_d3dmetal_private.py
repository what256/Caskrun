#!/usr/bin/env python3
"""Original-component import boundary tests; fixtures contain no Apple code."""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "apple_component_import", Path(__file__).with_name("import-apple-d3dmetal-private.py"))
assert SPEC and SPEC.loader
IMPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMPORT)


class PrivateImportTests(unittest.TestCase):
    def setUp(self):
        build = IMPORT.REPO / ".build"
        build.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="d3dmetal-import-unit-", dir=build)
        self.base = Path(self.temporary.name)
        self.root = self.base / "evaluation"
        self.root.mkdir()
        self.lib = self.root / "redist/lib"
        self.windows = self.lib / "wine/x86_64-windows"
        self.unix = self.lib / "wine/x86_64-unix"
        self.windows.mkdir(parents=True)
        self.unix.mkdir(parents=True)
        self.framework = self.lib / "external/D3DMetal.framework"
        resources = self.framework / "Versions/A/Resources"
        resources.mkdir(parents=True)
        (self.framework / "Versions/A/D3DMetal").write_bytes(b"synthetic-framework-not-Apple")
        (self.framework / "Versions/Current").symlink_to("A")
        (self.framework / "D3DMetal").symlink_to("Versions/Current/D3DMetal")
        (resources / "LICENSE").write_text("Synthetic fixture notice. No Apple code.")
        (self.lib / "external/libd3dshared.dylib").write_bytes(b"synthetic-bridge-not-Apple")
        for name in IMPORT.DOCS:
            (self.root / name).write_text("Synthetic " + name + ". No legal agreement.")
        for name in IMPORT.CORE_DLLS:
            self.pe(self.windows / (name + ".dll"), 0x8664)
            (self.unix / (name + ".so")).symlink_to("../../external/libd3dshared.dylib")
        self.image = self.base / "official-fixture.dmg"
        self.image.write_bytes(b"synthetic-dmg-no-runtime")
        self.args = argparse.Namespace(
            evaluation_root=self.root, image=self.image,
            source_url="https://download.developer.apple.com/Developer_Tools/Fixture/Fixture.dmg",
            expected_image_sha256=IMPORT.sha256(self.image),
            license_reviewed_sha256=None, output=self.base / "component")
        self.signature = patch.object(IMPORT, "apple_framework_signature",
                                      return_value={"verified": True, "fixture": True})
        self.arches = patch.object(IMPORT, "macho_arches", return_value=["x86_64"])
        self.signature.start()
        self.arches.start()

    def tearDown(self):
        self.arches.stop()
        self.signature.stop()
        self.temporary.cleanup()

    @staticmethod
    def pe(path, machine):
        data = bytearray(128)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<H", data, 68, machine)
        path.write_bytes(data)

    def inspect(self):
        return IMPORT.inspect(self.args)

    def test_inspection_does_not_enable_backend_or_claim_rendering(self):
        receipt, _ = self.inspect()
        self.assertFalse(receipt["backend_available"])
        self.assertFalse(receipt["wine_host_abi_verified"])
        self.assertFalse(receipt["d3d11_device_render_verified"])
        self.assertFalse(receipt["d3d12_device_render_verified"])
        self.assertFalse(receipt["supports_x86_windows_apps"])
        self.assertFalse(receipt["distribution"]["public_release_approved"])
        self.assertNotIn(str(self.base), json.dumps(receipt))
        self.assertFalse(self.args.output.exists())

    def test_refuses_changed_image_before_component_use(self):
        self.image.write_bytes(b"different")
        with self.assertRaisesRegex(IMPORT.ImportError, "image hash"):
            self.inspect()

    def test_refuses_unreviewed_license_without_creating_output(self):
        receipt, selected = self.inspect()
        with self.assertRaisesRegex(IMPORT.ImportError, "person must review"):
            IMPORT.stage(self.args, receipt, selected)
        self.assertFalse(self.args.output.exists())

    def test_exact_license_hash_is_required(self):
        receipt, selected = self.inspect()
        self.args.license_reviewed_sha256 = "0" * 64
        with self.assertRaisesRegex(IMPORT.ImportError, "person must review"):
            IMPORT.stage(self.args, receipt, selected)

    def test_refuses_existing_destination(self):
        receipt, selected = self.inspect()
        self.args.license_reviewed_sha256 = receipt["license_sha256"]
        self.args.output.mkdir()
        marker = self.args.output / "keep"
        marker.write_text("untouched")
        with self.assertRaisesRegex(IMPORT.ImportError, "already exists"):
            IMPORT.stage(self.args, receipt, selected)
        self.assertEqual(marker.read_text(), "untouched")

    def test_refuses_missing_notices(self):
        (self.root / "Acknowledgements.rtf").unlink()
        with self.assertRaisesRegex(IMPORT.ImportError, "Acknowledgements"):
            self.inspect()

    def test_refuses_app_bundle_source(self):
        renamed = self.root.with_suffix(".app")
        self.root.rename(renamed)
        self.args.evaluation_root = renamed
        with self.assertRaisesRegex(IMPORT.ImportError, "installed application"):
            self.inspect()

    def test_refuses_32bit_pe_in_x64_payload(self):
        self.pe(self.windows / "d3d12.dll", 0x14C)
        with self.assertRaisesRegex(IMPORT.ImportError, "x86_64"):
            self.inspect()

    def test_refuses_external_framework_link(self):
        (self.framework / "outside").symlink_to(self.image)
        with self.assertRaisesRegex(IMPORT.ImportError, "external/absolute"):
            self.inspect()

    def test_refuses_broken_framework_link(self):
        (self.framework / "missing").symlink_to("absent")
        with self.assertRaisesRegex(IMPORT.ImportError, "broken or looping"):
            self.inspect()

    def test_refuses_retargeted_unix_bridge(self):
        bridge = self.unix / "d3d12.so"
        bridge.unlink()
        bridge.symlink_to("../../external/D3DMetal.framework/Versions/A/D3DMetal")
        with self.assertRaisesRegex(IMPORT.ImportError, "unreviewed link"):
            self.inspect()

    def test_refuses_symlinked_redist_directory(self):
        redist = self.root / "redist"
        renamed = self.base / "elsewhere"
        redist.rename(renamed)
        redist.symlink_to(renamed)
        with self.assertRaisesRegex(IMPORT.ImportError, "symlink parent"):
            self.inspect()

    def test_refuses_mirror_source_url(self):
        self.args.source_url = "https://example.invalid/Apple/Fixture.dmg"
        with self.assertRaisesRegex(IMPORT.ImportError, "official Apple"):
            self.inspect()

    def test_stages_only_component_and_preserves_framework_links(self):
        (self.root / "wine-engine").write_text("must never be copied")
        receipt, selected = self.inspect()
        self.args.license_reviewed_sha256 = receipt["license_sha256"]
        IMPORT.stage(self.args, receipt, selected)
        staged = self.args.output
        self.assertFalse((staged / "wine-engine").exists())
        self.assertTrue((staged / "lib/external/D3DMetal.framework/Versions/Current").is_symlink())
        self.assertEqual((staged / "lib/wine/x86_64-unix/d3d11.so").readlink(),
                         Path("../../external/libd3dshared.dylib"))
        recorded = json.loads((staged / "component-receipt.json").read_text())
        self.assertEqual(recorded["license_reviewed_sha256"], receipt["license_sha256"])
        self.assertFalse(recorded["backend_available"])
        for entry in recorded["files"]:
            if entry["type"] == "file":
                self.assertEqual(IMPORT.sha256(staged / entry["path"]), entry["sha256"])


if __name__ == "__main__":
    unittest.main()
