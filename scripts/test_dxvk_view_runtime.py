#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Offline negative boundaries for the actual DXVK workload proof parser."""
import copy
import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dxvk_view_proof", Path(__file__).with_name("test-dxvk-view-runtime.py"))
proof = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = proof
spec.loader.exec_module(proof)


def operation(name, level=None):
    row = {"event": "check", "name": name, "passed": True, "hresult": "0x00000000"}
    if level is not None:
        row["feature_level"] = level
    return row


def fixture(api):
    if api == "d3d9":
        rows = [{"event": "probe", "schema": 1, "api": "d3d9ex", "architecture": "x64", "watchdog_ms": 30000}]
        rows += [operation(name) for name in ("watchdog_event", "watchdog_thread", "load_d3d9", "identity_gate",
                 "d3d9_export", "create_d3d9ex", "window_class", "create_hwnd", "hardware_device")]
        for frame in range(6):
            if frame == 4:
                rows.append(operation("reset_resize"))
            rows += [operation(name) for name in ("clear", "cull_state", "lighting_state", "depth_state",
                "vertex_format", "viewport", "no_texture", "diffuse_color_op", "diffuse_color_arg",
                "diffuse_alpha_op", "diffuse_alpha_arg", "disable_second_stage", "begin_scene",
                "draw_triangle", "end_scene", "present")]
            for phase in ("clear", "triangle"):
                rows += [operation(name) for name in ("get_render_target", "create_staging_surface",
                          "readback_surface", "lock_staging_surface")]
                for position in ("corner", "center"):
                    color = [0, 255, 0, 255] if (phase, position) == ("triangle", "center") else [32, 64, 128, 255]
                    rows.append({"event": "pixel", "phase": phase, "position": position, "passed": True,
                                 "actual_rgba": color.copy(), "expected_rgba": color.copy()})
        rows.append({"event": "result", "passed": True, "failed_checks": 0})
    else:
        rows = [{"event": "probe", "schema": 4, "architecture": "x64", "watchdog_ms": 30000,
                 "driver": "hardware", "mode": "lifecycle", "identity_source": "host_mapped_files"}]
        initial = proof.contract.NONE_CHECKS.copy()
        del initial["create_hwnd"]
        initial["identity_gate"] = 1
        rows += [operation(name, "none") for name in initial.elements()]
        for name in ("d3d11.dll", "dxgi.dll"):
            rows.append({"event": "module", "name": name, "path": "C:\\windows\\system32\\" + name,
                         "sha256": "a" * 64, "expected_sha256": "a" * 64, "matches_receipt": True})
        for level in ("10_0", "11_0"):
            rows += [operation(name, level) for name in proof.contract.LIFECYCLE_CHECKS.elements()]
            for name in proof.contract.LIFECYCLE_PIXELS.elements():
                color = [0, 255, 0, 255] if name.endswith("triangle_center") else [32, 64, 128, 255]
                rows.append({"event": "pixel", "feature_level": level, "name": name, "passed": True,
                    "actual_rgba": color.copy(), "expected_rgba": color.copy(), "tolerance": 3})
            rows.append({"event": "dimensions", "feature_level": level,
                         "source": "resized_backbuffer", "width": 192, "height": 96})
        rows += [{"event": "modules", "winemetal_loaded": False, "wined3d_loaded": True},
                 {"event": "result", "passed": True, "failed_checks": 0, "exit_code": 0}]
    return rows


class WorkloadProofTests(unittest.TestCase):
    def rejects(self, api, change):
        rows = fixture(api)
        change(rows)
        with self.assertRaises(RuntimeError):
            proof.validate(rows, "x64", api)

    def test_accepts_complete_original_workload_shapes(self):
        for api in ("d3d9", "d3d11"):
            self.assertTrue(proof.validate(fixture(api), "x64", api)["passed"])

    def test_rejects_claiming_a_clear_pixel_is_a_triangle(self):
        def alter(rows):
            pixel = next(row for row in rows if row["event"] == "pixel" and row.get("position") == "center"
                         and row.get("phase") == "triangle")
            pixel["actual_rgba"] = pixel["expected_rgba"] = [32, 64, 128, 255]
        self.rejects("d3d9", alter)

    def test_rejects_d3d11_forged_actual_and_expected_together(self):
        def alter(rows):
            pixel = next(row for row in rows if row["event"] == "pixel")
            pixel["actual_rgba"] = pixel["expected_rgba"] = [0, 0, 0, 255]
        self.rejects("d3d11", alter)

    def test_rejects_duplicated_sample_position_with_same_count(self):
        def alter(rows):
            next(row for row in rows if row["event"] == "pixel" and row["position"] == "center")["position"] = "corner"
        self.rejects("d3d9", alter)

    def test_rejects_reset_after_every_present(self):
        def alter(rows):
            reset = next(row for row in rows if row.get("name") == "reset_resize")
            rows.remove(reset)
            rows.insert(-1, reset)
        self.rejects("d3d9", alter)

    def test_rejects_boolean_numbers_in_probe_identity_and_result(self):
        self.rejects("d3d9", lambda rows: rows[0].update(schema=True))
        self.rejects("d3d9", lambda rows: rows[-1].update(failed_checks=False))

    def test_rejects_unknown_or_duplicate_terminal_records(self):
        self.rejects("d3d11", lambda rows: rows.insert(-1, {"event": "unreviewed"}))
        self.rejects("d3d9", lambda rows: rows.insert(-1, copy.deepcopy(rows[-1])))

    def test_rejects_reported_success_with_failed_operation_hresult(self):
        self.rejects("d3d9", lambda rows: rows[1].update(hresult="0x80004005"))

    def test_rejects_wrong_resize_dimensions(self):
        def alter(rows):
            next(row for row in rows if row["event"] == "dimensions")["width"] = 128
        self.rejects("d3d11", alter)

    def test_rejects_wrong_logical_module_when_expected_is_bound(self):
        with self.assertRaises(RuntimeError):
            proof.validate(fixture("d3d11"), "x64", "d3d11",
                           logical_expected={"d3d11.dll": "b" * 64, "dxgi.dll": "a" * 64})

    def test_rejects_omitting_a_feature_level_lifecycle(self):
        def alter(rows):
            next(row for row in rows if row["event"] == "check" and row.get("feature_level") == "11_0")["feature_level"] = "10_0"
        self.rejects("d3d11", alter)

    def test_rejects_unbounded_record_list(self):
        self.rejects("d3d9", lambda rows: rows.extend([copy.deepcopy(rows[1])] * 513))

    def test_rejects_extra_operation_at_an_unknown_feature_level(self):
        self.rejects("d3d11", lambda rows: rows.insert(-1, operation("hardware_device", "unreviewed")))

    def test_rejects_success_numbers_in_place_of_boolean_evidence(self):
        self.rejects("d3d9", lambda rows: rows[1].update(passed=1))


if __name__ == "__main__":
    unittest.main()
