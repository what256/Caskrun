#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run one original D3D12 clear/draw/presentation probe in a fresh owned prefix.

Requires the isolated --app-local DLL from build-dxmt-d3d12-dev.py and an
already compiled x64 scripts/probes/d3d12[-draw|-present]-smoke.c via --probe.
No downloads, builds, installed app changes, games or user bottles. V15 remains
read-only. Startup is limited to 60s and the probe to 30s (also its PE watchdog).
Exact mapped images, not logical builtin stubs alone, identify the renderer.
Only the common wineserver for this fresh prefix is stopped. Success removes
the session after independent ownership checks; failures preserve ignored logs.
The printed/written summary contains no paths, account or process identifiers.
"""
from __future__ import annotations

import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parents[1]


def load_module(name, filename):
    spec = importlib.util.spec_from_file_location(name, PROJECT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


builder = load_module("caskrun_d3d12_builder", "build-dxmt-d3d12-dev.py")
contract = load_module("caskrun_d3d12_mapping", "test-dxmt-runtime.py")
harness = contract.harness
Error = harness.HarnessError
MAX_LOG = 4 * 1024 * 1024
V15 = PROJECT / ".build/native-text-wine-runtime-dev-v15"
EXPERIMENT = PROJECT / ".build/d3dmetal-compat-oct26"
# Additional execution/optional compiler inputs, beyond the builder's pins.
EXECUTION_INPUTS = {
    "bin/_dispatch": "48fa40c71c6d9332573c378e402d94b8de0556d56b973339fe52f63c4a304b3a",
    "components/dxmt/runtime/bin/wine": "061c7fab3177ca39131664c734c757993fed380df04d7c761d75c49d205557a3",
    "components/dxmt/runtime/lib/wine/x86_64-unix/ntdll.so": "fa562ffa6f0d5d4fdc5a6587ba7d0b27313dbf8e95fca5e5a1a8f354cd961070",
    "runtime/bin/wineserver": "5427fad31731e8f427e93756c47712745deb3e677a8f7971f0da0a9709eeab66",
    "runtime/lib/wine/x86_64-windows/dxgi.dll": "0e72376b1bede49fa9c464e1868c65e3771b2f8f870920b3bb43681703012d9d",
}
COMPILER_INPUTS = {
    "components/dxmt/runtime/lib/wine/x86_64-windows/d3dcompiler_47.dll": "fd20caa7ef511772c1ef0da9f835d9ad6e0f5a047614f443b44d51618ca1b80e",
    "components/dxmt/runtime/lib/wine/x86_64-windows/wined3d.dll": "5ff940a123294220c961f483d28d5b02ca518d7cbe210ff6b2ee0d7227cb0f5d",
}
BASE_CHECKS = {
    "load_dxgi", "load_d3d12", "logical_dxgi_hash", "logical_d3d12_hash",
    "host_identity_gate", "factory_export", "dxgi_factory", "first_adapter",
    "hardware_adapter", "device_export", "hardware_device_11_0", "command_queue",
    "command_allocator", "command_list", "rtv_heap", "gpu_texture", "readback_layout",
    "readback_buffer", "close_commands", "fence", "fence_event", "signal_fence",
    "fence_completion", "gpu_complete", "map_readback",
}
DRAW_CHECKS = {
    "load_compiler", "logical_compiler_hash", "compiler_export", "root_serializer_export",
    "compile_vertex_shader", "vertex_shader_blob", "compile_pixel_shader", "pixel_shader_blob",
    "serialize_root_signature", "root_signature_blob", "root_signature", "graphics_pipeline",
}


def require(condition, message):
    if not condition:
        raise Error(message)


def artifact(path):
    path = harness.real_path(path.absolute())
    require(path.is_relative_to(PROJECT / ".build"), "Use only project-local build artifacts")
    return path


def validate_candidate_receipt(receipt, candidate_hash):
    require(isinstance(receipt, dict) and type(receipt.get("schema")) is int
            and receipt.get("schema") == 1 and receipt.get("experimental") is True
            and receipt.get("installed") is False and receipt.get("app_local") is True
            and receipt.get("wine_builtin_marked") is False
            and receipt.get("dxmt_revision") == builder.PIN
            and receipt.get("directx_headers_revision") == builder.HEADERS_PIN
            and receipt.get("inputs") == builder.INPUTS and receipt.get("patches") == builder.PATCHES
            and receipt.get("sha256") == candidate_hash,
            "Candidate does not match the reviewed app-local provenance")


def checked_inputs(runtime, candidate, probe, draw):
    require(runtime.absolute() == V15, "Use the reviewed V15 runtime")
    harness.real_path(runtime, directory=True)
    expected = {**builder.INPUTS, **EXECUTION_INPUTS, **(COMPILER_INPUTS if draw else {})}
    for name, wanted in expected.items():
        require(harness.digest(artifact(runtime / name)) == wanted, "A reviewed runtime input changed")
    for name, wanted in builder.PATCHES.items():
        require(harness.digest(harness.real_path(PROJECT / "runtime-patches" / name)) == wanted,
                "A reviewed source patch changed")
    # These are the package's intentional relative command aliases, not an
    # arbitrary executable link supplied by the caller.
    wine = runtime / "bin/wine"
    require(wine.is_symlink() and os.readlink(wine) == "_dispatch", "Unexpected Wine entrypoint")
    require(os.access(wine, os.X_OK), "The reviewed Wine entrypoint is not executable")
    candidate, probe = artifact(candidate), artifact(probe)
    require(candidate.name == "d3d12-native.dll", "Use the separate --app-local candidate")
    require(contract.pe_architecture(candidate) == 0x8664 and contract.pe_architecture(probe) == 0x8664,
            "Candidate and original probe must both be x64 PE images")
    receipt = harness.read_json(candidate.with_name(candidate.stem + "-provenance.json"))
    validate_candidate_receipt(receipt, harness.digest(candidate))
    require(not candidate.is_relative_to(runtime) and not probe.is_relative_to(runtime),
            "Experimental candidate and probe must be separate from the reviewed runtime")
    return wine, candidate, probe, expected


class Session:
    def __init__(self, runtime):
        harness.real_path(EXPERIMENT, directory=True)
        self.root = Path(tempfile.mkdtemp(prefix="dxmt-d3d12-probe-", dir=EXPERIMENT))
        self.raw = self.root / "raw"
        for folder in ("raw", "home", "app", "tmp", "cache"):
            (self.root / folder).mkdir(mode=0o700)
        for folder in ("Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "Library"):
            (self.root / "home" / folder).mkdir(mode=0o700)
        self.runtime = runtime
        self.prefix = self.root / "prefix"
        self.token = secrets.token_hex(32)
        metadata = self.root.stat()
        self.owner = {"schema": 1, "uid": os.getuid(), "token": self.token,
                      "root_identity": [metadata.st_dev, metadata.st_ino]}
        harness.write_json(self.root / ".caskrun-test-owner.json", self.owner)
        self.manifest = {"tracked": {}}
        self.children = []

    def _validate_owner(self):
        harness.real_path(self.root, directory=True)
        metadata = self.root.stat()
        require(metadata.st_uid == os.getuid()
                and [metadata.st_dev, metadata.st_ino] == self.owner["root_identity"]
                and harness.read_json(self.root / ".caskrun-test-owner.json") == self.owner,
                "Fresh session ownership changed; storage was preserved")

    def environment(self, backend):
        # Deliberate allowlist: no inherited loader paths, Wine prefix, tokens,
        # Caskrun configuration, graphics overrides or account environment.
        return {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8",
            "USER": "CaskrunTest", "LOGNAME": "CaskrunTest",
            "HOME": str(self.root / "home"), "CFFIXED_USER_HOME": str(self.root / "home"),
            "TMPDIR": str(self.root / "tmp") + os.sep,
            "XDG_CONFIG_HOME": str(self.root / "cache/config"),
            "XDG_DATA_HOME": str(self.root / "cache/data"),
            "XDG_CACHE_HOME": str(self.root / "cache"),
            "DARWIN_USER_CACHE_DIR": str(self.root / "cache") + os.sep,
            "WINEPREFIX": str(self.prefix), "WINEARCH": "win64", "WINEDEBUG": "-all",
            "WINEDLLOVERRIDES": "mscoree,mshtml=", "CASKRUN_GRAPHICS_BACKEND": backend,
            "DXMT_SHADER_CACHE_PATH": str(self.root / "cache"),
            "DXMT_LOG_PATH": "none", "DXMT_LOG_LEVEL": "warn",
        }

    def track(self, process):
        table = harness.process_table()
        if process.pid in table:
            self.manifest["tracked"].setdefault(str(process.pid), table[process.pid]["birth"])
        harness.collect_descendants(process.pid, self.manifest["tracked"], table)
        harness.write_json(self.root / "processes.json", self.manifest)

    def invoke(self, command, environment, name, seconds, gate=None, inspect=None):
        self._validate_owner()
        output, errors = self.raw / (name + ".jsonl"), self.raw / (name + ".stderr")
        with output.open("xb") as out, errors.open("xb") as err:
            process = subprocess.Popen([str(value) for value in command], cwd=self.root / "app",
                env=environment, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
            self.children.append(process)
            deadline, next_tracking, released = time.monotonic() + seconds, 0.0, gate is None
            while process.poll() is None:
                if time.monotonic() >= next_tracking:
                    self.track(process)
                    next_tracking = time.monotonic() + 0.25
                require(output.stat().st_size <= MAX_LOG and errors.stat().st_size <= MAX_LOG,
                        "Probe diagnostics exceeded the bounded limit")
                if gate is not None and not released and gate.with_suffix(".ready").exists():
                    harness.real_path(gate.with_suffix(".ready"))
                    inspect()
                    fd = os.open(gate.with_suffix(".release"),
                                 os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                    os.close(fd)
                    released = True
                require(time.monotonic() < deadline, "The bounded startup or probe timed out")
                time.sleep(0.05)
            self.track(process)
            require(released, "The original probe exited before its identity gate")
            require(output.stat().st_size <= MAX_LOG and errors.stat().st_size <= MAX_LOG,
                    "Probe diagnostics exceeded the bounded limit")
        return process.returncode, output.read_bytes()

    def stop_and_check_idle(self):
        self._validate_owner()
        environment = self.environment("wined3d")
        server = artifact(self.runtime / "runtime/bin/wineserver")
        require(harness.digest(server) == EXECUTION_INPUTS["runtime/bin/wineserver"],
                "The common server changed; no shutdown was attempted")
        # No prefix exists if startup never began. A real prefix must have no
        # symbolic-link ancestor before a prefix-scoped server command is used.
        if self.prefix.exists() or self.prefix.is_symlink():
            harness.real_path(self.prefix, directory=True)

            def server_call(flag, timeout):
                return subprocess.run([str(server), flag], cwd=self.root, env=environment,
                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    timeout=timeout, check=False)

            try:
                waiting = server_call("-w", 1)
            except subprocess.TimeoutExpired:
                waiting = None
            if waiting is None:
                stopped = server_call("-k", 15)
                require(stopped.returncode in (0, 1) and
                        (stopped.returncode == 0 or not stopped.stderr),
                        "Owned prefix shutdown failed; storage was preserved")
            else:
                require(waiting.returncode == 0, "Owned prefix idleness could not be established")
            require(server_call("-w", 15).returncode == 0,
                    "Owned prefix did not become idle; storage was preserved")
        for child in self.children:
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                raise Error("An owned invocation remains active; storage was preserved") from None
        table = harness.process_table()
        require(not any(table.get(int(pid), {}).get("birth") == birth
                        for pid, birth in self.manifest["tracked"].items()),
                "Owned processes remain; storage was preserved")
        require(not any(str(self.root) in row["command"] for pid, row in table.items()
                        if pid != os.getpid()), "A process still refers to owned test storage")
        inspection = subprocess.run(["/usr/sbin/lsof", "-nP", "-Fpc", "+D", str(self.root)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=15, check=False)
        pids = re.findall(rb"(?m)^p([0-9]+)$", inspection.stdout)
        require(not inspection.stderr and inspection.returncode in (0, 1)
                and not any(int(pid) != os.getpid() for pid in pids)
                and (inspection.returncode != 0 or pids),
                "Independent open-file ownership is inconclusive; storage was preserved")


def mapped_gate(session, fixture, artifacts, expected, draw):
    contract.verify_live_mappings(session, fixture, artifacts, expected)
    session.manifest["tracked"].update(session._mapped_probe_processes)
    pid = next(iter(session._mapped_probe_processes))
    result = subprocess.run(["/usr/sbin/lsof", "-nP", "-a", "-p", pid, "-d", "txt", "-F0pfn"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5, check=False)
    require(result.returncode == 0 and not result.stderr, "Renderer exclusions could not be inspected")
    paths = contract.mapped_text_paths(result.stdout, int(pid))
    dependency = False
    for path in paths:
        name = path.name.lower()
        require(name not in {"d3dmetal", "libd3dshared.dylib"}
                and "moltenvk" not in name and "/components/dxvk/" not in str(path),
                "An unexpected Apple or Vulkan renderer was mapped")
        if name in {"wined3d.dll", "wined3d.so"}:
            compiler_dependency = session.runtime / "components/dxmt/runtime/lib/wine/x86_64-windows/wined3d.dll"
            require(draw and path == compiler_dependency
                    and harness.digest(path) == COMPILER_INPUTS[str(path.relative_to(session.runtime))],
                    "Unexpected WineD3D renderer at the identity gate")
            dependency = True
    harness.write_json(session.raw / "renderer-exclusions.json", {"paths": [str(p) for p in sorted(paths)]})
    return dependency


def parse_present(rows, exit_code):
    start = {"event": "probe", "schema": 1, "api": "d3d12",
             "workload": "present_lifecycle", "fallback": False}
    finish = {"event": "result", "passed": True, "game_compatibility_verified": False,
              "onscreen_pixels_verified": False}
    require(exit_code == 0 and rows[0] == start and rows[-1] == finish,
            "The original presentation probe did not finish successfully")
    initialization = {
        "load_dxgi", "load_d3d12", "logical_dxgi_hash", "logical_d3d12_hash", "host_identity_gate",
        "factory_export", "dxgi_factory", "factory2", "first_adapter", "hardware_adapter",
        "device_export", "hardware_device_11_0", "command_queue", "fence", "fence_event", "window_class",
    }
    window = {"adjust_initial_rect", "create_hwnd", "visible_hwnd",
              "adjust_window_rect", "resize_hwnd", "client_size"}
    chain = {"create_swapchain", "swapchain3", "swapchain_desc", "swapchain_size_format", "swapchain_hwnd"}
    idle = {"queue_signal", "fence_completion", "gpu_idle", "completed_fence_value"}
    frame = {"backbuffer_index", "get_backbuffer", "backbuffer_nonnull", "backbuffer_size_format",
             "command_allocator", "command_list", "rtv_heap", "readback_layout", "readback_buffer",
             "close_commands", "map_readback", "backbuffer_pixels_match", "present", "backbuffer_rotated"}
    expected = {"initialization": Counter({name: 1 for name in initialization}),
                "initial": Counter({name: 1 for name in window | chain}),
                "resized": Counter({name: 1 for name in {
                    "adjust_window_rect", "resize_hwnd", "client_size", "resize_buffers",
                    "swapchain_desc", "swapchain_size_format", "swapchain_hwnd",
                    "destroy_first_hwnd", "first_hwnd_gone"}}),
                "recreated": Counter({name: 1 for name in window | chain | {
                    "destroy_remaining_hwnd", "unregister_window_class"}})}
    for phase in ("initial", "resized", "recreated"):
        expected[phase].update({name: 2 for name in frame})
        expected[phase].update({name: 4 + (2 if phase == "resized" else 1 if phase == "recreated" else 0)
                                for name in idle})
    checks = [row for row in rows if row.get("event") == "check"]
    require(all(row.get("passed") is True and row.get("hresult") == "0x00000000"
                and row.get("phase") in expected for row in checks)
            and all(Counter(row.get("name") for row in checks if row.get("phase") == phase) == wanted
                    for phase, wanted in expected.items()),
            "Presentation omitted, duplicated or failed a required lifecycle operation")
    pixels = [row for row in rows if row.get("event") == "backbuffer_pixels"]
    dimensions = {"initial": (240, 180), "resized": (320, 220), "recreated": (288, 200)}
    require(len(pixels) == 6 and Counter((row.get("phase"), row.get("buffer")) for row in pixels)
            == Counter({(phase, index): 1 for phase in dimensions for index in (0, 1)}),
            "Presentation did not read both buffers at each lifecycle stage")
    for row in pixels:
        width, height = dimensions[row["phase"]]
        require(all(type(row.get(key)) is int for key in ("buffer", "width", "height", "verified", "total"))
                and row["width"] == width and row["height"] == height
                and row["verified"] == row["total"] == width * height,
                "A lifecycle backbuffer readback was incomplete")
    clients = [row for row in rows if row.get("event") == "client_dimensions"]
    require(Counter(row.get("phase") for row in clients) == Counter({phase: 1 for phase in dimensions}),
            "Presentation omitted or duplicated a client-size measurement")
    for row in clients:
        width, height = dimensions[row["phase"]]
        require(row.get("rect_read") is True
                and all(type(row.get(key)) is int for key in
                        ("actual_width", "actual_height", "expected_width", "expected_height"))
                and row["actual_width"] == row["expected_width"] == width
                and row["actual_height"] == row["expected_height"] == height,
                "A native client rectangle did not match the required dimensions")
    require([row for row in rows if row.get("event") not in {"check", "backbuffer_pixels", "client_dimensions"}]
            == [start, {"event": "identity_ready"}, finish],
            "Presentation emitted an unexpected record")
    return {"passed_checks": len(checks), "backbuffer_pixels_verified": sum(row["verified"] for row in pixels),
            "present_count": 6, "resize_verified": True, "hwnd_recreation_verified": True,
            "presentation_api_verified": True, "onscreen_pixels_verified": False}


def parse_result(data, draw, exit_code, present=False):
    require(len(data) <= MAX_LOG, "Probe output exceeded its bounded limit")
    rows = [json.loads(line, object_pairs_hook=contract.unique_object)
            for line in data.decode("utf-8", "strict").splitlines() if line.startswith("{")]
    require(1 <= len(rows) <= 256 and all(isinstance(row, dict) for row in rows)
            and type(rows[0].get("schema")) is int,
            "Original probe records have an invalid shape")
    if present:
        return parse_present(rows, exit_code)
    start = {"event": "probe", "schema": 1, "api": "d3d12", "fallback": False}
    if draw:
        start["workload"] = "shader_draw"
    finish = {"event": "result", "passed": True, "game_compatibility_verified": False}
    if draw:
        finish["present_verified"] = False
    require(exit_code == 0 and rows[0] == start and rows[-1] == finish,
            "The original probe did not report a complete success")
    checks = [row for row in rows if row.get("event") == "check"]
    wanted = BASE_CHECKS | (DRAW_CHECKS if draw else set()) | {
        "draw_pixels_match" if draw else "clear_pixels_match"}
    require(Counter(row.get("name") for row in checks) == Counter({name: 1 for name in wanted})
            and all(row.get("passed") is True and row.get("hresult") == "0x00000000" for row in checks),
            "The probe omitted, duplicated or failed a required operation")
    other = [row for row in rows if row.get("event") != "check"]
    expected = [start, {"event": "identity_ready"}]
    if draw:
        expected.append({"event": "draw_pixels", "verified": 256, "total": 256})
    require(other == [*expected, finish], "Unexpected or incomplete probe pixel evidence")
    return {"passed_checks": len(checks), "offscreen_pixels_verified": 256,
            "shader_draw_verified": draw, "presentation_verified": False}


def run(args):
    summary = {"schema": 1, "experiment": "dxmt-d3d12", "passed": False,
               "workload": "shader_draw" if args.draw else "present_lifecycle" if args.present else "clear",
               "game_compatibility_verified": False, "installed_app_changed": False}
    session = None
    try:
        wine, candidate, probe, pinned = checked_inputs(args.runtime, args.candidate, args.probe, args.draw)
        session = Session(args.runtime)
        candidate_hash, probe_hash = harness.digest(candidate), harness.digest(probe)
        selected, fixture = session.root / "app/d3d12.dll", session.root / "app/d3d12-smoke.exe"
        shutil.copyfile(candidate, selected)
        shutil.copyfile(probe, fixture)
        code, _ = session.invoke([wine, "wineboot.exe", "-u"], session.environment("wined3d"), "bootstrap", 60)
        require(code == 0, "Fresh-prefix startup failed; raw diagnostics were preserved")
        environment = session.environment("dxmt")
        environment["WINEDLLOVERRIDES"] = "d3d12=n;dxgi,d3d11,d3d10core,winemetal=b;mscoree,mshtml="
        pe = args.runtime / "components/dxmt/runtime/lib/wine/x86_64-windows"
        artifacts = {"d3d12.dll": selected, "dxgi.dll": pe / "dxgi.dll", "winemetal.dll": pe / "winemetal.dll",
                     "winemetal.so": args.runtime / "components/dxmt/runtime/lib/wine/x86_64-unix/winemetal.so"}
        if args.draw:
            artifacts["d3dcompiler_47.dll"] = pe / "d3dcompiler_47.dll"
        expected = {name: harness.digest(path) for name, path in artifacts.items()}
        gate = session.root / "app/identity"
        command = [wine, fixture, "--expected-d3d12-sha256", candidate_hash,
                   "--expected-dxgi-sha256", harness.digest(artifact(session.prefix / "drive_c/windows/system32/dxgi.dll")),
                   "--identity-gate", "Z:" + str(gate).replace("/", "\\")]
        if args.draw:
            logical_compiler = artifact(session.prefix / "drive_c/windows/system32/d3dcompiler_47.dll")
            require(harness.digest(logical_compiler) == COMPILER_INPUTS[str((pe / "d3dcompiler_47.dll").relative_to(args.runtime))],
                    "Fresh-prefix logical shader compiler differs from V15")
            command += ["--expected-compiler-sha256", harness.digest(logical_compiler)]
        evidence = {}

        def inspect():
            require(harness.digest(selected) == candidate_hash and harness.digest(fixture) == probe_hash,
                    "An owned probe input changed before the identity gate")
            evidence["wined3d_compiler_dependency_mapped"] = mapped_gate(session, fixture, artifacts, expected, args.draw)

        code, data = session.invoke(command, environment, "probe", 30, gate, inspect)
        parsed = parse_result(data, args.draw, code, args.present)
        require(all(harness.digest(artifact(args.runtime / name)) == wanted for name, wanted in pinned.items())
                and harness.digest(candidate) == candidate_hash and harness.digest(probe) == probe_hash
                and all(harness.digest(path) == expected[name] for name, path in artifacts.items()),
                "A reviewed or selected input changed during the probe")
        summary.update(parsed, **evidence, actual_host_mapping_verified=True,
                       selected_sha256=expected, probe_sha256=probe_hash, passed=True)
    except Error as error:
        summary["error"] = str(error)
    except KeyboardInterrupt:
        summary["error"] = "Probe interrupted; owned-prefix cleanup was attempted"
    except (OSError, ValueError, UnicodeError, subprocess.SubprocessError, KeyError, TypeError):
        summary["error"] = "A required artifact, record or bounded command could not be verified"
    finally:
        if session is not None:
            try:
                session.stop_and_check_idle()
                summary["cleanup"] = "verified_no_owners"
                if summary["passed"]:
                    require(shutil.rmtree.avoids_symlink_attacks, "Safe removal is unavailable")
                    session._validate_owner()
                    shutil.rmtree(session.root)
                    summary["cleanup"] = "removed"
            except (Error, OSError, subprocess.SubprocessError):
                summary.update(passed=False, cleanup="preserved_owners_unconfirmed")
        if session is not None:
            receipt = EXPERIMENT / ("dxmt-d3d12-" + summary["workload"] + "-summary.json")
            try:
                harness.write_json(receipt, summary)
            except (Error, OSError):
                summary.update(passed=False, receipt="could_not_write_sanitized_summary")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["passed"] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=V15)
    parser.add_argument("--candidate", type=Path, default=EXPERIMENT / "dxmt-d3d12/build/d3d12-native.dll")
    parser.add_argument("--probe", type=Path, required=True, help="Already compiled original x64 clear/draw probe")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--draw", action="store_true", help="Require original shader-draw and exact compiler evidence")
    mode.add_argument("--present", action="store_true", help="Require original backbuffer/Present/resize/recreation evidence")
    return run(parser.parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main())
