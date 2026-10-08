#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise optional DXMT controls in owned fresh x64/x86 test bottles.

Reuses the original exact mapped-module, lifecycle and cleanup gates. Pacing
records measure API submission, not display scanout, actual game FPS or speed.
All paths, runtime logs and process ownership records remain ignored locally.
"""
from collections import Counter
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("caskrun_dxmt_options_base", Path(__file__).with_name("test-dxmt-runtime.py"))
base = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = base
spec.loader.exec_module(base)
OriginalSession = base.DXMTSession
original_parser = base.parse_probe
OPTIONS = (0, 100)
PACING_CHECKS = Counter({name: 1 for name in (
    "pacing_hwnd", "pacing_swapchain", "lifecycle_get_backbuffer", "pacing_render_target", "pacing_timer", "pacing_finished")})


def parse_options_probe(data, architecture, expected, **kwargs):
    if kwargs.get("identity_only"):
        return original_parser(data, architecture, expected, **kwargs)
    if len(data) > base.MAX_LOG:
        raise base.HarnessError("Optional-control output exceeds the bounded log limit")
    filtered, checks, timing = [], Counter(), None
    pacing = False
    for line in data.decode("utf-8", "strict").splitlines():
        if not line.startswith("{"):
            filtered.append(line)
            continue
        row = json.loads(line, object_pairs_hook=base.unique_object)
        if not isinstance(row, dict):
            raise base.HarnessError("Optional-control evidence contains a non-object record")
        if row.get("event") == "check" and row.get("name") == "pacing_hwnd":
            pacing = True
        if pacing and row.get("event") == "check":
            if row.get("feature_level") != "11_0" or row.get("passed") is not True or row.get("hresult") != "0x00000000":
                raise base.HarnessError("An optional-control pacing operation failed")
            checks[row.get("name")] += 1
        elif row.get("event") == "pacing":
            if timing is not None or row != {
                "event": "pacing", "feature_level": "11_0", "frames": 60,
                "elapsed_us": row.get("elapsed_us"), "measurement": "api_submission_not_display_fps"
            } or type(row.get("frames")) is not int or type(row.get("elapsed_us")) is not int or not 0 < row["elapsed_us"] < 20_000_000:
                raise base.HarnessError("Optional-control timing has an unexpected shape")
            timing = row
        else:
            filtered.append(line)
    if checks != PACING_CHECKS or timing is None:
        raise base.HarnessError("Optional-control evidence omitted a required timing stage")
    result = original_parser(("\n".join(filtered) + "\n").encode(), architecture, expected, **kwargs)
    result["dxmt_options"] = {"frame_rate": OPTIONS[0], "spatial_scale": OPTIONS[1],
                              "submitted_frames": 60, "elapsed_us": timing["elapsed_us"],
                              "display_fps": "not_measured", "output_pixels": "not_captured"}
    return result


class OptionsSession(OriginalSession):
    def environment(self):
        result = super().environment()
        result["DXMT_LOG_LEVEL"] = "info"
        if OPTIONS != (0, 100):
            # Explicit controls must prevail even if the prior config ended in
            # a nonmatching section and an inherited value requests another rate.
            config = self.root / "dxmt.conf"
            config.write_text("[not-the-owned-probe.exe]\nd3d11.preferredMaxFrameRate=240;\n", encoding="utf-8")
            result["DXMT_CONFIG"] = "d3d11.preferredMaxFrameRate=239;"
            result["DXMT_METALFX_SPATIAL_SWAPCHAIN"] = "1" if OPTIONS[1] == 100 else "0"
        return result

    def core_call(self, arguments, deadline=120):
        result = super().core_call(arguments, deadline)
        if arguments[:1] == ["set-bottle-graphics"] and arguments[-1] == "dxmt":
            settings = super().core_call(["set-bottle-dxmt-options", arguments[1], str(OPTIONS[0]), str(OPTIONS[1])])
            if (settings.get("selected_backend") != "dxmt" or settings.get("dxmt_available") is not True
                    or settings.get("dxmt_frame_rate") != OPTIONS[0] or settings.get("dxmt_spatial_scale") != OPTIONS[1]):
                raise base.HarnessError("The core did not save the exact optional DXMT controls")
        return result

    def verify_busy_switch_refused(self, bottle):
        super().verify_busy_switch_refused(bottle)
        directory = self.storage / "bottles" / bottle
        before = {name: (directory / name).read_bytes() for name in ("graphics.json", "prefix-record.json")}
        environment = self.environment()
        environment.pop("CASKRUN_CANCEL_STDIN", None)
        refused = subprocess.run([str(self.core), "set-bottle-dxmt-options", bottle, "60", "100"],
            cwd=self.root, env=environment, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=False)
        if refused.returncode == 0 or b"Close Windows apps" not in refused.stderr:
            raise base.HarnessError("The live optional-control change was not refused")
        if any((directory / name).read_bytes() != data for name, data in before.items()):
            raise base.HarnessError("A refused live optional-control change modified preferences or the pin")

    def gated_probe(self, bottle, fixture, arguments, artifacts, expected, **kwargs):
        if "--identity-only" not in arguments:
            arguments = [*arguments, "--pacing-frames", "60"]
        launched, proof = super().gated_probe(bottle, fixture, arguments, artifacts, expected, **kwargs)
        if "--identity-only" not in arguments:
            text = Path(launched["log_path"]).read_text(encoding="utf-8", errors="strict")
            if "MetalFX spatial scaler is not supported on this device" in text:
                raise base.HarnessError("MetalFX was requested but the engine reported a fallback")
            if OPTIONS != (0, 100):
                scale = f"{OPTIONS[1] // 100}.{OPTIONS[1] % 100:02}"
                wanted = f"Found config env: d3d11.preferredMaxFrameRate={OPTIONS[0]};d3d11.metalSpatialUpscaleFactor={scale};"
                if "Found config file: NUL" not in text or wanted not in text:
                    raise base.HarnessError("The actual translator did not receive the exact managed controls")
        return launched, proof


def run_options(arguments):
    global OPTIONS
    build = base.harness.real_path(base.PROJECT / ".build", directory=True)
    runtime = base.harness.real_path(arguments.runtime.absolute(), directory=True)
    receipt = base.candidate_receipt(runtime, build)
    if not receipt["switchable"]:
        raise base.HarnessError("Optional controls require the reviewed switchable package")
    core = base.owned_artifact(arguments.core, build, executable=True)
    wine = base.owned_artifact(runtime / "bin/wine", build, executable=True)
    probes = {architecture: base.owned_artifact(getattr(arguments, "probe_" + architecture), build)
              for architecture in base.ARCHITECTURES}
    for architecture, probe in probes.items():
        if base.pe_architecture(probe) != base.ARCHITECTURES[architecture][1]:
            raise base.HarnessError("An original probe has the wrong Windows architecture")
    session = OptionsSession.setup(base.PROJECT, core, wine)
    summary = {"schema":1,"component":"DXMT optional controls","passed":False,
               "runtime_receipt_sha256":receipt["components_receipt_sha256"],
               "architectures":{},"display_fps":"not_measured","visible_pixels":"not_captured",
               "gameplay":"untested","cleanup":"pending"}
    session.summary["dxmt"] = summary
    session.manifest["dxmt_bottles"] = []
    session.manifest["dxmt_legacy_layout"] = False
    session.save()
    failure = None
    settings = [(30,100),(60,100),(0,125),(0,150),(0,200)] if arguments.matrix else [OPTIONS]
    try:
        for architecture, probe in probes.items():
            bottle = session.core_call(["create","Original DXMT options " + architecture])["id"]
            if not base.re.fullmatch(r"[a-f0-9-]{36}",bottle):
                raise base.HarnessError("Invalid disposable bottle identifier")
            session.manifest["dxmt_bottles"].append(bottle)
            session.save()
            initialized = session.core_call(["initialize",bottle])
            prefix = session.storage / "bottles" / bottle / "prefix"
            if initialized.get("prefix_path") != str(prefix) or initialized.get("initialized") is not True:
                raise base.HarnessError("The expected fresh bottle was not initialized")
            fixture = session.root / ("probe-" + architecture + ".exe")
            base.shutil.copyfile(probe,fixture)
            fixture.chmod(0o600)
            digest = base.harness.digest(fixture)
            if digest != base.harness.digest(probe):
                raise base.HarnessError("The original probe changed while copying")
            # Use the ordinary renderer setter once, then change options while
            # independently idle. No user prefix or runtime view is modified.
            OPTIONS = (0,100)
            selected = session.core_call(["set-bottle-graphics",bottle,"dxmt"])
            if selected.get("dxmt_available") is not True:
                raise base.HarnessError("Verified DXMT was not available")
            results = []
            for OPTIONS in settings:
                selected = session.core_call(["set-bottle-dxmt-options",bottle,str(OPTIONS[0]),str(OPTIONS[1])])
                if selected.get("dxmt_frame_rate") != OPTIONS[0] or selected.get("dxmt_spatial_scale") != OPTIONS[1]:
                    raise base.HarnessError("The selected optional controls were not saved")
                logical = receipt["baseline"][architecture]
                argv = ["--expected-d3d11-sha256",logical["d3d11.dll"],
                        "--expected-dxgi-sha256",logical["dxgi.dll"],"--lifecycle"]
                launched, proof = session.gated_probe(bottle,fixture,argv,receipt["mapped"][architecture],
                    receipt["mapped_hashes"][architecture],check_busy_refusal=True)
                if launched.get("exit_code") != 0 or launched.get("timed_out") is not False or launched.get("signal") is not None:
                    raise base.HarnessError("The optional-control probe failed or timed out")
                log = base.harness.real_path(Path(launched["log_path"]))
                if log.parent != prefix.parent / "logs":
                    raise base.HarnessError("Unexpected optional-control log location")
                parsed = parse_options_probe(log.read_bytes(),architecture,logical,lifecycle=True,
                    mapped_expected=receipt["mapped_hashes"][architecture],mapping_evidence=proof)
                if base.harness.digest(fixture) != digest:
                    raise base.HarnessError("The executed probe changed during verification")
                results.append(parsed)
                session.stop_owned_prefixes()
                session.save()
            # Both Off restores the reviewed baseline. Switching renderer must
            # preserve (rather than silently discard) the saved DXMT options.
            session.core_call(["set-bottle-dxmt-options",bottle,"0","100"])
            session.core_call(["set-bottle-graphics",bottle,"auto"])
            state = session.core_call(["bottle-graphics",bottle])
            if state.get("selected_backend") != "auto" or state.get("dxmt_frame_rate") != 0 or state.get("dxmt_spatial_scale") != 100:
                raise base.HarnessError("Returning to baseline did not preserve the expected controls")
            summary["architectures"][architecture] = results
            session.save()
        summary["passed"] = True
    except (base.HarnessError,OSError,UnicodeError,KeyError,TypeError,subprocess.TimeoutExpired) as error:
        failure = str(error) if isinstance(error,base.HarnessError) else "Owned optional-control validation could not finish"
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
        except (base.HarnessError,OSError,subprocess.TimeoutExpired):
            summary["passed"] = False
            summary["cleanup"] = "preserved_owners_unconfirmed"
            session.save()
    print(json.dumps(summary,sort_keys=True))
    if summary["cleanup"].startswith("preserved"):
        print("Owned evidence preserved in .build/launcher-validation-" + session.session_id,file=sys.stderr)
    return 0 if summary["passed"] and summary["cleanup"] == "removed" else 1


def main():
    global OPTIONS
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("core", "runtime", "probe-x64", "probe-x86"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--matrix", action="store_true", help="Test 30/60 FPS targets and all three optional spatial scales")
    parser.add_argument("--frame-rate", type=int, choices=range(0, 241), default=0)
    parser.add_argument("--spatial-scale", type=int, choices=(100, 125, 150, 200), default=100)
    arguments = parser.parse_args()
    OPTIONS = (arguments.frame_rate, arguments.spatial_scale)
    arguments.lifecycle = True
    return run_options(arguments)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (base.HarnessError, OSError, ValueError, TypeError, KeyError, subprocess.TimeoutExpired) as error:
        print(str(error) if isinstance(error, base.HarnessError) else "Owned DXMT optional-control probe could not finish", file=sys.stderr)
        raise SystemExit(1)
