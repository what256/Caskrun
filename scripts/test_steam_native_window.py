#!/usr/bin/env python3
"""Test exact native-header production methods with original AppKit fixtures.

No Wine, publisher app, personal prefix, clipboard, or OS-wide input is used.
The interactive mode briefly shows this test's own window and is opt-in.
"""
from pathlib import Path
import argparse
import subprocess
import tempfile
import os
import signal
import plistlib

REPO = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--source", type=Path, default=REPO / ".build/steam-native-window-oct14/source")
parser.add_argument("--interactive", action="store_true")
parser.add_argument("--compile-only", action="store_true")
args = parser.parse_args()
source = args.source.resolve()
if not source.is_relative_to((REPO / ".build").resolve()):
    parser.error("source must be an isolated tree inside .build")


def extract(text: str, signature: str) -> str:
    start = text.index(signature)
    opening = text.index("{", start)
    depth = 0
    for end in range(opening, len(text)):
        if text[end] == "{":
            depth += 1
        elif text[end] == "}":
            depth -= 1
            if not depth:
                return text[start:end + 1]
    raise ValueError("unterminated production function: " + signature)


def main() -> None:
    driver = source / "dlls/winemac.drv"
    app = (driver / "cocoa_app.m").read_text()
    window = (driver / "cocoa_window.m").read_text()
    # Architectural invariants that the native-only fixture cannot simulate.
    assert app.index("if ([self caskrunHandleSteamHeaderEvent:anEvent])") < app.index("if ([self caskrunHandleTextContextEvent:anEvent])")
    assert "[caskrunSteamHeaderRouter cancel];" in extract(app, "- (void)applicationDidResignActive:")
    assert "[caskrunSteamHeaderRouter forgetWindow:window];" in extract(app, "- (void) windowWillOrderOut:")
    header_route = extract(app, "- (BOOL) caskrunHandleSteamHeaderEvent:")
    assert "event.window == owner" in header_route
    assert header_route.count("macdrv_cancel_native_text_context();") == 2
    assert header_route.index("macdrv_cancel_native_text_context();") < header_route.index("[owner sendEvent:event]")
    assert "caskrunConsumedRightButton = NO" in header_route
    assert "[self caskrunUpdateSteamHeader];" in extract(window, "- (void) setMacDrvParentWindow:")
    assert "[self caskrunUpdateSteamHeader];" in extract(window, "- (void) setFrameAndWineFrame:")
    assert "[self caskrunUpdateSteamHeader];" in extract(window, "- (void) adjustFeaturesForState")
    template = (REPO / "scripts/fixtures/steam-native-window.m.in").read_text()
    functions = "\n".join(extract(app, name) for name in (
        "static BOOL CaskrunSteamDockEnabled", "static NSString* CaskrunArgumentBasename", "static BOOL CaskrunIsWindowsProcess",
        "static BOOL CaskrunNativeDockEnabled", "static NSString* CaskrunNativeImageName", "static BOOL CaskrunNativeBackgroundProcess"))
    controller_methods = "\n".join(extract(app, name) for name in (
        "- (BOOL) caskrunSteamNativeWindowEnabled", "- (BOOL) caskrunHandleSteamHeaderEvent:",
        "- (void) unminimizeWindowIfNoneVisible", "- (BOOL) applicationShouldHandleReopen:",
        "- (NSApplicationTerminateReply) applicationShouldTerminate:"))
    window_methods = "\n".join(extract(window, name) for name in (
        "- (BOOL) caskrunSteamHeaderEligible", "- (void) caskrunUpdateSteamHeader", "- (void) setTitle:",
        "- (BOOL) caskrunSteamHeaderAcceptsEvents", "- (BOOL) caskrunSteamHeaderContainsEvent:",
        "- (void) caskrunCloseSteamLogin:", "- (void) caskrunMinimizeSteamLogin:",
        "- (BOOL)windowShouldClose:", "- (void) miniaturize:"))
    code = template.replace("/* PROCESS_FUNCTIONS */", functions).replace("/* CONTROLLER_METHODS */", controller_methods).replace("/* WINDOW_METHODS */", window_methods).replace("/* CONTENT_VIEW_METHODS */", extract(window, "- (void) didAddSubview:"))
    with tempfile.TemporaryDirectory(prefix=".steam-header-test-", dir=REPO / ".build") as temp:
        work = Path(temp)
        program = work / "fixture.m"
        program.write_text(code)
        app_bundle = work / "Steam Header Test.app"
        macos = app_bundle / "Contents/MacOS"
        macos.mkdir(parents=True)
        binary = macos / "SteamHeaderTest"
        with (app_bundle / "Contents/Info.plist").open("wb") as stream:
            plistlib.dump({"CFBundleName": "Steam Header Test", "CFBundleIdentifier": "dev.caskrun.steam-header-test",
                          "CFBundleExecutable": "SteamHeaderTest", "CFBundlePackageType": "APPL"}, stream)
        subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter", "-Wno-deprecated-declarations",
                        "-framework", "AppKit", "-framework", "Carbon", "-I", str(driver), str(program), "-o", str(binary)], check=True)
        composition = (REPO / "scripts/fixtures/steam-native-window-composition.m.in").read_text()
        image_methods = "\n".join(extract(window, name) for name in (
            "- (BOOL) wantsUpdateLayer", "- (void) updateLayer", "- (void) setSurfaceRect:", "- (void) setColorImage:"))
        composition_file = work / "composition.m"
        composition_file.write_text(composition.replace("/* VIEW_METHODS */", image_methods))
        composition_binary = work / "composition"
        subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter", "-framework", "AppKit", "-framework", "QuartzCore",
                        "-I", str(driver), str(composition_file), "-o", str(composition_binary)], check=True)
        if args.compile_only:
            print("Exact production AppKit and WineContentView fixtures compiled")
        else:
            subprocess.run([str(composition_binary)], check=True, timeout=15)
            report = work / "result.txt"
            pid_file = work / "owned-pid.txt"
            command = (["/usr/bin/open", "-n", "-W", str(app_bundle), "--args", "--report-file", str(report), "--pid-file", str(pid_file)]
                       if args.interactive else [str(binary), "--headless"])
            process = subprocess.Popen(command, start_new_session=True)
            try:
                result = process.wait(timeout=20)
                if report.exists():
                    output = report.read_text()
                    print(output, end="")
                    if "interactive AppKit native Steam header checks passed" not in output:
                        raise RuntimeError("owned AppKit fixture did not finish its checks")
                if result:
                    raise subprocess.CalledProcessError(result, command)
            finally:
                if report.exists() and process.poll() is None:
                    print(report.read_text(), end="")
                # Only a freshly-created fixture PID, verified against its unique bundle.
                if pid_file.exists():
                    pid = int(pid_file.read_text().strip())
                    comm = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "comm="], capture_output=True, text=True).stdout.strip()
                    if comm == str(binary):
                        if os.getpgid(pid) == pid:
                            os.killpg(pid, signal.SIGTERM)
                        else:
                            os.kill(pid, signal.SIGTERM)
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)



if __name__ == "__main__":
    main()
