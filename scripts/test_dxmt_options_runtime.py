#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Negative evidence tests; no Wine, real profiles or publisher apps."""
from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True

def module(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value

runner = module("caskrun_dxmt_options_test_runner", "test-dxmt-options-runtime.py")
fixtures = module("caskrun_dxmt_options_fixture", "test_dxmt_runtime.py")


def evidence(architecture="x64"):
    rows = fixtures.evidence(architecture, lifecycle=True)
    index = len(rows) - 2
    extra = [{"event":"check", "feature_level":"11_0", "name":name,
              "passed":True, "hresult":"0x00000000"} for name in runner.PACING_CHECKS]
    extra.append({"event":"pacing", "feature_level":"11_0", "frames":60, "elapsed_us":2000000,
                  "measurement":"api_submission_not_display_fps"})
    return rows[:index] + extra + rows[index:]


class OptionsEvidenceTests(unittest.TestCase):
    def parse(self, rows, architecture="x64"):
        return runner.parse_options_probe(fixtures.encode(rows), architecture, fixtures.EXPECTED, lifecycle=True)

    def test_complete_evidence_preserves_lifecycle_and_measurement_boundary(self):
        for architecture in ("x64", "x86"):
            result = self.parse(evidence(architecture), architecture)
            self.assertEqual(result["feature_levels"]["11_0"]["present_count"], 7)
            self.assertEqual(result["dxmt_options"]["elapsed_us"], 2000000)
            self.assertEqual(result["dxmt_options"]["display_fps"], "not_measured")

    def test_missing_duplicate_or_failed_pacing_checks_refused(self):
        for change in ("missing", "duplicate", "failed"):
            rows = evidence()
            index = next(i for i,row in enumerate(rows) if row.get("name") == "pacing_timer")
            if change == "missing": rows.pop(index)
            elif change == "duplicate": rows.insert(index, deepcopy(rows[index]))
            else: rows[index]["passed"] = False
            with self.assertRaises(runner.base.HarnessError): self.parse(rows)

    def test_invalid_or_duplicate_timing_refused(self):
        for key,value in (("elapsed_us",0),("elapsed_us",True),("elapsed_us",20000000),
                          ("frames",59),("frames",60.0),("measurement","display_fps"),("feature_level","10_0")):
            rows = evidence()
            next(row for row in rows if row["event"] == "pacing")[key] = value
            with self.assertRaises(runner.base.HarnessError): self.parse(rows)
        rows=evidence()
        rows.insert(-1,deepcopy(next(row for row in rows if row["event"] == "pacing")))
        with self.assertRaises(runner.base.HarnessError): self.parse(rows)

    def test_timing_does_not_replace_rendering_or_module_proof(self):
        rows=evidence()
        next(row for row in rows if row.get("name") == "lifecycle_recreated_present")["passed"]=False
        with self.assertRaises(runner.base.HarnessError): self.parse(rows)
        rows=evidence()
        next(row for row in rows if row.get("event") == "module")["matches_receipt"]=False
        with self.assertRaises(runner.base.HarnessError): self.parse(rows)

    def test_unknown_record_refused(self):
        rows=evidence()
        with self.assertRaises(runner.base.HarnessError):
            runner.parse_options_probe(fixtures.encode(rows)+b'{"event":"unknown"}\n',"x64",fixtures.EXPECTED,lifecycle=True)

    def test_raw_log_limit_cannot_be_bypassed_by_filtering(self):
        with self.assertRaises(runner.base.HarnessError):
            runner.parse_options_probe(b"x"*(runner.base.MAX_LOG+1),"x64",fixtures.EXPECTED,lifecycle=True)


if __name__ == "__main__": unittest.main()
