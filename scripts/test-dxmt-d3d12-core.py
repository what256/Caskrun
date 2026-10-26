#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Verify packaged D3D12 through Core, saved DXMT and a real idle migration.

Uses original x64 GPU probes in one disposable owned bottle. No user bottles,
storefronts or accounts. A host identity gate proves the selected builtin DLL
and native bridge in the live probe. Logs remain in ignored private storage;
the summary contains no account, device, bottle or process identifiers.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import sys
import time

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, PROJECT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


dxmt = load("d3d12_core_dxmt", "test-dxmt-runtime.py")
dev = load("d3d12_core_probe", "test-dxmt-d3d12-dev.py")
package = load("d3d12_core_package", "check-runtime-package.py")
Error = dxmt.HarnessError
harness = dxmt.harness


def require(condition, message):
    if not condition:
        raise Error(message)


class Session(dxmt.DXMTSession):
    def environment(self):
        environment = super().environment()
        if getattr(self, "previous_wine", None) is not None:
            environment["CASKRUN_WINE"] = str(self.previous_wine)
        return environment

    def original_probe(self, bottle, fixture, arguments, artifacts, draw):
        gate = self.root / ("d3d12-gate-" + secrets.token_hex(12))
        expected = {name: harness.digest(path) for name, path in artifacts.items()}
        command = ["launch-wait", bottle, str(fixture), "--timeout-seconds", "60", "--",
                   *arguments, "--identity-gate", "Z:" + str(gate).replace("/", "\\")]
        with ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(self.core_call, command, deadline=120)
            deadline = time.monotonic() + 75
            while not gate.with_suffix(".ready").exists():
                if future.done():
                    future.result()
                    raise Error("The original Core probe exited before its identity gate")
                require(time.monotonic() < deadline, "Core did not reach the bounded identity gate")
                time.sleep(0.05)
            dependency = dev.mapped_gate(self, fixture, artifacts, expected, draw)
            self.verify_busy_switch_refused(bottle)
            # Refusal must leave the same exact process and selected files alive.
            dev.mapped_gate(self, fixture, artifacts, expected, draw)
            fd = os.open(gate.with_suffix(".release"),
                         os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            os.close(fd)
            launched = future.result()
        require(launched.get("exit_code") == 0 and launched.get("signal") is None
                and launched.get("timed_out") is False, "Core-selected GPU probe failed")
        log = harness.real_path(Path(launched["log_path"]))
        require(log.parent == self.storage / "bottles" / bottle / "logs"
                and log.stat().st_size <= dxmt.MAX_LOG, "Unexpected Core probe log")
        return log.read_bytes(), expected, dependency


def run(args):
    build = harness.real_path(PROJECT / ".build", directory=True)
    runtime = harness.real_path(args.runtime.absolute(), directory=True)
    previous = harness.real_path(args.previous_runtime.absolute(), directory=True)
    suffix = ("Caskrun.app", "Contents", "Resources", "wine-runtime")
    require(runtime.parts[-4:] == previous.parts[-4:] == suffix,
            "Use real app-bundled runtimes; migration must retain its app-path guard")
    policy = (PROJECT / "core/src/runtime_migration.rs").read_text()
    identity = package.verify_package(runtime, policy)
    previous_identity = package.verify_package(previous, policy)
    require(identity != previous_identity, "Use distinct reviewed migration packages")
    receipt = harness.read_json(runtime / "components.json")["dxmt_d3d12"]
    pe = runtime / "components/dxmt/runtime/lib/wine/x86_64-windows"
    require(harness.digest(pe / "d3d12.dll") == receipt["packaged_dll_sha256"],
            "Packaged D3D12 differs from its receipt")
    core = dxmt.owned_artifact(args.core, build, executable=True)
    probes = {name: dxmt.owned_artifact(getattr(args, "probe_" + name), build)
              for name in ("draw", "present")}
    require(all(dxmt.pe_architecture(path) == 0x8664 for path in probes.values()),
            "Only original x64 D3D12 probes are supported")
    session = Session.setup(PROJECT, core, runtime / "bin/wine")
    session.runtime = runtime
    session.manifest.update(dxmt_bottles=[], dxmt_legacy_layout=False)
    summary = {"schema": 1, "component": "DXMT D3D12", "passed": False,
               "runtime_identity": identity, "previous_runtime_identity": previous_identity,
               "game_compatibility_verified": False, "onscreen_pixels_verified": False,
               "results": {}, "cleanup": "pending"}
    session.summary["dxmt_d3d12"] = summary
    session.save()
    failure = None
    try:
        session.previous_wine = previous / "bin/wine"
        bottle = session.core_call(["create", "Original D3D12 Integration"])["id"]
        require(isinstance(bottle, str) and dxmt.re.fullmatch(r"[a-f0-9-]{36}", bottle),
                "Invalid disposable bottle identifier")
        session.manifest["dxmt_bottles"].append(bottle)
        session.save()
        session.core_call(["initialize", bottle])
        session.core_call(["set-bottle-graphics", bottle, "dxmt"])
        session.core_call(["set-bottle-dxmt-options", bottle, "60", "100"])
        session.stop_owned_prefixes()
        directory = session.storage / "bottles" / bottle
        prefix = directory / "prefix"
        pin_before = harness.read_json(directory / "prefix-record.json")
        require(pin_before["runtime_identity"] == previous_identity, "Prior runtime was not initialized")
        preference = (directory / "graphics.json").read_bytes()
        sentinel = prefix / "drive_c/caskrun-original-save.txt"
        sentinel.write_bytes(b"original fixture save preserved\n")
        logical = {name: harness.digest(prefix / "drive_c/windows/system32" / name)
                   for name in ("d3d12.dll", "dxgi.dll", "d3dcompiler_47.dll")}
        session.previous_wine = None
        session.core_call(["initialize", bottle])
        require(harness.read_json(directory / "prefix-record.json")["runtime_identity"] == identity
                and (directory / "graphics.json").read_bytes() == preference
                and sentinel.read_bytes() == b"original fixture save preserved\n",
                "Idle runtime migration did not preserve preferences or fixture data")
        state = session.core_call(["bottle-graphics", bottle])
        require(state["selected_backend"] == state["effective_backend"] == "dxmt"
                and state.get("dxmt_d3d12_available") is True
                and state.get("dxmt_frame_rate") == 60
                and state.get("dxmt_spatial_scale") == 100,
                "Core did not expose the reviewed opt-in D3D12 capability")
        summary.update(idle_migration_preserved_preferences_and_save=True,
                       saved_frame_target=60, saved_spatial_scale=100)
        for name, source in probes.items():
            fixture = session.root / ("d3d12-" + name + ".exe")
            shutil.copyfile(source, fixture)
            artifacts = {dll: pe / dll for dll in ("d3d12.dll", "dxgi.dll", "winemetal.dll")}
            artifacts["winemetal.so"] = runtime / "components/dxmt/runtime/lib/wine/x86_64-unix/winemetal.so"
            arguments = ["--expected-d3d12-sha256", logical["d3d12.dll"],
                         "--expected-dxgi-sha256", logical["dxgi.dll"]]
            if name == "draw":
                artifacts["d3dcompiler_47.dll"] = pe / "d3dcompiler_47.dll"
                arguments += ["--expected-compiler-sha256", logical["d3dcompiler_47.dll"]]
            data, mapped, dependency = session.original_probe(bottle, fixture, arguments, artifacts, name == "draw")
            parsed = dev.parse_result(data, name == "draw", 0, name == "present")
            parsed.update(selected_module_sha256=mapped, actual_host_mapping_verified=True,
                          compiler_dependency_mapped=dependency, busy_renderer_change="refused")
            summary["results"][name] = parsed
            session.stop_owned_prefixes()
        require((directory / "graphics.json").read_bytes() == preference
                and sentinel.read_bytes() == b"original fixture save preserved\n"
                and all(harness.digest(prefix / "drive_c/windows/system32" / name) == digest
                        for name, digest in logical.items()),
                "Probes modified a preference, save or prefix graphics DLL")
        session.core_call(["set-bottle-graphics", bottle, "wined3d"])
        state = session.core_call(["bottle-graphics", bottle])
        require(state["selected_backend"] == state["effective_backend"] == "wined3d",
                "WineD3D switchback failed")
        require(package.verify_package(runtime, policy) == identity,
                "The reviewed runtime changed during integration testing")
        summary.update(passed=True, prefix_graphics_dlls_unchanged=True,
                       wined3d_switchback_verified=True)
    except Exception as error:
        failure = error
        summary["reason"] = str(error) if isinstance(error, Error) else "Integration check failed; private diagnostics preserved"
    finally:
        try:
            session.stop_owned_prefixes()
            summary["cleanup"] = "verified_idle"
            session.save()
            if failure is None:
                session.cleanup()
                summary["cleanup"] = "removed"
        except Exception:
            summary.update(passed=False, cleanup="preserved_owners_unconfirmed")
        target = PROJECT / ".build/dxmt-d3d12-integration/core-integration-summary.json"
        harness.write_json(target, summary)
    print(json.dumps(summary, indent=2))
    return 0 if summary["passed"] and summary["cleanup"] == "removed" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("core", "runtime", "previous-runtime", "probe-draw", "probe-present"):
        parser.add_argument("--" + name, type=Path, required=True)
    return run(parser.parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
