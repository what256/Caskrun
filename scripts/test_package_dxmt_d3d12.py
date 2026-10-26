#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Refusal boundaries for D3D12 packaging; never run Wine or create a package."""
import argparse
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("d3d12_packager", Path(__file__).with_name("package-dxmt-d3d12-dev.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class RefusalBoundaries(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(prefix="d3d12-package-boundary-", dir=package.REPO / ".build")
        self.root = Path(self.fixture.name)
        self.args = argparse.Namespace(**{name: self.root / name for name in
            ("base", "candidate", "draw_proof", "present_proof", "output")})
        self.run = mock.patch.object(package.P, "run", side_effect=AssertionError("No build/run is allowed"))
        self.run.start()

    def tearDown(self):
        self.run.stop()
        self.fixture.cleanup()

    def test_existing_output_is_not_replaced(self):
        self.args.output.mkdir()
        sentinel = self.args.output / "preserved"
        sentinel.write_text("keep")
        with self.assertRaisesRegex(package.P.PackageError, "Output exists"):
            package.validate(self.args)
        self.assertEqual(sentinel.read_text(), "keep")

    def test_output_cannot_contain_or_replace_an_input(self):
        for output in (self.args.base, self.root / "candidate/child"):
            self.args.output = output
            with self.assertRaisesRegex(package.P.PackageError, "overlaps"):
                package.validate(self.args)

    def test_linked_output_is_refused(self):
        self.args.output.symlink_to(self.root / "outside")
        with self.assertRaises(package.P.PackageError):
            package.validate(self.args)

    def test_unreviewed_base_is_refused_before_candidate_read(self):
        with mock.patch.object(package.P, "tree_identity", return_value="0" * 64):
            with self.assertRaisesRegex(package.P.PackageError, "reviewed V15"):
                package.validate(self.args)

    def test_different_candidate_is_refused_before_proof_read(self):
        self.args.candidate.write_bytes(b"not-the-reviewed-DLL")
        with mock.patch.object(package.P, "tree_identity", return_value=package.BASE_IDENTITY):
            with self.assertRaisesRegex(package.P.PackageError, "candidate differs"):
                package.validate(self.args)


if __name__ == "__main__":
    unittest.main()
