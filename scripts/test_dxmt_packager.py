"""Filesystem/PE tests for the isolated candidate packager; never run Wine."""
import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("package-wine-runtime-dxmt-dev.py")
SPEC = importlib.util.spec_from_file_location("dxmt_packager", SCRIPT)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class CandidateGuards(unittest.TestCase):
    def setUp(self):
        build = PACKAGE.REPO / ".build"
        build.mkdir(exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="dxmt-packager-tests-", dir=build))

    def tearDown(self):
        shutil.rmtree(self.root)

    def test_output_outside_owned_build_refused(self):
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.owned_path(PACKAGE.REPO / "candidate-runtime")
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.owned_path(PACKAGE.REPO / ".build")

    def test_symlink_parent_refused_even_when_internal(self):
        target = self.root / "actual"
        target.mkdir()
        (self.root / "alias").symlink_to("actual")
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.owned_path(self.root / "alias/output")

    def test_existing_output_refused_before_base_validation(self):
        output = self.root / "already-present"
        output.mkdir()
        (output / "preserve").write_text("untouched")
        args = argparse.Namespace(**{name: self.root / name for name in
                                     ("base", "wine_build", "dxmt_source", "dxmt_build64", "dxmt_build32")},
                                  output=output)
        with self.assertRaisesRegex(PACKAGE.PackageError, "already exists"):
            PACKAGE.validate_inputs(args)
        self.assertEqual((output / "preserve").read_text(), "untouched")

    def test_extra_edit_in_the_patched_file_is_refused(self):
        source = self.root / "source"
        file = source / "src/winemetal/unix/winemetal_unix.c"
        file.parent.mkdir(parents=True)
        file.write_text("/* independently authored exact-patch fixture */\n")
        expected = PACKAGE.sha256(file)
        with mock.patch.object(PACKAGE, "PATCHED_DXMT_SOURCES", {str(file.relative_to(source)): expected}):
            PACKAGE.require_patched_dxmt_sources(source)
            # The name/reverse-apply checks can still pass for this unrelated
            # append. It cannot be reconstructed from the recorded patch.
            with file.open("a") as stream:
                stream.write("/* additional unrecorded same-file modification */\n")
            with self.assertRaisesRegex(PACKAGE.PackageError, "exact reviewed patched input"):
                PACKAGE.require_patched_dxmt_sources(source)

    def test_identity_is_relocatable_and_tracks_content_modes_and_links(self):
        first = self.root / "first"
        first.mkdir()
        file = first / "program"
        file.write_bytes(b"independent fixture")
        file.chmod(0o755)
        (first / "alias").symlink_to("program")
        second = self.root / "second"
        shutil.copytree(first, second, symlinks=True)
        identity = PACKAGE.tree_identity(first)
        self.assertEqual(identity, PACKAGE.tree_identity(second))
        (second / "program").chmod(0o644)
        self.assertNotEqual(identity, PACKAGE.tree_identity(second))
        (second / "program").chmod(0o755)
        (second / "program").write_bytes(b"changed")
        self.assertNotEqual(identity, PACKAGE.tree_identity(second))
        (second / "program").write_bytes(file.read_bytes())
        (second / "alias").unlink()
        (second / "alias").symlink_to("./program")
        self.assertNotEqual(identity, PACKAGE.tree_identity(second))

    def test_tree_refuses_external_and_absolute_links(self):
        tree = self.root / "tree"
        tree.mkdir()
        outside = self.root / "separate"
        outside.write_text("must not be read")
        link = tree / "link"
        link.symlink_to("../separate")
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.tree_identity(tree)
        link.unlink()
        (tree / "file").write_text("local")
        link.symlink_to(tree / "file")
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.tree_identity(tree)

    def test_tree_refuses_special_files(self):
        tree = self.root / "tree"
        tree.mkdir()
        os.mkfifo(tree / "pipe")
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.tree_identity(tree)

    def pe_fixture(self, machine):
        path = self.root / "fixture.dll"
        data = bytearray(128)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<H", data, 68, machine)
        path.write_bytes(data)
        return path

    def test_pe_architectures_are_read_from_header(self):
        for machine in (0x14c, 0x8664):
            self.assertEqual(PACKAGE.pe_machine(self.pe_fixture(machine)), machine)

    def test_pe_truncated_and_out_of_file_headers_refused(self):
        path = self.pe_fixture(0x8664)
        data = bytearray(path.read_bytes())
        struct.pack_into("<I", data, 60, len(data) - 1)
        path.write_bytes(data)
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.pe_machine(path)
        path.write_bytes(b"MZ")
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.pe_machine(path)

    def test_dependency_resolution_stays_in_candidate(self):
        library = self.root / "runtime/lib/wine/x86_64-unix"
        library.mkdir(parents=True)
        image = library / "winemetal.so"
        image.write_bytes(b"fixture")
        target = library / "ntdll.so"
        target.write_bytes(b"fixture")
        self.assertEqual(PACKAGE.package_dependency(self.root, image, "@rpath/ntdll.so"), target)
        self.assertEqual(PACKAGE.package_dependency(self.root, image, "@loader_path/ntdll.so"), target)
        self.assertIsNone(PACKAGE.package_dependency(self.root, image, "/usr/lib/libSystem.B.dylib"))
        with self.assertRaises(PACKAGE.PackageError):
            PACKAGE.package_dependency(self.root, image, str(target))
        with self.assertRaises((PACKAGE.PackageError, FileNotFoundError)):
            PACKAGE.package_dependency(self.root, image, "@loader_path/../../../../outside.so")

    def test_publication_never_replaces_existing_empty_directory(self):
        source = self.root / "source"
        source.mkdir()
        (source / "keep").write_text("candidate")
        output = self.root / "output"
        output.mkdir()
        with self.assertRaises(OSError):
            PACKAGE.publish_exclusively(source, output)
        self.assertEqual((source / "keep").read_text(), "candidate")
        self.assertEqual(list(output.iterdir()), [])
        output.rmdir()
        PACKAGE.publish_exclusively(source, output)
        self.assertEqual((output / "keep").read_text(), "candidate")
        self.assertFalse(source.exists())


if __name__ == "__main__":
    unittest.main()
