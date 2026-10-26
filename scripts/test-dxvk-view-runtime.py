#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Validate a dedicated DXVK runtime view without changing prefix or app DLLs.

Uses original Caskrun probes and fresh project-local fixtures only. Bootstrap
uses the base Wine view, then rendering uses the dedicated DXVK view with
builtin-only overrides. Every probe pauses until lsof proves the actual
expected PE backing files, including the WoW64 child rather than start.exe.
Raw mappings/logs remain ignored. The summary contains no personal paths.
"""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import ntpath
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("dxvk_shared_probe_contract", PROJECT / "scripts/test-dxmt-runtime.py")
contract = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = contract
spec.loader.exec_module(contract)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def view_inventory(view):
    return {f"{architecture}/{name}": digest(view / "lib/wine" / architecture / name)
            for architecture in ("x86_64-windows", "i386-windows")
            for name in ("d3d9.dll", "d3d10core.dll", "d3d11.dll", "dxgi.dll")}


def mappings(pid):
    result = subprocess.run(["lsof", "-nP", "-a", "-p", str(pid), "-d", "txt", "-Fn"],
                            capture_output=True, text=True, timeout=5)
    return {line[1:] for line in result.stdout.splitlines() if line.startswith("n")}


def actual_process(required):
    result = subprocess.run(["lsof", "-nP", "-Fpn", "--", str(required[-1])],
                            capture_output=True, text=True, timeout=5)
    wanted = set(map(str, required))
    # The server can hold an ordinary open handle to the exe. Only the real
    # probe maps the exe and all selected DLLs as executable text together.
    owners = {int(line[1:]) for line in result.stdout.splitlines() if line.startswith("p")}
    candidates = [pid for pid in owners if wanted <= mappings(pid)]
    if len(candidates) != 1:
        raise RuntimeError("The exact owned probe and selected PE backing files were not mapped together")
    return candidates[0]


def compiler_dependency(path, architecture):
    tool = "x86_64-w64-mingw32-objdump" if architecture == "x64" else "i686-w64-mingw32-objdump"
    text = subprocess.check_output([tool, "-p", str(path)], text=True, timeout=15)
    imports, current = [], None
    for line in text.split("There is an export table", 1)[0].splitlines():
        if "DLL Name:" in line:
            current = line.split("DLL Name:", 1)[1].strip().lower()
        elif current == "wined3d.dll":
            match = re.match(r"\s*[0-9a-fA-F]+\s+<none>\s+[0-9a-fA-F]+\s+(\S+)", line)
            if match:
                imports.append(match.group(1))
    if not imports or "vkd3d_D3DCompile2VKD3D" not in imports or any(not name.startswith("vkd3d_") for name in imports):
        raise RuntimeError("The WineD3D shader-compiler dependency was not established")
    return {"sha256": digest(path), "wine_d3d_imports": imports}


def validate(records, architecture, api, *, logical_expected=None):
    if architecture not in ("x64", "x86") or api not in ("d3d9", "d3d11"):
        raise RuntimeError("Unknown probe API or architecture")
    allowed = {"probe", "check", "pixel", "result"}
    if api == "d3d11":
        allowed |= {"module", "modules", "dimensions"}
    if (not isinstance(records, list) or not 2 <= len(records) <= contract.MAX_RECORDS
            or any(not isinstance(row, dict) or row.get("event") not in allowed for row in records)):
        raise RuntimeError("Probe records are unbounded or contain an unknown event")
    identity = ({"event": "probe", "schema": 4, "architecture": architecture,
                 "watchdog_ms": 30000, "driver": "hardware", "mode": "lifecycle",
                 "identity_source": "host_mapped_files"} if api == "d3d11" else
                {"event": "probe", "schema": 1, "api": "d3d9ex", "architecture": architecture,
                 "watchdog_ms": 30000})
    finished = {"event": "result", "passed": True, "failed_checks": 0}
    if api == "d3d11":
        finished["exit_code"] = 0
    if (records[0] != identity or type(records[0].get("schema")) is not int
            or type(records[0].get("watchdog_ms")) is not int):
        raise RuntimeError("Probe identity or architecture was not proved")
    if (records[-1] != finished or records[-1].get("passed") is not True
            or type(records[-1].get("failed_checks")) is not int
            or (api == "d3d11" and type(records[-1].get("exit_code")) is not int)):
        raise RuntimeError("Probe did not complete successfully")
    events = Counter(row["event"] for row in records)
    if events["probe"] != 1 or events["result"] != 1:
        raise RuntimeError("Probe start or result was duplicated")
    checks = [row for row in records if row["event"] == "check"]
    pixels = [row for row in records if row["event"] == "pixel"]
    check_keys = {"event", "name", "passed", "hresult"} | ({"feature_level"} if api == "d3d11" else set())
    if any(set(row) != check_keys or not isinstance(row.get("name"), str)
           or row.get("passed") is not True or row.get("hresult") != "0x00000000" for row in checks):
        raise RuntimeError("A required Direct3D operation failed")
    if api == "d3d11":
        if (any(row.get("feature_level") not in ("none", "10_0", "11_0") for row in checks)
                or any(row.get("feature_level") not in ("10_0", "11_0")
                       or not isinstance(row.get("name"), str) for row in pixels)):
            raise RuntimeError("An unknown Direct3D feature level was reported")
        none = contract.NONE_CHECKS.copy(); del none["create_hwnd"]; none["identity_gate"] = 1
        if Counter(row["name"] for row in checks if row["feature_level"] == "none") != none:
            raise RuntimeError("D3D11 initialization evidence is incomplete")
        for level in ("10_0", "11_0"):
            if Counter(row["name"] for row in checks if row["feature_level"] == level) != contract.LIFECYCLE_CHECKS:
                raise RuntimeError("D3D11 lifecycle evidence is incomplete")
            if Counter(row["name"] for row in pixels if row["feature_level"] == level) != contract.LIFECYCLE_PIXELS:
                raise RuntimeError("D3D11 readback evidence is incomplete")
        bridge = [row for row in records if row["event"] == "modules"]
        if (len(bridge) != 1 or bridge[0].get("winemetal_loaded") is not False
                or type(bridge[0].get("wined3d_loaded")) is not bool):
            raise RuntimeError("Another Direct3D renderer was loaded")
        modules = [row for row in records if row["event"] == "module"]
        if Counter(row.get("name") for row in modules) != Counter({"d3d11.dll": 1, "dxgi.dll": 1}):
            raise RuntimeError("The two logical module identities were not recorded")
        if logical_expected is not None and set(logical_expected) != {"d3d11.dll", "dxgi.dll"}:
            raise RuntimeError("Logical module expectations are incomplete")
        for row in modules:
            wanted = logical_expected[row["name"]] if logical_expected is not None else row.get("expected_sha256")
            if (not isinstance(wanted, str) or not contract.HASH.fullmatch(wanted)
                    or row.get("sha256") != wanted or row.get("expected_sha256") != wanted
                    or row.get("matches_receipt") is not True or not isinstance(row.get("path"), str)
                    or ntpath.basename(row["path"]).lower() != row["name"]):
                raise RuntimeError("A logical module did not match its prefix identity")
        dimensions = [row for row in records if row["event"] == "dimensions"]
        if (len(dimensions) != 2
                or Counter(row.get("feature_level") for row in dimensions) != Counter({"10_0": 1, "11_0": 1})
                or any(row.get("source") != "resized_backbuffer" or type(row.get("width")) is not int
                       or type(row.get("height")) is not int or row["width"] != 192 or row["height"] != 96
                       for row in dimensions)):
            raise RuntimeError("Resized backbuffer dimensions were not proved")
    else:
        expected = Counter({name: 1 for name in (
            "watchdog_event", "watchdog_thread", "load_d3d9", "identity_gate", "d3d9_export",
            "create_d3d9ex", "window_class", "create_hwnd", "hardware_device", "reset_resize")})
        expected.update({name: 6 for name in ("clear", "cull_state", "lighting_state", "depth_state",
            "vertex_format", "viewport", "no_texture", "diffuse_color_op", "diffuse_color_arg",
            "diffuse_alpha_op", "diffuse_alpha_arg", "disable_second_stage", "begin_scene",
            "draw_triangle", "end_scene", "present")})
        expected.update({name: 12 for name in ("get_render_target", "create_staging_surface",
                                               "readback_surface", "lock_staging_surface")})
        if Counter(row["name"] for row in checks) != expected or len(pixels) != 24:
            raise RuntimeError("D3D9 initialization, Reset or presentation evidence is incomplete")
        if Counter((row.get("phase"), row.get("position")) for row in pixels) != Counter({
                (phase, position): 6 for phase in ("clear", "triangle") for position in ("corner", "center")}):
            raise RuntimeError("D3D9 readback phases or sample positions are incomplete")
        reset = next(i for i, row in enumerate(records) if row.get("name") == "reset_resize")
        if (sum(row.get("name") == "present" for row in records[:reset]) != 4
                or sum(row.get("name") == "present" for row in records[reset + 1:]) != 2):
            raise RuntimeError("D3D9 did not present both before and after Reset")
    for row in pixels:
        pixel_keys = {"event", "actual_rgba", "expected_rgba", "passed"} | (
            {"name", "feature_level", "tolerance"} if api == "d3d11" else {"phase", "position"})
        if set(row) != pixel_keys or not isinstance(row.get("expected_rgba"), list):
            raise RuntimeError("Readback pixel record has an unknown shape")
        wanted = ([0, 255, 0, 255] if (row["name"].endswith("triangle_center") if api == "d3d11"
                   else row.get("phase") == "triangle" and row.get("position") == "center")
                  else [32, 64, 128, 255])
        if (not isinstance(row.get("actual_rgba"), list) or len(row["actual_rgba"]) != 4
                or row.get("expected_rgba") != wanted or row.get("passed") is not True
                or any(type(value) is not int for value in row["expected_rgba"])
                or (api == "d3d11" and (type(row.get("tolerance")) is not int or row["tolerance"] != 3))):
            raise RuntimeError("Readback pixel shape is invalid")
        if any(type(value) is not int or not 0 <= value <= 255 for value in row["actual_rgba"]):
            raise RuntimeError("Readback pixel is invalid")
        if any(abs(a - b) > (3 if api == "d3d11" else 0) for a, b in zip(row["actual_rgba"], wanted)):
            raise RuntimeError("Readback color does not match its deterministic workload")
    return {"architecture": architecture, "api": api, "checks": len(checks), "pixel_checks": len(pixels),
            "passed": True, "present_count": sum(row["name"].endswith("present") for row in checks)}


class CoreDXVKSession(contract.DXMTSession):
    def environment(self):
        # Core alone selects the backend, override list, driver and loader.
        # Inherit no real user's Wine environment or account credentials.
        env = contract.harness.Session.environment(self)
        env.update({"WINEDEBUG": "-all", "VK_LOADER_LAYERS_DISABLE": "~all~",
                    "DXVK_LOG_LEVEL": "info", "DXVK_LOG_PATH": str(self.raw),
                    "DXVK_STATE_CACHE_PATH": str(self.raw)})
        return env


def core_validation(args):
    """Exercise actual Core selection and the packaged dispatcher read-only."""
    build = contract.harness.real_path(PROJECT / ".build", directory=True)
    package, work = args.fixture.resolve(), args.work.resolve()
    if build not in work.parents or work.exists():
        raise RuntimeError("Use a new ignored evidence directory")
    contract.harness.real_path(package, directory=True)
    if build not in package.parents:
        raise RuntimeError("The packaged runtime must be project-local")
    core = contract.owned_artifact(args.core, build, executable=True)
    wine = contract.owned_artifact(package / "bin/wine", build, executable=True)
    receipt = contract.harness.read_json(package / "components.json")
    dxvk = receipt.get("dxvk", {})
    if dxvk.get("implementation") != "caskrun-dxvk-hybrid-v1":
        raise RuntimeError("The final hybrid package receipt is missing")
    view = package / "components/dxvk/runtime"
    selected_inventory = view_inventory(view)
    if selected_inventory != {name: item["runtime_sha256"] for name, item in dxvk.get("pe_packaging", {}).items()}:
        raise RuntimeError("The packaged translator images do not match their receipt")
    native_paths = {"moltenvk": package / "components/dxvk/vulkan/lib/libMoltenVK.dylib",
                    "vulkan_loader": package / "lib/libvulkan.1.dylib"}
    native_inventory = {key: digest(path) for key, path in native_paths.items()}
    if native_inventory != dxvk.get("gpu_proof", {}).get("native_sha256"):
        raise RuntimeError("Final driver and loader differ from the validated package")
    work.mkdir()
    session = CoreDXVKSession.setup(PROJECT, core, wine)
    session.manifest["dxmt_bottles"] = []  # Reuse the prefix-scoped cleanup contract.
    session.save()
    summary = {"schema": 1, "implementation": "caskrun-dxvk-hybrid-v1",
               "selection": "core_backend_selection_and_packaged_dispatcher",
               "passed": False, "results": [], "cleanup": "pending"}
    failure = None
    try:
        bottle = session.core_call(["create", "Original DXVK API validation"])["id"]
        if not re.fullmatch(r"[a-f0-9-]{36}", bottle):
            raise RuntimeError("Unexpected owned bottle identity")
        session.manifest["dxmt_bottles"].append(bottle)
        session.save()
        initialized = session.core_call(["initialize", bottle])
        prefix = session.storage / "bottles" / bottle / "prefix"
        if initialized.get("initialized") is not True or initialized.get("prefix_path") != str(prefix):
            raise RuntimeError("Core did not initialize the owned test bottle")
        before = {f"{directory}/{name}": digest(prefix / "drive_c/windows" / directory / name)
                  for directory in ("system32", "syswow64")
                  for name in ("d3d8.dll", "d3d9.dll", "d3d10core.dll", "d3d11.dll", "dxgi.dll")}
        state = session.core_call(["set-bottle-graphics", bottle, "dxvk"])
        if (state.get("selected_backend") != "dxvk" or state.get("effective_backend") != "dxvk"
                or state.get("dxvk_available") is not True):
            raise RuntimeError("Core did not select the verified packaged DXVK backend")
        for architecture, pe_dir, windows_dir in (("x64", "x86_64-windows", "system32"), ("x86", "i386-windows", "syswow64")):
            for api in ("d3d11", "d3d9"):
                app = work / (architecture + "-" + api)
                app.mkdir()
                source = getattr(args, ("probe_" if api == "d3d11" else "probe_d3d9_") + architecture)
                shutil.copy2(contract.owned_artifact(source, build), app / "probe.exe")
                names = ("d3d11.dll", "dxgi.dll") if api == "d3d11" else ("d3d9.dll",)
                expected = {name: selected_inventory[f"{pe_dir}/{name}"] for name in names}
                artifacts = {name: view / "lib/wine" / pe_dir / name for name in names}
                logical = {name: before[f"{windows_dir}/{name}"] for name in names} if api == "d3d11" else None
                gate = app / "identity"
                arguments = (["--lifecycle", "--expected-d3d11-sha256", logical["d3d11.dll"],
                              "--expected-dxgi-sha256", logical["dxgi.dll"]] if logical else [])
                arguments += ["--identity-gate", "Z:" + str(gate).replace("/", "\\")]
                command = ["launch-wait", bottle, str(app / "probe.exe"), "--timeout-seconds", "60", "--", *arguments]
                with ThreadPoolExecutor(max_workers=1) as pool:
                    future = pool.submit(session.core_call, command, deadline=100)
                    expires = time.monotonic() + 75
                    while not gate.with_suffix(".ready").exists():
                        if future.done():
                            future.result()
                            raise RuntimeError("Core launch ended before the original probe identity gate")
                        if time.monotonic() > expires:
                            raise RuntimeError("Core launch did not reach its bounded gate")
                        time.sleep(.05)
                    time.sleep(.25)
                    mapped = contract.verify_live_mappings(session, app / "probe.exe", artifacts, expected)
                    pid = int(next(iter(session._mapped_probe_processes)))
                    actual = mappings(pid)
                    if any(path.lower().endswith(("/wined3d.dll", "/winemetal.dll")) for path in actual):
                        raise RuntimeError("A different renderer was present before compilation")
                    if architecture == "x64" and api == "d3d11":
                        session.verify_busy_switch_refused(bottle)
                    gate.with_suffix(".release").touch()
                    seen = set()
                    while not future.done():
                        seen.update(path for path in mappings(pid) if path.endswith(("libMoltenVK.dylib", "libvulkan.1.dylib")))
                        time.sleep(.05)
                    launched = future.result()
                if (launched.get("exit_code") != 0 or launched.get("signal") is not None
                        or launched.get("timed_out") is not False or not set(map(str, native_paths.values())) <= seen):
                    raise RuntimeError("Packaged Core-selected rendering or observed driver identity failed")
                log = contract.harness.real_path(Path(launched["log_path"]))
                if log.parent != prefix.parent / "logs" or log.stat().st_size > contract.MAX_LOG:
                    raise RuntimeError("Core returned an unexpected probe log")
                records = [json.loads(line, object_pairs_hook=contract.unique_object)
                           for line in log.read_text().splitlines() if line.startswith("{")]
                parsed = validate(records, architecture, api, logical_expected=logical)
                (app / "result.jsonl").write_text("".join(json.dumps(row, separators=(",", ":")) + "\n" for row in records))
                parsed.update({"selected_module_sha256": expected, "mapped_runtime_modules": [f"{pe_dir}/{name}" for name in names],
                               "actual_host_mapping_verified": mapped["process_verified"],
                               "observed_native_sha256": native_inventory,
                               "no_wined3d_at_renderer_identity_gate": True})
                summary["results"].append(parsed)
                session.manifest["tracked"].update(session._mapped_probe_processes)
                session.save()
                session.stop_owned_prefixes()
        after = {name: digest(prefix / "drive_c/windows" / name) for name in before}
        if before != after or selected_inventory != view_inventory(view):
            raise RuntimeError("Core-selected rendering changed prefix or translator images")
        if native_inventory != {key: digest(path) for key, path in native_paths.items()}:
            raise RuntimeError("The packaged driver or loader changed during validation")
        summary.update({"passed": True, "prefix_graphics_dll_sha256_before": before,
                        "prefix_graphics_dll_sha256_after": after, "prefix_graphics_dlls_unchanged": True,
                        "selected_runtime_module_sha256": selected_inventory, "native_sha256": native_inventory,
                        "runtime_modules_unchanged": True, "native_images_unchanged": True,
                        "core_sha256": digest(core), "components_receipt_sha256": digest(package / "components.json"),
                        "busy_renderer_change": "refused_without_preference_or_pin_changes"})
    except Exception:
        failure = sys.exc_info()
    finally:
        try:
            session.stop_owned_prefixes()
            if failure is None:
                session.cleanup()
                summary["cleanup"] = "owned_prefix_removed_after_verified_idle"
            else:
                session.save()
                summary["cleanup"] = "preserved_idle_for_diagnosis"
        except Exception:
            summary["cleanup"] = "preserved_owners_unconfirmed"
            if failure is None:
                failure = sys.exc_info()
        (work / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    if failure:
        raise failure[1].with_traceback(failure[2])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--work", required=True, type=Path)
    for name in ("probe-x64", "probe-x86", "probe-d3d9-x64", "probe-d3d9-x86"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--keep-evidence", action="store_true")
    parser.add_argument("--api", choices=("both", "d3d9", "d3d11"), default="both")
    parser.add_argument("--core", type=Path, help="Validate actual Core selection and packaged dispatcher instead of a direct view")
    args = parser.parse_args()
    if args.core:
        core_validation(args)
        return
    fixture, work = args.fixture.resolve(), args.work.resolve()
    if PROJECT / ".build" not in work.parents or work.exists():
        parser.error("Use a new ignored project-local work directory")
    view, base = fixture / "components/dxvk/runtime", fixture / "runtime"
    if not (view / "bin/wine").is_file() or not (base / "bin/wineserver").is_file():
        parser.error("A real isolated dual-view fixture is required")
    work.mkdir(parents=True)
    (work / ".caskrun-dxvk-probe").write_text("original-caskrun-dxvk-view-probe-v1\n")
    home, prefix = work / "home", work / "prefix"; home.mkdir()
    env = os.environ.copy()
    for key in ("DYLD_LIBRARY_PATH", "VK_ICD_FILENAMES", "VK_ADD_DRIVER_FILES", "VK_INSTANCE_LAYERS", "VK_LAYER_PATH"):
        env.pop(key, None)
    env.update({"HOME": str(home), "WINEPREFIX": str(prefix), "WINEDEBUG": "-all",
        "WINEDATADIR": str(base / "share/wine"), "WINEDLLPATH": str(base / "lib/wine"),
        "WINELOADER": str(base / "bin/wine"), "WINESERVER": str(base / "bin/wineserver"),
        "DYLD_FALLBACK_LIBRARY_PATH": str(fixture / "lib"), "WINEDLLOVERRIDES": "mscoree,mshtml=",
        "VK_DRIVER_FILES": str(fixture / "share/vulkan/icd.d/MoltenVK_icd.json"),
        "VK_LOADER_LAYERS_DISABLE": "~all~", "DXVK_LOG_PATH": str(work),
        "DXVK_STATE_CACHE_PATH": str(work), "DXVK_LOG_LEVEL": "info"})
    config = work / "dxvk.conf"; config.write_text("d3d9.deAliasedSamplers = Auto\ndxvk.numCompilerThreads = 3\n")
    env["DXVK_CONFIG_FILE"] = str(config)
    selected_inventory = view_inventory(view)
    native_paths = {"moltenvk": fixture / "lib/libMoltenVK.dylib",
                    "vulkan_loader": fixture / "lib/libvulkan.1.dylib"}
    native_inventory = {key: digest(path) for key, path in native_paths.items()}
    base_d3d8 = {arch: digest(base / "lib/wine" / arch / "d3d8.dll")
                for arch in ("x86_64-windows", "i386-windows")}
    if any(digest(view / "lib/wine" / arch / "d3d8.dll") != value for arch, value in base_d3d8.items()):
        raise RuntimeError("Untested D3D8 must retain the original Wine implementation")
    summary = []
    try:
        with (work / "bootstrap.log").open("wb") as output:
            subprocess.run([str(base / "bin/wine"), "wineboot.exe", "-u"], env=env,
                           stdout=output, stderr=subprocess.STDOUT, check=True, timeout=60)
        before = {f"{directory}/{name}": digest(prefix / "drive_c/windows" / directory / name)
                  for directory in ("system32", "syswow64")
                  for name in ("d3d8.dll", "d3d9.dll", "d3d10core.dll", "d3d11.dll", "dxgi.dll")}
        env.update({"WINEDATADIR": str(view / "share/wine"), "WINEDLLPATH": str(view / "lib/wine"),
                    "WINELOADER": str(view / "bin/wine"),
                    "WINEDLLOVERRIDES": "d3d9,d3d10core,d3d11,dxgi=b;mscoree,mshtml="})
        for architecture, pe_dir, windows_dir in (("x64", "x86_64-windows", "system32"), ("x86", "i386-windows", "syswow64")):
            for api in ("d3d11", "d3d9"):
                if args.api != "both" and args.api != api:
                    continue
                app = work / (architecture + "-" + api); app.mkdir()
                source = getattr(args, ("probe_" if api == "d3d11" else "probe_d3d9_") + architecture)
                shutil.copy2(source, app / "probe.exe")
                names = ("d3d11.dll", "dxgi.dll") if api == "d3d11" else ("d3d9.dll",)
                required = [view / "lib/wine" / pe_dir / name for name in names] + [app / "probe.exe"]
                expected = {name: digest(view / "lib/wine" / pe_dir / name) for name in names}
                gate = app / "identity"
                command = [str(view / "bin/wine"), str(app / "probe.exe")]
                if api == "d3d11":
                    command += ["--lifecycle", "--expected-d3d11-sha256", before[windows_dir + "/d3d11.dll"],
                                "--expected-dxgi-sha256", before[windows_dir + "/dxgi.dll"]]
                command += ["--identity-gate", "Z:" + str(gate).replace("/", "\\")]
                with (app / "result.jsonl").open("wb") as output, (app / "stderr.log").open("wb") as error:
                    process = subprocess.Popen(command, env=env, cwd=app, stdout=output, stderr=error)
                    start = time.monotonic()
                    while not gate.with_suffix(".ready").exists() and process.poll() is None and time.monotonic() - start < 8:
                        time.sleep(.05)
                    if not gate.with_suffix(".ready").exists():
                        raise RuntimeError("Probe did not reach its bounded identity gate")
                    pid = actual_process(required)
                    actual = mappings(pid)
                    if not set(map(str, required)) <= actual:
                        raise RuntimeError("The exact runtime-view images were not mapped")
                    if any(path.lower().endswith(("/wined3d.dll", "/winemetal.dll")) for path in actual):
                        raise RuntimeError("Another renderer was present before the shader compiler was loaded")
                    (app / "mappings-private.json").write_text(json.dumps(sorted(actual), indent=2) + "\n")
                    gate.with_suffix(".release").touch()
                    native_seen = set()
                    while process.poll() is None and time.monotonic() - start < 40:
                        native_seen.update(path for path in mappings(pid) if path.endswith(("libMoltenVK.dylib", "libvulkan.1.dylib")))
                        time.sleep(.05)
                    code = process.wait(timeout=2)
                    if code:
                        raise RuntimeError(f"{architecture} {api} returned {code}")
                if not set(map(str, native_paths.values())) <= native_seen:
                    raise RuntimeError("The reviewed MoltenVK and Vulkan loader images were not observed during rendering")
                records = [json.loads(line) for line in (app / "result.jsonl").read_text().splitlines() if line.startswith("{")]
                logical = {name: before[windows_dir + "/" + name] for name in names} if api == "d3d11" else None
                row = validate(records, architecture, api, logical_expected=logical)
                row.update({"selected_module_sha256": expected, "actual_host_mapping_verified": True,
                            "native_moltenvk_observed": True, "no_wined3d_at_renderer_identity_gate": True,
                            "observed_native_sha256": native_inventory,
                            "mapped_runtime_modules": [f"{pe_dir}/{name}" for name in names],
                            "prefix_graphics_dlls_unchanged": True})
                if api == "d3d11":
                    row["shader_compiler_dependency"] = compiler_dependency(view / "lib/wine" / pe_dir / "d3dcompiler_47.dll", architecture)
                summary.append(row)
        after = {name: digest(prefix / "drive_c/windows" / name) for name in before}
        if before != after:
            raise RuntimeError("Selecting the DXVK view changed a prefix graphics DLL")
        if selected_inventory != view_inventory(view):
            raise RuntimeError("The selected runtime-view PE files changed during validation")
        if native_inventory != {key: digest(path) for key, path in native_paths.items()}:
            raise RuntimeError("The observed native driver or loader changed during validation")
    finally:
        subprocess.run([str(base / "bin/wineserver"), "-k"], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=10)
    result = {"schema": 1, "loading_model": "wine-builtin-runtime-view-v1",
              "implementation": "caskrun-dxvk-hybrid-v1",
              "selected_runtime_module_sha256": selected_inventory,
              "runtime_modules_unchanged": True,
              "native_sha256": native_inventory, "native_images_unchanged": True,
              "base_wine_d3d8_retained": True, "base_wine_d3d8_sha256": base_d3d8,
              "prefix_graphics_dll_sha256_before": before,
              "prefix_graphics_dll_sha256_after": after,
              "runtime_view_validation_passed": True, "prefix_graphics_dlls_unchanged": True,
              "d3d9_validation_passed": args.api in ("both", "d3d9"),
              "d3d11_validation_passed": args.api in ("both", "d3d11"),
              "app_directories_contain_only_original_probes": True, "results": summary,
              "scope": "original API/readback/presentation/lifecycle probes; no game compatibility or performance claim"}
    (work / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    if not args.keep_evidence:
        owners = subprocess.run(["lsof", "-t", "+D", str(work)], capture_output=True, timeout=15)
        if owners.stdout.strip():
            raise RuntimeError("Owned fixture still has open files; cleanup refused")
        shutil.rmtree(work)


if __name__ == "__main__":
    main()
