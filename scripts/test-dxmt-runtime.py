#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run the original Direct3D probe in fresh owned Caskrun/WoW64 bottles.

Requires a project-local core, independently packaged experimental DXMT runtime,
and both compiled scripts/probes/dxmt-smoke.c PEs. Uses only Python's standard
library and the existing launcher harness's tested ownership/process handling.
No accounts, downloads, games, installed app replacement or existing bottles.
All raw module paths and diagnostics stay ignored. Success removes the session;
failure stops only its prefix servers and preserves evidence for local review.
This checks rendering and Present APIs, not visible pixels or game compatibility.
"""
from __future__ import annotations

from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import argparse
import importlib.util
import json
import ntpath
import os
from pathlib import Path
import re
import secrets
import shutil
import struct
import subprocess
import sys
import time

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("caskrun_dxmt_launcher_harness", Path(__file__).with_name("test-launchers.py"))
harness = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = harness
spec.loader.exec_module(harness)
HarnessError = harness.HarnessError
MAX_LOG = 4 * 1024 * 1024
MAX_RECORDS = 512
HASH = re.compile(r"[a-f0-9]{64}\Z")
ARCHITECTURES = {"x64": ("x86_64", 0x8664), "x86": ("i386", 0x014C)}
OVERRIDES = "mscoree,mshtml="
LEGACY_OVERRIDES = "d3d11,dxgi,d3d10core,winemetal=b;mscoree,mshtml="
NONE_CHECKS = Counter({name: 1 for name in (
    "watchdog_event", "watchdog_thread", "load_d3d11_dxgi", "d3d11.dll",
    "dxgi.dll", "d3d11_exports", "shader_compiler", "compiler_export",
    "window_class", "create_hwnd")})
LEVEL_CHECKS = Counter({
    "hardware_device": 1, "offscreen_texture": 1, "hwnd_swapchain": 1,
    "swapchain_backbuffer": 1, "window_open": 4, "present": 4,
    "create_render_target": 5, "create_staging_texture": 10, "map_staging_texture": 10,
    **{name: 5 for name in ("compile_vertex_shader", "compile_pixel_shader",
       "create_vertex_shader", "create_pixel_shader", "create_input_layout",
       "create_vertex_buffer", "create_rasterizer")},
})
PIXELS = Counter({**{f"offscreen_{stage}_{position}": 1 for stage in ("clear", "triangle")
                    for position in ("corner", "center")},
                  **{f"hwnd_{stage}_{position}": 4 for stage in ("clear", "triangle")
                    for position in ("corner", "center")}})
LIFECYCLE_CHECKS = Counter({
    **{name: 1 for name in ("lifecycle_primary_hwnd", "lifecycle_secondary_hwnd",
       "lifecycle_primary_swapchain", "lifecycle_secondary_swapchain", "lifecycle_recreated_swapchain",
       "lifecycle_resize_rectangle", "lifecycle_resize_hwnd", "lifecycle_resize_buffers",
       "lifecycle_resized_dimensions", "lifecycle_release_before_recreate", "lifecycle_destroy_primary_hwnd",
       "lifecycle_release_after_destroy", "lifecycle_release_secondary", "lifecycle_destroy_secondary_hwnd",
       "lifecycle_resized_present", "lifecycle_recreated_present", "lifecycle_survivor_present")},
    "lifecycle_get_backbuffer": 4, "lifecycle_window_open": 7,
    "lifecycle_primary_present": 2, "lifecycle_secondary_present": 2,
    "create_render_target": 7, "create_staging_texture": 14, "map_staging_texture": 14,
    **{name: 7 for name in ("compile_vertex_shader", "compile_pixel_shader",
       "create_vertex_shader", "create_pixel_shader", "create_input_layout",
       "create_vertex_buffer", "create_rasterizer")},
})
LIFECYCLE_PIXELS = Counter({f"lifecycle_{phase}_{stage}_{position}": count
    for phase, count in (("primary", 2), ("secondary", 2), ("resized", 1), ("recreated", 1), ("survivor", 1))
    for stage in ("clear", "triangle") for position in ("corner", "center")})


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise HarnessError("A probe record contains duplicate JSON keys")
        result[key] = value
    return result


def parse_probe(data: bytes, architecture: str, expected: dict[str, str], *, lifecycle=False,
                identity_only=False, mapped_expected=None, mapping_evidence=None) -> dict:
    """Recompute evidence; never trust a reported pass without every stage."""
    if identity_only and lifecycle:
        raise HarnessError("Identity and rendering lifecycle evidence cannot be combined")
    if len(data) > MAX_LOG:
        raise HarnessError("Probe diagnostics exceed the bounded log limit")
    records = []
    for line in data.decode("utf-8", "strict").splitlines():
        if not line.startswith("{"):
            continue  # Wine diagnostics are retained separately, never published.
        try:
            record = json.loads(line, object_pairs_hook=unique_object)
        except (ValueError, UnicodeError):
            raise HarnessError("Probe output contains malformed JSON") from None
        if not isinstance(record, dict) or record.get("event") not in {"probe", "module", "check", "pixel", "modules", "result", "dimensions"}:
            raise HarnessError("Probe output contains an unknown record")
        records.append(record)
        if len(records) > MAX_RECORDS:
            raise HarnessError("Probe emitted too many records")
    gated = mapped_expected is not None
    identity = {"event": "probe", "schema": 3 if identity_only else 4 if gated else 2 if lifecycle else 1,
                "architecture": architecture, "watchdog_ms": 30000, "driver": "none" if identity_only else "hardware"}
    if identity_only:
        identity["mode"] = "identity"
    elif lifecycle:
        identity["mode"] = "lifecycle"
    if gated:
        identity["identity_source"] = "host_mapped_files"
        if (not isinstance(mapped_expected, dict) or not isinstance(mapping_evidence, dict) or mapping_evidence != {
                "mapping_kind": "txt", "process_verified": True, "sha256": mapped_expected}
                or not all(isinstance(value, str) and HASH.fullmatch(value) for value in mapped_expected.values())
                or not {"d3d11.dll", "dxgi.dll"}.issubset(mapped_expected)):
            raise HarnessError("The live mapped-module identity was not independently verified")
    elif mapping_evidence is not None:
        raise HarnessError("Unexpected mapped-module evidence")
    if not records or records[0] != identity:
        raise HarnessError("Probe identity or architecture is not the expected one")
    if records[-1] != {"event": "result", "passed": True, "failed_checks": 0, "exit_code": 0}:
        raise HarnessError("Probe did not finish successfully")
    if Counter(row["event"] for row in records)["probe"] != 1 or Counter(row["event"] for row in records)["result"] != 1:
        raise HarnessError("Probe has duplicate start or result records")
    modules = [row for row in records if row["event"] == "module"]
    if Counter(row.get("name") for row in modules) != Counter({"d3d11.dll": 1, "dxgi.dll": 1}):
        raise HarnessError("Probe did not identify both selected graphics modules")
    for row in modules:
        wanted = expected[row["name"]]
        if (row.get("sha256") != wanted or row.get("expected_sha256") != wanted
                or row.get("matches_receipt") is not True
                or not isinstance(row.get("path"), str)
                or ntpath.basename(row["path"]).lower() != row["name"]):
            raise HarnessError("A loaded graphics module does not match the candidate receipt")
    checks = {level: Counter() for level in ("none", "10_0", "11_0")}
    pixels = {level: Counter() for level in ("10_0", "11_0")}
    expected_checks = LIFECYCLE_CHECKS if lifecycle else LEVEL_CHECKS
    expected_pixels = LIFECYCLE_PIXELS if lifecycle else PIXELS
    for row in records:
        if row["event"] == "check":
            level = row.get("feature_level")
            if level not in checks or row.get("passed") is not True or row.get("hresult") != "0x00000000":
                raise HarnessError("A required Direct3D operation failed")
            checks[level][row.get("name")] += 1
        elif row["event"] == "pixel":
            level, name = row.get("feature_level"), row.get("name")
            actual = row.get("actual_rgba")
            if level not in pixels or name not in expected_pixels or not isinstance(actual, list) or len(actual) != 4:
                raise HarnessError("Readback evidence has an unknown shape")
            wanted = [0, 255, 0, 255] if name.endswith("triangle_center") else [32, 64, 128, 255]
            if (row.get("expected_rgba") != wanted or row.get("tolerance") != 3 or row.get("passed") is not True
                    or any(type(value) is not int or not 0 <= value <= 255 for value in actual)
                    or any(abs(value - target) > 3 for value, target in zip(actual, wanted))):
                raise HarnessError("A clear or triangle pixel did not match the expected colour")
            pixels[level][name] += 1
    expected_none = NONE_CHECKS.copy()
    if identity_only:
        expected_none = Counter({name: 1 for name in ("watchdog_event", "watchdog_thread", "load_d3d11_dxgi", "d3d11.dll", "dxgi.dll")})
        expected_checks = Counter()
        expected_pixels = Counter()
    elif lifecycle:
        del expected_none["create_hwnd"]
    if gated:
        expected_none["identity_gate"] = 1
    if checks["none"] != expected_none or any(checks[level] != expected_checks or pixels[level] != expected_pixels for level in pixels):
        raise HarnessError("Probe omitted or duplicated a required rendering stage")
    dimensions = [row for row in records if row["event"] == "dimensions"]
    if lifecycle and not identity_only:
        if (len(dimensions) != 2 or Counter(row.get("feature_level") for row in dimensions) != Counter({"10_0": 1, "11_0": 1})
                or any(row.get("source") != "resized_backbuffer" or row.get("width") != 192 or row.get("height") != 96 for row in dimensions)):
            raise HarnessError("Resized backbuffers did not report the expected dimensions")
    elif dimensions:
        raise HarnessError("A basic probe unexpectedly emitted lifecycle evidence")
    bridges = [row for row in records if row["event"] == "modules"]
    if (len(bridges) != 1 or type(bridges[0].get("winemetal_loaded")) is not bool
            or (not identity_only and bridges[0]["winemetal_loaded"] is not True)
            or type(bridges[0].get("wined3d_loaded")) is not bool):
        raise HarnessError("The expected Metal bridge was not observed")
    if identity_only:
        result = {"architecture": architecture, "passed": True, "mode": "identity",
            "logical_module_sha256": expected, "rendering": "not_tested"}
        if gated:
            result["selected_module_sha256"] = mapped_expected
            result["identity_source"] = "host_mapped_files"
        return result
    result = {"architecture": architecture, "passed": True,
            "selected_module_sha256": mapped_expected if gated else expected, "winemetal_loaded": True,
            "wined3d_loaded": bridges[0]["wined3d_loaded"],
            "feature_levels": {level: {"hardware_device": True, "offscreen_readback": True,
                "hwnd_readback": True, "present_count": checks[level]["present"]} for level in pixels}}
    if lifecycle:
        result["mode"] = "lifecycle"
        result["feature_levels"] = {level: {"two_windows_alternating": True,
            "resized_backbuffer": [192, 96], "recreated_on_same_hwnd": True,
            "release_after_hwnd_destroyed": True, "surviving_window_readback": True,
            "present_count": sum(count for name, count in checks[level].items() if name.endswith("_present"))}
            for level in pixels}
    if gated:
        result["logical_module_sha256"] = expected
        result["identity_source"] = "host_mapped_files"
    return result


def owned_artifact(path: Path, build: Path, *, executable: bool = False) -> Path:
    path = path.absolute()
    harness.real_path(path.parent, directory=True)
    resolved = harness.real_path(path.resolve())
    if not path.is_relative_to(build) or not resolved.is_relative_to(build) or (executable and not os.access(path, os.X_OK)):
        raise HarnessError("Use only owned artifacts inside this project's ignored build directory")
    return path


def pe_architecture(path: Path) -> int:
    with path.open("rb") as handle:
        dos = handle.read(64)
        if len(dos) != 64 or dos[:2] != b"MZ":
            raise HarnessError("A Windows artifact lacks a valid PE header")
        offset = struct.unpack_from("<I", dos, 60)[0]
        if not 64 <= offset <= 1024 * 1024:
            raise HarnessError("A Windows artifact has an invalid PE header offset")
        handle.seek(offset)
        header = handle.read(6)
    if len(header) != 6 or header[:4] != b"PE\0\0":
        raise HarnessError("A Windows artifact lacks a PE signature")
    return struct.unpack_from("<H", header, 4)[0]


def candidate_receipt(runtime: Path, build: Path) -> dict:
    harness.real_path(runtime, directory=True)
    if not runtime.is_relative_to(build):
        raise HarnessError("The candidate must be an owned ignored runtime package")
    path = runtime / "components.json"
    value = harness.read_json(path)
    if (not isinstance(value, dict) or value.get("format") != "caskrun-runtime-components" or value.get("version") != 1
            or value.get("status") != "experimental" or value.get("game_validated") is not False):
        raise HarnessError("The candidate lacks an experimental components receipt")
    entries = value.get("components", [])
    if not isinstance(entries, list) or any(not isinstance(entry, dict) for entry in entries):
        raise HarnessError("The candidate has an invalid components receipt")
    components = [entry for entry in entries if entry.get("name") == "DXMT"]
    if len(components) != 1 or components[0].get("status") != "built" or not re.fullmatch(r"[a-f0-9]{40}", components[0].get("revision", "")):
        raise HarnessError("The candidate does not record one independently built DXMT revision")
    switchable = value.get("layout") == "switchable-v1"
    if switchable and (value.get("default_backend") != "wined3d" or value.get("backend_roots") != {
            "wined3d": "runtime", "dxmt": "components/dxmt/runtime"}):
        raise HarnessError("The candidate has an unknown switchable runtime layout")
    if value.get("layout") is not None and not switchable:
        raise HarnessError("The candidate has an unknown runtime layout")
    expected, mapped, mapped_hashes, baseline, baseline_mapped = {}, {}, {}, {}, {}
    artifacts = value.get("artifacts", {})
    base_artifacts = value.get("base_artifacts", {})
    if (not isinstance(artifacts, dict) or any(not isinstance(entry, dict) for entry in artifacts.values())
            or not isinstance(base_artifacts, dict) or any(not isinstance(entry, dict) for entry in base_artifacts.values())):
        raise HarnessError("The candidate has an invalid graphics artifact receipt")
    dxmt_root = "components/dxmt/runtime" if switchable else "runtime"
    for architecture, (folder, machine) in ARCHITECTURES.items():
        expected[architecture] = {}
        mapped[architecture] = {}
        mapped_hashes[architecture] = {}
        baseline[architecture] = {}
        baseline_mapped[architecture] = {}
        for name in ("d3d11.dll", "dxgi.dll", "d3d10core.dll", "winemetal.dll"):
            relative = f"{dxmt_root}/lib/wine/{folder}-windows/{name}"
            receipt = artifacts.get(relative, {})
            wanted = receipt.get("packaged_sha256", "")
            artifact = owned_artifact(runtime / relative, build)
            if not HASH.fullmatch(wanted) or harness.digest(artifact) != wanted or pe_architecture(artifact) != machine:
                raise HarnessError("A packaged graphics DLL does not match its architecture or receipt")
            if name in {"d3d11.dll", "dxgi.dll"}:
                expected[architecture][name] = wanted
                mapped[architecture][name] = artifact
                mapped_hashes[architecture][name] = wanted
            if switchable:
                relative = f"runtime/lib/wine/{folder}-windows/{name}"
                original = owned_artifact(runtime / relative, build)
                original_hash = base_artifacts.get(relative, {}).get("packaged_sha256", "")
                if not HASH.fullmatch(original_hash) or harness.digest(original) != original_hash or pe_architecture(original) != machine:
                    raise HarnessError("A baseline DLL does not match its architecture or receipt")
                if name in {"d3d11.dll", "dxgi.dll"}:
                    baseline[architecture][name] = original_hash
                    baseline_mapped[architecture][name] = original
    for name in ("winemac.so", "winemetal.so"):
        relative = f"{dxmt_root}/lib/wine/x86_64-unix/{name}"
        wanted = artifacts.get(relative, {}).get("packaged_sha256", "")
        artifact = owned_artifact(runtime / relative, build)
        if not HASH.fullmatch(wanted) or harness.digest(artifact) != wanted:
            raise HarnessError("A packaged Unix graphics bridge does not match its receipt")
        if switchable and name == "winemetal.so":
            for architecture in ARCHITECTURES:
                mapped[architecture][name] = artifact
                mapped_hashes[architecture][name] = wanted
    return {"component_revision": components[0]["revision"],
            "components_receipt_sha256": harness.digest(path), "expected": expected,
            "switchable": switchable, "mapped": mapped, "baseline": baseline,
            "mapped_hashes": mapped_hashes, "baseline_mapped": baseline_mapped}


def mapped_text_paths(data: bytes, pid: int) -> set[Path]:
    """Only lsof's mapped executable-image records count, never open handles."""
    if len(data) > MAX_LOG:
        raise HarnessError("Mapped-file inspection exceeds its bounded limit")
    selected_pid, descriptor = None, None
    paths = set()
    for raw in data.split(b"\0"):
        field = raw.lstrip(b"\n")
        if not field:
            continue
        if field[:1] == b"p":
            if not field[1:].isdigit() or int(field[1:]) != pid or selected_pid is not None:
                raise HarnessError("Mapped-file inspection returned an unexpected process")
            selected_pid = pid
        elif field[:1] == b"f":
            descriptor = field[1:]
        elif field[:1] == b"n" and descriptor == b"txt":
            if selected_pid != pid:
                raise HarnessError("Mapped-file inspection omitted its process identity")
            path = Path(os.fsdecode(field[1:]))
            if path.is_absolute():
                paths.add(path)
    if selected_pid != pid:
        raise HarnessError("Mapped-file inspection omitted its process identity")
    return paths


def mapped_fixture_owners(data: bytes) -> set[int]:
    """Find same-user processes which map this exact owned probe executable."""
    if len(data) > MAX_LOG:
        raise HarnessError("Mapped fixture inspection exceeds its bounded limit")
    owners, pid, uid = set(), None, None
    for raw in [*data.split(b"\0"), b"p"]:
        field = raw.lstrip(b"\n")
        if field[:1] == b"p":
            if pid is not None:
                if uid != os.getuid():
                    raise HarnessError("A mapped probe owner has an unexpected user identity")
                owners.add(pid)
            if not field[1:]:
                pid = None
                continue
            if not field[1:].isdigit():
                raise HarnessError("Mapped fixture inspection returned an invalid process")
            pid, uid = int(field[1:]), None
        elif field[:1] == b"u":
            if pid is None or uid is not None or not field[1:].isdigit():
                raise HarnessError("Mapped fixture inspection returned an invalid user identity")
            uid = int(field[1:])
    return owners


def verify_live_mappings(session, fixture: Path, artifacts: dict[str, Path], expected: dict[str, str]) -> dict:
    """Bind every required mapped image to one still-live owned probe process."""
    if set(artifacts) != set(expected):
        raise HarnessError("Mapped-file expectations do not match the selected backend")
    session._validate_owner()
    # Wine's start.exe can ask the server to create a detached WoW64 child.
    # Ancestry alone then sees only start.exe. Inspect this exact owned fixture
    # file, accepting mapped executable images and this UID only. No discovered
    # process is signalled: prefix-scoped shutdown still owns all cleanup.
    discovery = subprocess.run(["/usr/sbin/lsof", "-nP", "-l", "-a", "-d", "txt", "-F0pu", str(fixture)],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5, check=False)
    if len(discovery.stdout) > MAX_LOG:
        raise HarnessError("Mapped fixture inspection exceeds its bounded limit")
    evidence = session.raw / ("mapped-fixture-" + secrets.token_hex(8) + ".lsof")
    fd = os.open(evidence, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(discovery.stdout)
    if discovery.returncode != 0 or discovery.stderr:
        raise HarnessError("The exact owned probe's mapped process could not be discovered")
    discovered = mapped_fixture_owners(discovery.stdout)
    table = harness.process_table()
    tracked = dict(session.manifest["tracked"])
    for pid in discovered:
        if pid not in table:
            raise HarnessError("A mapped probe exited during ownership discovery")
        tracked[str(pid)] = table[pid]["birth"]
    matched = []
    for saved_pid, birth in tracked.items():
        pid = int(saved_pid)
        if table.get(pid, {}).get("birth") != birth:
            continue
        result = subprocess.run(["/usr/sbin/lsof", "-nP", "-a", "-p", str(pid), "-d", "txt", "-F0pfn"],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5, check=False)
        if len(result.stdout) > MAX_LOG:
            raise HarnessError("Mapped-file inspection exceeds its bounded limit")
        evidence = session.raw / ("mapped-images-" + str(pid) + "-" + secrets.token_hex(8) + ".lsof")
        fd = os.open(evidence, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(result.stdout)
        if result.returncode == 1 and not result.stdout and not result.stderr:
            continue
        if result.returncode != 0 or result.stderr:
            raise HarnessError("The owned probe's mapped files could not be inspected")
        paths = mapped_text_paths(result.stdout, pid)
        # An exact mapped fixture proves this is the Windows probe, rather than
        # a core/wineserver descendant which merely opened the DLLs for a hash.
        if fixture not in paths:
            continue
        if (any(path not in paths for path in artifacts.values())
                or any({path for path in paths if path.name.lower() == name.lower()} != {artifact}
                       for name, artifact in artifacts.items())):
            raise HarnessError("The live probe mapped different graphics files than the selected receipt")
        if any(harness.digest(path) != expected[name] for name, path in artifacts.items()):
            raise HarnessError("A mapped graphics artifact changed during inspection")
        if harness.process_table().get(pid, {}).get("birth") != birth:
            raise HarnessError("The mapped probe process exited or changed during inspection")
        matched.append(pid)
    if len(matched) != 1:
        raise HarnessError("Exactly one live owned probe mapping could not be established")
    session._mapped_probe_processes = {str(pid): tracked[str(pid)] for pid in matched}
    return {"mapping_kind": "txt", "process_verified": True, "sha256": expected}


class DXMTSession(harness.Session):
    def environment(self):
        environment = super().environment()
        cache = self.root / "dxmt-cache"
        cache.mkdir(mode=0o700, exist_ok=True)
        harness.real_path(cache, directory=True)
        configuration = self.root / "dxmt.conf"
        if not configuration.exists():
            configuration.write_text("# Original Caskrun probe: default DXMT settings.\n", encoding="utf-8")
            configuration.chmod(0o600)
        harness.real_path(configuration)
        environment.update({
            "WINEDLLOVERRIDES": OVERRIDES, "WINEDEBUG": "-all", "WINEARCH": "win64",
            "CFFIXED_USER_HOME": str(self.host), "DARWIN_USER_CACHE_DIR": str(cache) + os.sep,
            "DXMT_SHADER_CACHE_PATH": str(cache), "DXMT_LOG_PATH": "none", "DXMT_LOG_LEVEL": "warn",
            "DXMT_CONFIG_FILE": "Z:" + str(configuration).replace("/", "\\"),
        })
        if self.manifest.get("dxmt_legacy_layout"):
            environment["WINEDLLOVERRIDES"] = LEGACY_OVERRIDES
        return environment

    def core_call(self, arguments, deadline=120):
        invocation = self.invoke(arguments, deadline=deadline, cancel_grace=15)
        if invocation.exit_code != 0 or invocation.timed_out or invocation.unresolved or not isinstance(invocation.value, dict):
            raise HarnessError("The isolated core command failed; raw diagnostics were preserved")
        return invocation.value

    def verify_busy_switch_refused(self, bottle: str) -> None:
        directory = harness.real_path(self.storage / "bottles" / bottle, directory=True)
        snapshots = {}
        for name in ("graphics.json", "prefix-record.json"):
            path = directory / name
            snapshots[name] = harness.real_path(path).read_bytes() if path.exists() else None
        # A separate read/guarded mutation process must not share invoke()'s
        # active-invocation cancellation state with the held Windows probe.
        environment = self.environment()
        environment.pop("CASKRUN_CANCEL_STDIN", None)
        result = subprocess.run([str(self.core), "set-bottle-graphics", bottle, "wined3d"],
            cwd=self.root, env=environment, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=False)
        for label, data in (("stdout", result.stdout), ("stderr", result.stderr)):
            path = self.raw / ("busy-switch-" + secrets.token_hex(8) + "." + label)
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "wb") as stream:
                stream.write(data)
        if (result.returncode == 0 or b"Close Windows apps in this bottle before changing it" not in result.stderr):
            raise HarnessError("The live renderer change was not refused by the running-bottle guard")
        for name, before in snapshots.items():
            path = directory / name
            after = harness.real_path(path).read_bytes() if path.exists() else None
            if before != after:
                raise HarnessError("A refused busy renderer change modified bottle preferences or its runtime pin")

    def gated_probe(self, bottle: str, fixture: Path, arguments: list[str], artifacts: dict[str, Path],
                    expected: dict[str, str], *, check_busy_refusal=False) -> tuple[dict, dict]:
        gate = self.root / ("identity-gate-" + secrets.token_hex(16))
        ready, release = gate.with_suffix(".ready"), gate.with_suffix(".release")
        command = ["launch-wait", bottle, str(fixture), "--timeout-seconds", "60", "--", *arguments,
                   "--identity-gate", "Z:" + str(gate).replace("/", "\\")]
        with ThreadPoolExecutor(max_workers=1) as executor:
            future = executor.submit(self.core_call, command, deadline=120)
            # Core attestation and native-app setup happen before the PE's own
            # 30-second watchdog starts; the launch timeout still bounds it.
            expires = time.monotonic() + 75
            while not ready.exists():
                if future.done():
                    future.result()  # Preserve the original invocation error.
                    raise HarnessError("The Windows probe exited before its identity gate")
                if time.monotonic() >= expires:
                    raise HarnessError("The Windows probe did not reach its bounded identity gate")
                time.sleep(0.05)
            harness.real_path(ready)
            # invoke() discovers descendants at most every 200 ms. Let its
            # initial ownership record catch up before inspecting the held PE.
            time.sleep(0.25)
            proof = verify_live_mappings(self, fixture, artifacts, expected)
            if check_busy_refusal:
                self.verify_busy_switch_refused(bottle)
                # The exact same mapped probe must still be alive after this
                # refusal, with no renderer files exchanged underneath it.
                proof = verify_live_mappings(self, fixture, artifacts, expected)
            fd = os.open(release, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            os.close(fd)
            result = future.result()
            self.manifest["tracked"].update(getattr(self, "_mapped_probe_processes", {}))
            self.save()
            return result, proof

    def stop_owned_prefixes(self):
        self._validate_owner()
        server = self.wine.parent / "wineserver"
        for bottle in self.manifest.get("dxmt_bottles", []):
            if not isinstance(bottle, str) or not re.fullmatch(r"[a-f0-9-]{36}", bottle):
                raise HarnessError("A test bottle identifier cannot be verified")
            prefix = harness.real_path(self.storage / "bottles" / bottle / "prefix", directory=True)
            environment = self.environment()
            environment["WINEPREFIX"] = str(prefix)
            def server_call(flag, timeout):
                return subprocess.run([str(server), flag], cwd=self.root, env=environment,
                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    timeout=timeout, check=False)
            # Wine 11 main.c exits !kill_lock_owner() for -k: an already idle
            # prefix therefore returns 1. Test idleness first, without a signal.
            # subprocess.run reaps only its own -w waiter if this quick wait
            # times out; it does not signal the server or Windows processes.
            try:
                waiting = server_call("-w", 1)
            except subprocess.TimeoutExpired:
                waiting = None
            if waiting is not None:
                if waiting.returncode:
                    raise HarnessError("An owned prefix server could not be inspected")
                continue
            stopped = server_call("-k", 15)
            # A server can exit between the first -w and -k. Accept its known
            # no-owner status only after the follow-up wait and the independent
            # process/open-file ownership checks below; never accept it alone.
            if stopped.returncode not in (0, 1) or (stopped.returncode == 1 and stopped.stderr):
                raise HarnessError("An owned prefix server could not be stopped")
            if server_call("-w", 15).returncode:
                raise HarnessError("An owned prefix server could not be verified idle")
        self.assert_idle(timeout=15)


def run(arguments) -> int:
    build = harness.real_path(PROJECT / ".build", directory=True)
    runtime = harness.real_path(arguments.runtime.absolute(), directory=True)
    receipt = candidate_receipt(runtime, build)
    core = owned_artifact(arguments.core, build, executable=True)
    wine = owned_artifact(runtime / "bin/wine", build, executable=True)
    probes = {architecture: owned_artifact(getattr(arguments, "probe_" + architecture), build)
              for architecture in ARCHITECTURES}
    for architecture, probe in probes.items():
        if pe_architecture(probe) != ARCHITECTURES[architecture][1]:
            raise HarnessError("An original probe has the wrong Windows architecture")
    session = DXMTSession.setup(PROJECT, core, wine)
    summary = {"schema": 1, "component": "DXMT", "component_revision": receipt["component_revision"],
        "components_receipt_sha256": receipt["components_receipt_sha256"], "passed": False,
        "mode": "lifecycle" if arguments.lifecycle else "basic",
        "prefix_mode": "separate_fresh_win64_prefixes_for_x64_and_x86_WoW64",
        "override_mode": "core_backend_selection" if receipt["switchable"] else "explicit_builtin",
        "architectures": {}, "gameplay": "untested",
        "visible_pixels": "untested", "cleanup": "pending"}
    session.summary["dxmt"] = summary
    session.manifest["dxmt_bottles"] = []
    session.manifest["dxmt_legacy_layout"] = not receipt["switchable"]
    session.save()
    failure = None
    try:
        for architecture, probe in probes.items():
            bottle = session.core_call(["create", "Original DXMT " + architecture])["id"]
            if not re.fullmatch(r"[a-f0-9-]{36}", bottle):
                raise HarnessError("The core returned an invalid disposable bottle identifier")
            session.manifest["dxmt_bottles"].append(bottle)
            session.save()
            initialized = session.core_call(["initialize", bottle])
            expected_prefix = session.storage / "bottles" / bottle / "prefix"
            if initialized.get("prefix_path") != str(expected_prefix) or initialized.get("initialized") is not True:
                raise HarnessError("The core did not initialize the expected fresh bottle")
            pin = harness.read_json(expected_prefix.parent / "prefix-record.json")
            if not isinstance(pin.get("runtime_identity"), str) or not pin["runtime_identity"].startswith("sha256:"):
                raise HarnessError("The core did not pin the candidate runtime identity")
            fixture = session.root / ("probe-" + architecture + ".exe")
            source_digest = harness.digest(probe)
            shutil.copyfile(probe, fixture)
            fixture.chmod(0o600)
            fixture_digest = harness.digest(fixture)
            if fixture_digest != source_digest:
                raise HarnessError("The original probe changed while its owned copy was prepared")
            def probe_run(*, identity_only=False, backend="dxmt"):
                selected = receipt["expected"][architecture] if backend == "dxmt" else receipt["baseline"][architecture]
                logical = receipt["baseline"][architecture] if receipt["switchable"] else selected
                probe_arguments = ["--expected-d3d11-sha256", logical["d3d11.dll"],
                    "--expected-dxgi-sha256", logical["dxgi.dll"]]
                if identity_only:
                    probe_arguments.append("--identity-only")
                elif arguments.lifecycle:
                    probe_arguments.append("--lifecycle")
                proof, mapped_expected = None, None
                if receipt["switchable"]:
                    artifacts = receipt["mapped"][architecture] if backend == "dxmt" else receipt["baseline_mapped"][architecture]
                    mapped_expected = receipt["mapped_hashes"][architecture] if backend == "dxmt" else selected
                    launched, proof = session.gated_probe(bottle, fixture, probe_arguments, artifacts, mapped_expected,
                        check_busy_refusal=backend == "dxmt" and not identity_only)
                else:
                    launched = session.core_call(["launch-wait", bottle, str(fixture), "--timeout-seconds", "45", "--", *probe_arguments], deadline=90)
                if launched.get("exit_code") != 0 or launched.get("signal") is not None or launched.get("timed_out") is not False:
                    raise HarnessError("The Windows probe failed or timed out; raw diagnostics were preserved")
                log = harness.real_path(Path(launched["log_path"]))
                if log.parent != expected_prefix.parent / "logs" or log.stat().st_size > MAX_LOG:
                    raise HarnessError("The core returned an unexpected or excessive probe log")
                result = parse_probe(log.read_bytes(), architecture, logical,
                    lifecycle=arguments.lifecycle and not identity_only, identity_only=identity_only,
                    mapped_expected=mapped_expected, mapping_evidence=proof)
                session.stop_owned_prefixes()
                return result

            def checked_backend(command, selected):
                state = session.core_call(command)
                if (state.get("bottle_id") != bottle or state.get("selected_backend") != selected
                        or state.get("effective_backend") != ("dxmt" if selected == "dxmt" else "wined3d")
                        or state.get("dxmt_available") is not True):
                    raise HarnessError("The core did not confirm the verified requested backend")

            if receipt["switchable"]:
                checked_backend(["bottle-graphics", bottle], "auto")
                transitions = {"default": probe_run(identity_only=True, backend="wined3d")}
                checked_backend(["set-bottle-graphics", bottle, "dxmt"], "dxmt")
                parsed = probe_run()
                parsed["busy_renderer_change"] = "refused_without_preference_or_pin_changes"
                checked_backend(["set-bottle-graphics", bottle, "wined3d"], "wined3d")
                transitions["restored_wined3d"] = probe_run(identity_only=True, backend="wined3d")
                checked_backend(["set-bottle-graphics", bottle, "auto"], "auto")
                transitions["restored_auto"] = probe_run(identity_only=True, backend="wined3d")
                parsed["backend_switching"] = transitions
            else:
                parsed = probe_run()
            if harness.digest(fixture) != fixture_digest:
                raise HarnessError("The executed probe fixture changed during validation")
            parsed["runtime_identity"] = pin["runtime_identity"]
            parsed["probe_sha256"] = fixture_digest
            summary["architectures"][architecture] = parsed
            session.save()
        summary["passed"] = True
    except (HarnessError, OSError, UnicodeError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        failure = str(error) if isinstance(error, HarnessError) else "The isolated validation could not finish; local diagnostics were preserved"
        summary["reason"] = failure
    finally:
        try:
            session.stop_owned_prefixes()
            summary["cleanup"] = "verified_idle"
            session.save()
            if failure is None:
                summary["cleanup"] = "removed"
                session.cleanup()
            else:
                summary["cleanup"] = "preserved_idle"
                session.save()
        except (HarnessError, OSError, subprocess.TimeoutExpired) as error:
            summary["passed"] = False
            summary["cleanup"] = "preserved_owners_unconfirmed"
            summary["cleanup_reason"] = str(error) if isinstance(error, HarnessError) else "Prefix stop or ownership inspection did not complete"
            session.save()
    # No raw paths, usernames, process tables or vendor diagnostics in this output.
    print(json.dumps(summary, sort_keys=True))
    if summary["cleanup"].startswith("preserved"):
        print("Owned evidence preserved in .build/launcher-validation-" + session.session_id, file=sys.stderr)
        print("Cleanup after review: python3 scripts/test-dxmt-runtime.py cleanup " + session.session_id, file=sys.stderr)
    return 0 if summary["passed"] and summary["cleanup"] == "removed" else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    start = commands.add_parser("run", help="Launch original Windows probes in disposable bottles")
    for name in ("core", "runtime", "probe-x64", "probe-x86"):
        start.add_argument("--" + name, type=Path, required=True)
    start.add_argument("--lifecycle", action="store_true", help="Check resize, two windows, recreation and destruction in separate fresh bottles")
    cleanup = commands.add_parser("cleanup", help="Stop and remove one owned saved test session")
    cleanup.add_argument("session")
    arguments = parser.parse_args(argv)
    try:
        if arguments.action == "run":
            return run(arguments)
        session = DXMTSession(PROJECT, arguments.session)
        if "dxmt" not in session.summary or "dxmt_bottles" not in session.manifest:
            raise HarnessError("This is not an owned DXMT test session")
        session.stop_owned_prefixes()
        session.cleanup()
        print("Owned DXMT fixture removed after prefix-scoped stop and zero-owner checks.")
        return 0
    except (HarnessError, OSError, UnicodeError, ValueError, TypeError, KeyError) as error:
        print(str(error) if isinstance(error, HarnessError) else "DXMT validation setup failed; no user bottles were selected", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
