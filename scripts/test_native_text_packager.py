#!/usr/bin/env python3
"""Focused regression checks for the isolated dual-view driver packager."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
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
        self.assertEqual(len(inputs), 8)
        for prefix, variant in (("runtime", "base-build"),
                                ("components/dxmt/runtime", "dxmt-build")):
            for relative in BUILD.DRIVERS:
                self.assertIn(prefix + "/" + relative, inputs)
                self.assertIn(variant, inputs[prefix + "/" + relative].parts)
            loader = prefix + "/lib/wine/x86_64-unix/ntdll.so"
            self.assertEqual(inputs[loader],
                             Path("fixture") / variant / "build/dlls/ntdll/ntdll.so")

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
                            BUILD.verify_drivers({relative: Path("driver.so")}, Path("reviewed-base"))
                        else:
                            with self.assertRaises(BUILD.PACKAGE.PackageError):
                                BUILD.verify_drivers({relative: Path("driver.so")}, Path("reviewed-base"))

    def test_rebuilt_ntdll_preserves_real_macho_exports_and_architecture(self):
        # These original tiny libraries exercise the actual nm/lipo path rather
        # than manufacturing symbol output that repeats the production rule.
        # Loader instructions may change; the established public ABI may not.
        build_root = BUILD.REPO / ".build"
        build_root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".native-loader-abi-", dir=build_root) as temporary:
            root = Path(temporary)
            variants = {
                "reviewed": ("x86_64", "int loader_entry(void) { return 1; }\n"
                             "int loader_version(void) { return 11; }\n"),
                "rebuilt": ("x86_64", "int loader_entry(void) { return 2; }\n"
                            "int loader_version(void) { return 11; }\n"),
                "extra": ("x86_64", "int loader_entry(void) { return 2; }\n"
                          "int loader_version(void) { return 11; }\n"
                          "int unreviewed_entry(void) { return 3; }\n"),
                "missing": ("x86_64", "int loader_entry(void) { return 2; }\n"),
                "wrong-architecture": ("arm64", "int loader_entry(void) { return 2; }\n"
                                       "int loader_version(void) { return 11; }\n"),
            }
            libraries = {}
            for name, (architecture, code) in variants.items():
                source = root / (name + ".c")
                library = root / (name + ".so")
                source.write_text(code)
                subprocess.run(["/usr/bin/clang", "-target", architecture + "-apple-macos11.0",
                                "-dynamiclib", "-Wall", "-Wextra", "-Werror", str(source),
                                "-o", str(library)], check=True, capture_output=True)
                libraries[name] = library
            self.assertNotEqual(libraries["reviewed"].read_bytes(), libraries["rebuilt"].read_bytes())
            base = root / "reviewed-base"
            for prefix in ("runtime", "components/dxmt/runtime"):
                relative = prefix + "/lib/wine/x86_64-unix/ntdll.so"
                previous = base / relative
                previous.parent.mkdir(parents=True)
                shutil.copy2(libraries["reviewed"], previous)
                for name in ("rebuilt", "extra", "missing", "wrong-architecture"):
                    with self.subTest(view=prefix, replacement=name):
                        if name == "rebuilt":
                            BUILD.verify_drivers({relative: libraries[name]}, base)
                        else:
                            with self.assertRaises(BUILD.PACKAGE.PackageError):
                                BUILD.verify_drivers({relative: libraries[name]}, base)

    def test_pe_halves_must_match_their_architecture(self):
        for architecture, machine in (("i386", 0x14c), ("x86_64", 0x8664)):
            relative = "runtime/lib/wine/" + architecture + "-windows/winemac.drv"
            for reported in (0x14c, 0x8664, 0xaa64):
                with self.subTest(architecture=architecture, machine=reported), \
                        patch.object(BUILD.PACKAGE, "pe_machine", return_value=reported):
                    if reported == machine:
                        BUILD.verify_drivers({relative: Path("driver.drv")}, Path("reviewed-base"))
                    else:
                        with self.assertRaises(BUILD.PACKAGE.PackageError):
                            BUILD.verify_drivers({relative: Path("driver.drv")}, Path("reviewed-base"))

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
