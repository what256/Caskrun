#!/usr/bin/env python3
"""Focused regression checks for the isolated dual-view driver packager."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "native_text_package", Path(__file__).with_name("build-wine-native-text-dev.py"))
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)


class NativeTextPackageTests(unittest.TestCase):
    def test_both_views_have_unix_and_both_windows_halves(self):
        inputs = BUILD.driver_inputs(Path("fixture"))
        self.assertEqual(len(inputs), 6)
        for prefix, variant in (("runtime", "base-build"),
                                ("components/dxmt/runtime", "dxmt-build")):
            for relative in BUILD.DRIVERS:
                self.assertIn(prefix + "/" + relative, inputs)
                self.assertIn(variant, inputs[prefix + "/" + relative].parts)

    def test_reviewed_exports_are_exact_per_view(self):
        tables = {"___wine_unix_call_funcs", "___wine_unix_call_wow64_funcs"}
        for prefix, expected in (("runtime", tables),
                                 ("components/dxmt/runtime", tables | BUILD.PACKAGE.EXPORTS)):
            relative = prefix + "/lib/wine/x86_64-unix/winemac.so"
            for symbols, accepted in ((expected, True),
                                      (expected | {"_unreviewed_export"}, False),
                                      (expected - {"___wine_unix_call_funcs"}, False)):
                with self.subTest(view=prefix, accepted=accepted, symbols=sorted(symbols)):
                    def run(tool, *args):
                        return "x86_64" if tool.endswith("lipo") else "\n".join(
                            "0000000000000000 T " + symbol for symbol in sorted(symbols))
                    with patch.object(BUILD.PACKAGE, "run", side_effect=run):
                        if accepted:
                            BUILD.verify_drivers({relative: Path("driver.so")})
                        else:
                            with self.assertRaises(BUILD.PACKAGE.PackageError):
                                BUILD.verify_drivers({relative: Path("driver.so")})

    def test_pe_halves_must_match_their_architecture(self):
        for architecture, machine in (("i386", 0x14c), ("x86_64", 0x8664)):
            relative = "runtime/lib/wine/" + architecture + "-windows/winemac.drv"
            for reported in (0x14c, 0x8664, 0xaa64):
                with self.subTest(architecture=architecture, machine=reported), \
                        patch.object(BUILD.PACKAGE, "pe_machine", return_value=reported):
                    if reported == machine:
                        BUILD.verify_drivers({relative: Path("driver.drv")})
                    else:
                        with self.assertRaises(BUILD.PACKAGE.PackageError):
                            BUILD.verify_drivers({relative: Path("driver.drv")})

    def test_preservation_snapshot_records_modes_links_and_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "directory"
            directory.mkdir(mode=0o700)
            file = directory / "file"
            file.write_bytes(b"synthetic data")
            file.chmod(0o600)
            (root / "relative-link").symlink_to("directory/file")
            before = BUILD.tree_files(root)
            directory.chmod(0o755)
            self.assertNotEqual(before, BUILD.tree_files(root))
            directory.chmod(0o700)
            file.chmod(0o644)
            self.assertNotEqual(before, BUILD.tree_files(root))
            file.chmod(0o600)
            file.write_bytes(b"different data")
            self.assertNotEqual(before, BUILD.tree_files(root))
            self.assertEqual(before["relative-link"], ("link", "directory/file"))

    def test_special_files_are_refused_without_following_them(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            os.mkfifo(root / "fifo")
            with self.assertRaises(BUILD.PACKAGE.PackageError):
                BUILD.tree_files(root)


if __name__ == "__main__":
    unittest.main()
