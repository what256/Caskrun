#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Parser acceptance tests only; these never execute Wine or Apple binaries."""
import copy
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "d3dmetal_host_probe_contract", Path(__file__).with_name("test-d3dmetal-host-dev.py"))
probe = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = probe
spec.loader.exec_module(probe)


class HostResultContract(unittest.TestCase):
    def setUp(self):
        names = probe.runner.BASE_CHECKS | {"clear_pixels_match", "owned_hidden_window", "owned_window_dc"}
        self.records = [{"event": "check", "name": name, "passed": True} for name in sorted(names)]
        self.records.append({"event": "result", "passed": True, "game_compatibility_verified": False})

    def test_complete_bounded_api_result_is_accepted(self):
        self.assertTrue(probe.probe_passed(0, self.records))

    def test_any_failed_check_overrides_positive_tail(self):
        for index in range(len(self.records) - 1):
            value = copy.deepcopy(self.records)
            value[index]["passed"] = False
            self.assertFalse(probe.probe_passed(0, value))

    def test_missing_api_or_window_evidence_is_refused(self):
        for index in range(len(self.records) - 1):
            self.assertFalse(probe.probe_passed(0, self.records[:index] + self.records[index + 1:]))

    def test_duplicate_required_check_is_refused(self):
        value = self.records[:-1] + [self.records[0], self.records[-1]]
        self.assertFalse(probe.probe_passed(0, value))

    def test_nonzero_exit_is_refused(self):
        for code in (1, 5, 124, -9):
            self.assertFalse(probe.probe_passed(code, self.records))

    def test_missing_or_misleading_tail_is_refused(self):
        self.assertFalse(probe.probe_passed(0, []))
        self.assertFalse(probe.probe_passed(0, self.records[:-1]))
        for field, value in (("passed", False), ("event", "check"), ("game_compatibility_verified", True)):
            records = copy.deepcopy(self.records)
            records[-1][field] = value
            self.assertFalse(probe.probe_passed(0, records))


class HostSourceProof(unittest.TestCase):
    def proof(self, thread=False):
        names = (*probe.PATCHES, *((probe.THREAD_PATCH,) if thread else ()))
        return {"source_patches": {name: "reviewed-" + name for name in names}}

    def digest(self, path):
        return "reviewed-" + path.name

    def test_original_three_patch_baseline_is_reproducible(self):
        with patch.object(probe.harness, "digest", side_effect=self.digest):
            probe.checked_source_patches(self.proof(), require_thread_validation=True)

    def test_unknown_or_missing_patch_is_refused(self):
        for name in (*probe.PATCHES, "unknown.patch"):
            proof = self.proof()
            if name == "unknown.patch":
                proof["source_patches"][name] = "unreviewed"
            else:
                del proof["source_patches"][name]
            with self.assertRaises(probe.runner.Error):
                probe.checked_source_patches(proof)

    def test_new_thread_patch_requires_source_review(self):
        with self.assertRaises(probe.runner.Error):
            probe.checked_source_patches(self.proof(thread=True))

    def test_thread_test_and_apple_test_have_separate_gates(self):
        proof = self.proof(thread=True)
        proof["native_thread_start_source_reviewed"] = True
        with patch.object(probe.harness, "digest", side_effect=self.digest), \
             patch.object(probe, "checked_thread_source_provenance"):
            probe.checked_source_patches(proof, require_thread_validation=False)
            with self.assertRaises(probe.runner.Error):
                probe.checked_source_patches(proof, require_thread_validation=True)
            proof["native_thread_start_validation_passed"] = True
            probe.checked_source_patches(proof, require_thread_validation=True)

    def test_modified_source_is_refused(self):
        proof = self.proof()
        proof["source_patches"][probe.PATCHES[0]] = "different"
        with patch.object(probe.harness, "digest", side_effect=self.digest):
            with self.assertRaises(probe.runner.Error):
                probe.checked_source_patches(proof)


if __name__ == "__main__":
    unittest.main()
