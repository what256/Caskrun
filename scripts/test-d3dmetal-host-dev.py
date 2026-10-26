#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Bounded original Windows API test of a separately prepared private host.

This does not build, download, stage or modify any runtime image. It requires
an explicit local candidate receipt after review of the paired Wine adapters.
Only a fresh owned prefix is used; a live mapped-file gate precedes DXGI factory
creation. The unmodified, separately licensed Apple component stays private.
Adapter/API success is not a packaging approval or game-compatibility claim.
"""
from __future__ import annotations

import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parents[1]
WORK = PROJECT / ".build/d3dmetal-bridge-integration"
APPLE = PROJECT / ".build/private-components/apple-d3dmetal-4.0-beta3"
APPLE_RECEIPT = "f4ee74e1881f107d908b0365c8499d92f4c0ce61216d01403fbec7d96d84ca2d"
PATCHES = (
    "d3dmetal-wine-11.0-non-native-regions.patch",
    "d3dmetal-wine-11.0-native-import-abi.patch",
    "d3dmetal-wine-11.0-macdrv-host.patch",
)
PROBE_SOURCES = ("d3dmetal-smoke.c", "d3d12-smoke.c")
HOST_FILES = {
    "ntdll.dll": "runtime/lib/wine/x86_64-windows/ntdll.dll",
    "ntdll.so": "runtime/lib/wine/x86_64-unix/ntdll.so",
    "winemac.drv": "runtime/lib/wine/x86_64-windows/winemac.drv",
    "winemac.so": "runtime/lib/wine/x86_64-unix/winemac.so",
}
EXECUTABLES = {
    "bin/_dispatch": "48fa40c71c6d9332573c378e402d94b8de0556d56b973339fe52f63c4a304b3a",
    "runtime/bin/wine": "061c7fab3177ca39131664c734c757993fed380df04d7c761d75c49d205557a3",
    "runtime/bin/wineserver": "5427fad31731e8f427e93756c47712745deb3e677a8f7971f0da0a9709eeab66",
}


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


runner = module("d3dmetal_host_owned_session", PROJECT / "scripts/test-dxmt-d3d12-dev.py")
harness = runner.harness
require = runner.require


def local_path(path, directory=False):
    path = harness.real_path(path.absolute(), directory=directory)
    require(path.is_relative_to(PROJECT / ".build"), "Use only private project-local artifacts")
    return path


def require_idle(package):
    table = harness.process_table()
    require(not any(str(package) in value["command"] for pid, value in table.items()
                    if pid != os.getpid()), "The private candidate has a live process owner")
    result = subprocess.run(["/usr/sbin/lsof", "-nP", "-Fpc", "+D", str(package)],
                            capture_output=True, timeout=30, check=False)
    require(result.returncode == 1 and not result.stdout and not result.stderr,
            "Private candidate idleness was not established")


def verify_official_component(package):
    receipt_path = local_path(APPLE / "component-receipt.json")
    require(harness.digest(receipt_path) == APPLE_RECEIPT,
            "The separately reviewed Apple input receipt changed")
    receipt = harness.read_json(receipt_path)
    require(receipt["source_image_sha256"] ==
            "afa311da371f5b6dd970b45378a3d3df567cc9e535818f56f9b3f9d4c9537c1d"
            and receipt["license_reviewed_sha256"] ==
            "5abb2d059be217663b00e8fd37e14411d374e11d17e3b744eebd49b8d17118c8",
            "The local evaluation input or accepted license differs")
    for item in receipt["files"]:
        relative = Path(item["path"])
        require(not relative.is_absolute() and ".." not in relative.parts,
                "Unexpected component receipt path")
        source = APPLE / relative
        if item["type"] == "directory":
            require(source.is_dir(), "An official input directory is absent")
        elif item["type"] == "symlink":
            require(source.is_symlink() and os.readlink(source) == item["target"],
                    "An official input link changed")
        else:
            require(not source.is_symlink() and harness.digest(source) == item["sha256"],
                    "An official input file changed")
        if relative.parts[0] != "lib":
            continue
        target = package / "runtime" / relative
        if item["type"] == "directory":
            require(target.is_dir(), "An official candidate directory is absent")
        elif item["type"] == "symlink":
            require(target.is_symlink() and os.readlink(target) == item["target"],
                    "An official candidate link changed")
        else:
            require(not target.is_symlink() and harness.digest(target) == item["sha256"],
                    "An official candidate file changed")
    framework = package / "runtime/lib/external/D3DMetal.framework"
    subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(framework)],
                   capture_output=True, check=True, timeout=15)
    return framework / "Versions/A/D3DMetal", package / "runtime/lib/external/libd3dshared.dylib"


def checked_inputs(package, probe, proof):
    require(package == WORK / "private-runtime", "Use the separate owned host candidate")
    require_idle(package)
    for relative, expected in EXECUTABLES.items():
        require(harness.digest(local_path(package / relative)) == expected,
                "A reviewed Wine execution input changed")
    require(type(proof.get("schema")) is int and proof.get("schema") == 1
            and proof.get("adapter_validation_passed") is True,
            "Candidate host adapters need their reviewed functional-test receipt")
    require(set(proof.get("source_patches", {})) == set(PATCHES),
            "Candidate host patch expectations are incomplete")
    for name in PATCHES:
        require(harness.digest(PROJECT / "runtime-patches" / name) == proof["source_patches"][name],
                "A reviewed host adapter patch changed")
    require(set(proof.get("host_files", {})) == set(HOST_FILES),
            "The paired PE/native host expectations are incomplete")
    artifacts = {}
    for name, relative in HOST_FILES.items():
        path = local_path(package / relative)
        require(harness.digest(path) == proof["host_files"][name], "A reviewed paired host image changed")
        artifacts[name] = path
    require(runner.contract.pe_architecture(probe) == 0x8664
            and harness.digest(probe) == proof.get("probe_sha256"),
            "The original Windows probe differs from the reviewed input")
    require(set(proof.get("probe_source_sha256", {})) == set(PROBE_SOURCES),
            "The reviewed probe source expectations are incomplete")
    for name in PROBE_SOURCES:
        require(harness.digest(PROJECT / "scripts/probes" / name) == proof["probe_source_sha256"][name],
                "An original probe source changed")
    framework, shared = verify_official_component(package)
    artifacts.update({"D3DMetal": framework, "libd3dshared.dylib": shared})
    for name in ("dxgi.dll", "d3d12.dll"):
        artifacts[name] = local_path(package / "runtime/lib/wine/x86_64-windows" / name)
    return artifacts


def probe_passed(code, records):
    required = runner.BASE_CHECKS | {"clear_pixels_match", "owned_hidden_window", "owned_window_dc"}
    checks = [item for item in records if item.get("event") == "check"]
    counts = Counter(item.get("name") for item in checks)
    return (code == 0 and bool(records) and records[-1].get("event") == "result"
            and records[-1].get("passed") is True
            and records[-1].get("game_compatibility_verified") is False
            and all(item.get("passed") is True for item in checks)
            and all(counts[name] == 1 for name in required))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--candidate-proof", type=Path, required=True)
    args = parser.parse_args(argv)
    result = {"schema": 1, "test": "official_apple_with_original_wine_host_adapters",
              "passed": False, "game_compatibility_verified": False,
              "swapchain_verified": False, "installed_or_user_data_changed": False}
    session = None
    stage = "inputs"
    try:
        package, probe = local_path(args.package, True), local_path(args.probe)
        proof = harness.read_json(local_path(args.candidate_proof))
        artifacts = checked_inputs(package, probe, proof)
        expected = {name: harness.digest(path) for name, path in artifacts.items()}
        runner.EXPERIMENT = WORK
        session = runner.Session(package)
        (WORK / "active-combined-session.json").write_text(json.dumps({
            "root": str(session.root), "prefix": str(session.prefix)}) + "\n")
        wine = package / "bin/wine"
        env = session.environment("wined3d")
        stage = "bootstrap"
        code, _ = session.invoke([wine, "wineboot.exe", "-u"], env, "bootstrap", 60)
        require(code == 0, "Private Wine bootstrap failed")
        # Suppress crash/debugger UI in this owned prefix only. This neither
        # changes the exception nor turns a failed process into a test pass.
        for key, value in (("Auto", "1"), ("Debugger", "false.exe")):
            code, _ = session.invoke([wine, "reg.exe", "add",
                "HKLM\\Software\\Microsoft\\Windows NT\\CurrentVersion\\AeDebug",
                "/v", key, "/t", "REG_SZ", "/d", value, "/f"], env, "debugger-" + key, 10)
            require(code == 0, "Could not suppress fixture crash UI")
        session.stop_and_check_idle()
        fixture = session.root / "app/d3dmetal-smoke.exe"
        shutil.copyfile(probe, fixture)
        gate = session.root / "app/identity"
        env.update({"CASKRUN_D3DMETAL_SHARED_LIBRARY": str(artifacts["libd3dshared.dylib"]),
            "D3DMETAL_FRAMEWORK_PATH": str(artifacts["D3DMetal"]),
            "WINEDLLOVERRIDES": "winemenubuilder.exe=;mscoree=;mshtml=;dxgi,d3d11,d3d12,d3d12core=b",
            "WINEDEBUG": "-all,trace+module"})
        command = [wine, fixture, "--expected-d3d12-sha256",
            harness.digest(session.prefix / "drive_c/windows/system32/d3d12.dll"),
            "--expected-dxgi-sha256", harness.digest(session.prefix / "drive_c/windows/system32/dxgi.dll"),
            "--identity-gate", "Z:" + str(gate).replace("/", "\\")]

        def inspect():
            runner.contract.verify_live_mappings(session, fixture, artifacts, expected)
            session.manifest["tracked"].update(session._mapped_probe_processes)
            result["actual_host_mapping_verified"] = True

        stage = "windows_api_probe"
        code, data = session.invoke(command, env, "probe", 35, gate, inspect)
        records = [json.loads(line) for line in data.decode().splitlines() if line.startswith("{")]
        result.update(exit_code=code, records=records, selected_sha256=expected,
                      passed=probe_passed(code, records))
        text = (session.raw / "probe.stderr").read_text(errors="replace")
        result["region_support_enabled"] = "Apple non-native region support enabled" in text
        result["application_region_registered"] = "Application PE region registered with Apple support" in text
        # Public Win32 initialization must be proven, not merely requested.
        passed_names = {item.get("name") for item in records if item.get("passed") is True}
        result["user_driver_initialized"] = {"owned_hidden_window", "owned_window_dc"} <= passed_names
        result["passed"] = (result["passed"] and result["user_driver_initialized"]
                            and result["actual_host_mapping_verified"]
                            and result["region_support_enabled"]
                            and result["application_region_registered"])
        stage = "post_probe_integrity"
        verify_official_component(package)
        for name, path in artifacts.items():
            require(harness.digest(path) == expected[name], "A paired host or Apple image changed during the test")
        result["post_probe_image_integrity_verified"] = True
    except BaseException as error:
        result.update(passed=False, failure_stage=stage, error=str(error))
    finally:
        if session is not None:
            try:
                session.stop_and_check_idle()
                session._validate_owner()
                # Retain bounded private API/mapping evidence, never a full
                # prefix. These files may contain local paths and are not
                # publication artifacts.
                evidence = Path(tempfile.mkdtemp(prefix="combined-proof-", dir=WORK))
                result["private_evidence"] = evidence.name
                total = 0
                for path in sorted(session.raw.iterdir()):
                    if not path.name.startswith(("probe.", "mapped-")):
                        continue
                    harness.real_path(path)
                    total += path.stat().st_size
                    require(total <= 12 * 1024 * 1024, "Private proof exceeds its bounded limit")
                    shutil.copyfile(path, evidence / path.name)
                require(shutil.rmtree.avoids_symlink_attacks, "Safe fixture removal is unavailable")
                shutil.rmtree(session.root)
                result["cleanup"] = "removed_after_no_owners_verified"
            except BaseException as error:
                result["cleanup_error"] = str(error)
                result["passed"] = False
            result["passed"] = result["passed"] and result.get("cleanup") == "removed_after_no_owners_verified"
        (WORK / "combined-api-result.private.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({name: value for name, value in result.items()
                          if name not in ("records", "selected_sha256", "error", "cleanup_error")}, indent=2))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
