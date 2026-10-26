#!/usr/bin/env python3
"""Boundary checks for the isolated DXMT D3D12 source builder."""

import importlib.util
import io
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("dxmt_d3d12_builder", PROJECT / "scripts/build-dxmt-d3d12-dev.py")
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class BuilderBoundaries(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(prefix="d3d12-boundary-", dir=PROJECT / ".build")
        self.root = Path(self.fixture.name)
        self.work = self.root / "work"
        self.work.mkdir()
        self.sentinel = self.root / "sentinel"
        self.sentinel.write_text("preserved")

    def tearDown(self):
        self.assertEqual(self.sentinel.read_text(), "preserved")
        self.fixture.cleanup()

    def test_regular_tree_is_accepted(self):
        (self.work / "build").mkdir()
        (self.work / "build/candidate.dll").write_bytes(b"test-only")
        builder.validate_work_tree(self.work)

    def test_output_directory_link_is_refused(self):
        (self.work / "build").symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(SystemExit):
            builder.validate_work_tree(self.work)

    def test_output_file_link_is_refused(self):
        (self.work / "candidate.dll").symlink_to(self.sentinel)
        with self.assertRaises(SystemExit):
            builder.validate_work_tree(self.work)

    def test_broken_output_link_is_refused(self):
        (self.work / "candidate.dll").symlink_to(self.root / "absent")
        with self.assertRaises(SystemExit):
            builder.validate_work_tree(self.work)

    def test_output_hardlink_is_refused(self):
        os.link(self.sentinel, self.work / "candidate.dll")
        with self.assertRaises(SystemExit):
            builder.validate_work_tree(self.work)

    def test_special_output_file_is_refused(self):
        os.mkfifo(self.work / "candidate.dll")
        with self.assertRaises(SystemExit):
            builder.validate_work_tree(self.work)

    def archive(self, name, kind=tarfile.REGTYPE):
        archive = self.root / "source.tar"
        with tarfile.open(archive, "w") as stream:
            entry = tarfile.TarInfo(name)
            entry.type = kind
            if kind == tarfile.REGTYPE:
                entry.size = 3
                stream.addfile(entry, io.BytesIO(b"api"))
            else:
                entry.linkname = "../sentinel"
                stream.addfile(entry)
        return archive

    def test_relative_regular_source_is_accepted(self):
        builder.unpack(self.archive("src/device.cpp"), self.work)
        self.assertEqual((self.work / "src/device.cpp").read_bytes(), b"api")

    def test_parent_escape_in_archive_is_refused(self):
        with self.assertRaises(SystemExit):
            builder.unpack(self.archive("../sentinel"), self.work)

    def test_absolute_archive_path_is_refused(self):
        with self.assertRaises(SystemExit):
            builder.unpack(self.archive(str(self.sentinel)), self.work)

    def test_archive_symlink_is_refused(self):
        with self.assertRaises(SystemExit):
            builder.unpack(self.archive("linked.cpp", tarfile.SYMTYPE), self.work)


if __name__ == "__main__":
    unittest.main()
