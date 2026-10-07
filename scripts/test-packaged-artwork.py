#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Verify packaged Home artwork using an original PE; never execute Windows.

Requires macOS, an owned Caskrun build and MinGW-w64 GCC/windres.
All fixture data stays ignored and is removed after a zero-owner check.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import stat
import struct
import subprocess
import tempfile


PROJECT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    arguments = parser.parse_args()
    app = arguments.app.resolve(strict=True)
    if not app.is_relative_to(PROJECT / ".build") or app.name != "Caskrun.app":
        parser.error("Use an owned Caskrun app under this project's .build directory.")
    marker = app / "Contents/Resources/.caskrun-generated-app"
    if marker.is_symlink() or not marker.is_file() or marker.read_bytes():
        parser.error("The app is not a recognized generated Caskrun package.")
    with (app / "Contents/Info.plist").open("rb") as handle:
        if plistlib.load(handle).get("CFBundleIdentifier") != "dev.caskrun.app":
            parser.error("Unrecognized Caskrun bundle identifier.")
    core = app / "Contents/MacOS/caskrun-core"
    wine = app / "Contents/Resources/wine-runtime/bin/wine"
    compiler = shutil.which("x86_64-w64-mingw32-gcc")
    resources = shutil.which("x86_64-w64-mingw32-windres")
    if not compiler or not resources:
        parser.error("MinGW-w64 GCC and windres are required for the original PE fixture.")
    spec = importlib.util.spec_from_file_location(
        "original_icon_fixture", PROJECT / "scripts/test-native-icon-renderer.py")
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    root = Path(tempfile.mkdtemp(prefix=".packaged-artwork-smoke-", dir=PROJECT / ".build")).resolve()
    root.chmod(0o700)
    identity = root.stat().st_dev, root.stat().st_ino
    (root / ".owner.json").write_text(json.dumps({
        "purpose": "original-packaged-artwork-smoke", "uid": os.getuid(), "identity": identity}))
    for name in ["host", "data", "tmp"]:
        (root / name).mkdir(mode=0o700)
    environment = {
        "PATH": os.pathsep.join([str(Path(compiler).parent), "/usr/bin", "/bin", "/usr/sbin", "/sbin"]),
        "HOME": str(root / "host"), "CASKRUN_HOME": str(root / "data"),
        "CASKRUN_WINE": str(wine), "TMPDIR": str(root / "tmp") + "/"}
    try:
        blue = (14, 82, 216, 255)
        png = fixture.png(256, 256, lambda x, y:
                          blue if (x - 128) ** 2 + (y - 128) ** 2 <= 104 ** 2 else (0, 0, 0, 0))
        ico = b"\x00\x00\x01\x00\x01\x00" + struct.pack(
            "<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 22) + png
        (root / "original.ico").write_bytes(ico)
        (root / "probe.c").write_text("int main(void) { return 0; }\n")
        (root / "probe.rc").write_text('1 ICON "original.ico"\n')
        subprocess.run([resources, "probe.rc", "probe.o"], cwd=root, env=environment,
                       capture_output=True, timeout=20, check=True)
        subprocess.run([compiler, "probe.c", "probe.o", "-o", "Original Icon Probe.exe"],
                       cwd=root, env=environment, capture_output=True, timeout=20, check=True)

        def call(*items):
            result = subprocess.run([str(core), *items], cwd=root, env=environment,
                                    capture_output=True, timeout=6, check=True)
            return json.loads(result.stdout)

        bottle = call("create", "Original Artwork Fixture")
        folder = root / "data/bottles" / bottle["id"]
        # create already owns an empty prefix. There is no Wine initialization,
        # service, installer, account or Windows execution in this fixture.
        assert (folder / "prefix").is_dir()
        saved = call("add-app", bottle["id"], "Original Icon Probe", str(root / "Original Icon Probe.exe"))
        image = call("app-icon", bottle["id"], saved["id"])
        path = Path(image["path"])
        assert path.is_relative_to(folder / "artwork/native") and path.suffix == ".png"
        parsed = fixture.decode_png(path.read_bytes())
        assert fixture.at(parsed, 148, 148)[3] < 10, "Finished circle received a second tile"
        assert fixture.at(parsed, 512, 512)[:3] == blue[:3], "Publisher colour changed"
        assert not (folder / "native-apps").exists(), "Listing created a launch alias"
        assert call("app-icon", bottle["id"], saved["id"])["path"] == str(path)
        assert call("bottle-icon", bottle["id"])["path"] == str(path)
        print("Passed packaged core/helper, manual EXE, first-list composition, app/bottle cache reuse and no-launch-alias checks.")
    finally:
        current = root.lstat()
        assert stat.S_ISDIR(current.st_mode) and current.st_uid == os.getuid()
        assert (current.st_dev, current.st_ino) == identity, "Fixture directory changed"
        owners = subprocess.run(["/usr/sbin/lsof", "+D", str(root)], capture_output=True, timeout=10)
        if owners.returncode not in (0, 1) or owners.stdout or owners.stderr.strip():
            raise RuntimeError("Preserved fixture: open owners or uncertain process inspection")
        shutil.rmtree(root)
        print("Owned fixture removed after zero-owner check.")


if __name__ == "__main__":
    main()
