#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Pure parser/provenance regressions; no Wine, graphics process or test prefix."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dxmt_d3d12_harness", Path(__file__).with_name("test-dxmt-d3d12-dev.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


def records(draw=False):
    start = {"event": "probe", "schema": 1, "api": "d3d12", "fallback": False}
    finish = {"event": "result", "passed": True, "game_compatibility_verified": False}
    if draw:
        start["workload"] = "shader_draw"
        finish["present_verified"] = False
    names = runner.BASE_CHECKS | (runner.DRAW_CHECKS if draw else set()) | {
        "draw_pixels_match" if draw else "clear_pixels_match"}
    rows = [start, {"event": "identity_ready"}]
    rows += [{"event": "check", "name": name, "passed": True, "hresult": "0x00000000"}
             for name in sorted(names)]
    if draw:
        rows.append({"event": "draw_pixels", "verified": 256, "total": 256})
    return [*rows, finish]


def parse(rows, draw=False, exit_code=0):
    return runner.parse_result(("\n".join(json.dumps(row) for row in rows) + "\n").encode(), draw, exit_code)


def receipt():
    return {"schema": 1, "experimental": True, "installed": False, "app_local": True,
            "wine_builtin_marked": False, "dxmt_revision": runner.builder.PIN,
            "directx_headers_revision": runner.builder.HEADERS_PIN,
            "inputs": copy.deepcopy(runner.builder.INPUTS), "patches": copy.deepcopy(runner.builder.PATCHES),
            "sha256": "a" * 64}


class ParserTests(unittest.TestCase):
    def test_complete_clear_and_draw_records(self):
        self.assertEqual(parse(records())["offscreen_pixels_verified"], 256)
        self.assertTrue(parse(records(True), True)["shader_draw_verified"])

    def test_missing_and_duplicate_stage_are_refused(self):
        valid = records()
        for changed in (valid[:2] + valid[3:], valid[:3] + [copy.deepcopy(valid[2])] + valid[3:]):
            with self.assertRaises(runner.Error):
                parse(changed)

    def test_failed_operation_or_late_crash_cannot_claim_success(self):
        changed = records()
        changed[2]["passed"] = False
        with self.assertRaises(runner.Error):
            parse(changed)
        with self.assertRaises(runner.Error):
            parse(records(), exit_code=1)

    def test_draw_with_zero_verified_pixels_is_refused(self):
        changed = records(True)
        changed[-2]["verified"] = 0
        with self.assertRaises(runner.Error):
            parse(changed, True)

    def test_boolean_schema_and_extra_records_are_refused(self):
        changed = records()
        changed[0]["schema"] = True
        with self.assertRaises(runner.Error):
            parse(changed)
        changed = records()
        changed.insert(-1, {"event": "unexpected"})
        with self.assertRaises(runner.Error):
            parse(changed)


class ProvenanceTests(unittest.TestCase):
    def test_exact_app_local_receipt_is_accepted(self):
        runner.validate_candidate_receipt(receipt(), "a" * 64)

    def test_wrong_candidate_hash_builtin_or_patch_receipt_is_refused(self):
        changes = ({"sha256": "b" * 64}, {"app_local": False}, {"wine_builtin_marked": True},
                   {"patches": {}}, {"inputs": {}}, {"schema": True})
        for fields in changes:
            changed = receipt()
            changed.update(fields)
            with self.subTest(fields=fields), self.assertRaises(runner.Error):
                runner.validate_candidate_receipt(changed, "a" * 64)


if __name__ == "__main__":
    unittest.main()
