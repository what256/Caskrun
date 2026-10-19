#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Check real Auto selection and one hardware-rendered pixel in a fresh owned bottle.

Uses the existing DXMTSession ownership, fake HOME, stdin cancellation and
prefix-scoped cleanup. No real bottles, accounts, network requests or windows.
Raw diagnostics remain ignored; success removes the disposable session.
"""
from __future__ import annotations
import argparse
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
PROJECT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("caskrun_auto_dxmt", Path(__file__).with_name("test-dxmt-runtime.py"))
dxmt = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = dxmt
spec.loader.exec_module(dxmt)
harness, HarnessError = dxmt.harness, dxmt.HarnessError


def pixel_record(data: bytes) -> dict:
    if len(data) > 1024 * 1024:
        raise HarnessError("The Auto probe log exceeded its limit")
    try:
        rows = [json.loads(line, object_pairs_hook=dxmt.unique_object)
                for line in data.decode("utf-8", "strict").splitlines() if line.startswith("{")]
    except (ValueError, UnicodeError):
        raise HarnessError("The Auto probe returned malformed evidence") from None
    keys = {"schema", "architecture", "hardware", "feature_level", "rgba", "winemetal_loaded",
            "wined3d_loaded", "hresult", "passed"}
    if len(rows) != 1 or not isinstance(rows[0], dict) or set(rows[0]) != keys:
        raise HarnessError("The Auto probe omitted or duplicated its result")
    row = rows[0]
    pixel = row["rgba"]
    if (row["schema"] != 1 or row["architecture"] != "x64" or row["hardware"] is not True
            or type(row["feature_level"]) is not int or row["feature_level"] not in {0xA000, 0xB000}
            or row["winemetal_loaded"] is not True or row["wined3d_loaded"] is not False
            or row["hresult"] != "0x00000000" or row["passed"] is not True
            or not isinstance(pixel, list) or len(pixel) != 4
            or any(type(value) is not int or not 0 <= value <= 255 for value in pixel)
            or any(abs(value - expected) > 3 for value, expected in zip(pixel, [32, 64, 128, 255]))):
        raise HarnessError("The Auto probe did not produce the expected hardware-rendered pixel")
    return row


def run(arguments) -> int:
    build = harness.real_path(PROJECT / ".build", directory=True)
    runtime = harness.real_path(arguments.runtime.absolute(), directory=True)
    receipt = dxmt.candidate_receipt(runtime, build)
    core = dxmt.owned_artifact(arguments.core, build, executable=True)
    wine = dxmt.owned_artifact(runtime / "bin/wine", build, executable=True)
    probe = dxmt.owned_artifact(arguments.probe, build)
    if not receipt["switchable"] or dxmt.pe_architecture(probe) != 0x8664:
        raise HarnessError("Use the reviewed switchable runtime and original x64 probe")
    session = dxmt.DXMTSession.setup(PROJECT, core, wine)
    summary = {"schema": 1, "passed": False, "cleanup": "pending", "gameplay": "untested",
               "probe_sha256": harness.digest(probe),
               "components_receipt_sha256": receipt["components_receipt_sha256"]}
    session.summary["auto_graphics"] = summary
    session.manifest["dxmt_bottles"] = []
    session.save()
    failure = None
    with session.locked():
        try:
            bottle = session.core_call(["create", "Original Auto graphics probe"])["id"]
            if not isinstance(bottle, str) or not re.fullmatch(r"[a-f0-9-]{36}", bottle):
                raise HarnessError("The disposable bottle identifier was invalid")
            session.manifest["dxmt_bottles"].append(bottle)
            session.save()
            initialized = session.core_call(["initialize", bottle])
            prefix = session.storage / "bottles" / bottle / "prefix"
            if initialized.get("prefix_path") != str(prefix) or initialized.get("initialized") is not True:
                raise HarnessError("The fresh Auto test bottle was not initialized")
            stopped = session.core_call(["quit-bottle", bottle])
            if stopped.get("server_stopped") is not True or stopped.get("tracked_after") != 0:
                raise HarnessError("The initialized test bottle did not become idle")
            folder = harness.real_path(prefix / "drive_c", directory=True) / "Probe"
            folder.mkdir(mode=0o700)
            fixture = folder / "AutoGraphics.exe"
            shutil.copyfile(probe, fixture)
            fixture.chmod(0o600)
            if harness.digest(fixture) != summary["probe_sha256"]:
                raise HarnessError("The original probe changed while being copied")
            session.core_call(["add-app", bottle, "Original Auto graphics probe", str(fixture)])
            before = session.core_call(["bottle-graphics", bottle])
            if before.get("selected_backend") != "auto" or before.get("effective_backend") != "wined3d":
                raise HarnessError("The fresh bottle did not begin with the unchanged Auto baseline")
            launched = session.core_call(["launch-wait", bottle, str(fixture), "--timeout-seconds", "45"], deadline=90)
            if launched.get("exit_code") != 0 or launched.get("signal") is not None or launched.get("timed_out") is not False:
                raise HarnessError("The original Auto graphics probe failed or timed out")
            log = harness.real_path(Path(launched["log_path"]))
            if log.parent != prefix.parent / "logs" or log.stat().st_size > 1024 * 1024:
                raise HarnessError("The Auto probe returned an unexpected log")
            summary["pixel"] = pixel_record(log.read_bytes())
            state = session.core_call(["bottle-graphics", bottle])
            if (state.get("selected_backend") != "auto" or state.get("effective_backend") != "dxmt"
                    or state.get("automatic_reason") != "d3d11-detected" or state.get("dxmt_available") is not True):
                raise HarnessError("Auto did not select the verified DXMT backend from the executable's imports")
            summary.update(selected_backend="auto", effective_backend="dxmt", automatic_reason="d3d11-detected", passed=True)
        except (HarnessError, OSError, UnicodeError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
            failure = str(error) if isinstance(error, HarnessError) else "The isolated Auto graphics check could not finish"
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
            except (HarnessError, OSError, subprocess.TimeoutExpired):
                summary.update(passed=False, cleanup="preserved_owners_unconfirmed")
                session.save()
    print(json.dumps(summary, sort_keys=True))  # No paths, accounts or process identities.
    if summary["cleanup"].startswith("preserved"):
        print("Owned evidence retained; clean up with: python3 scripts/test-auto-graphics-runtime.py cleanup " + session.session_id, file=sys.stderr)
    return 0 if summary["passed"] and summary["cleanup"] == "removed" else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    start = commands.add_parser("run")
    for name in ("core", "runtime", "probe"):
        start.add_argument("--" + name, type=Path, required=True)
    commands.add_parser("cleanup").add_argument("session")
    arguments = parser.parse_args(argv)
    try:
        if arguments.action == "run":
            return run(arguments)
        session = dxmt.DXMTSession(PROJECT, arguments.session)
        if "auto_graphics" not in session.summary or "dxmt_bottles" not in session.manifest:
            raise HarnessError("This is not an owned Auto graphics test session")
        with session.locked():
            session.stop_owned_prefixes()
            session.cleanup()
        print("Owned Auto fixture removed after prefix-scoped stop and zero-owner checks.")
        return 0
    except (HarnessError, OSError, UnicodeError, ValueError, TypeError, KeyError) as error:
        print(str(error) if isinstance(error, HarnessError) else "Auto validation setup failed; no user bottles were selected", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
