#!/usr/bin/env python3
"""Extract pinned upstream open-source Vulkan components without running its installer.

Downloads stay inside the required output directory. No SDK application,
installer helper, system path, shell profile or system driver is executed or
changed. The SDK driver is ARM64 only; x86_64 Wine needs a separate driver
cross build. The SDK's universal loader is not evidence of a universal ICD.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import urllib.request
import zipfile

VERSION = "1.4.363.0"
SIZE = 396088200
SHA256 = "163431e0203c8215286a17cf393643ce6b951f77d1b4f74ab30821a1f0beb09b"
URL = "https://sdk.lunarg.com/sdk/download/1.4.363.0/mac/vulkan_sdk.zip?Human=true"
DATA_MEMBER = "vulkansdk-macOS-1.4.363.0.app/Contents/Resources/installer.dat"


def allowed(name, compiler_support):
    if name.startswith("macOS/include/"):
        return True
    if name in {
        "macOS/lib/libvulkan.1.4.363.dylib", "macOS/lib/libvulkan.1.dylib",
        "macOS/lib/libvulkan.dylib", "macOS/lib/libvulkan_kosmickrisp.dylib",
        "macOS/share/vulkan/icd.d/libkosmickrisp_icd.json", "macOS/lib/pkgconfig/vulkan.pc",
        "macOS/bin/glslc", "macOS/bin/spirv-as", "macOS/bin/vulkaninfo",
    }:
        return True
    return compiler_support and name.startswith((
        "macOS/lib/libSPIRV-Tools", "macOS/lib/pkgconfig/SPIRV-", "macOS/lib/cmake/SPIRV-",
    ))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--compiler-support", action="store_true")
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    archive = out / ("vulkansdk-macos-" + VERSION + ".zip")
    if not archive.exists():
        request = urllib.request.Request(URL, headers={"User-Agent": "Caskrun-upstream-validation/1.0"})
        with urllib.request.urlopen(request, timeout=60) as response, archive.open("wb") as target:
            shutil.copyfileobj(response, target, 1024 * 1024)
    digest = hashlib.sha256()
    with archive.open("rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    if archive.stat().st_size != SIZE or digest.hexdigest() != SHA256:
        raise SystemExit("SDK size/checksum differs from pinned official metadata")
    data = out / "installer.dat"
    with zipfile.ZipFile(archive) as container, container.open(DATA_MEMBER) as source, data.open("wb") as target:
        shutil.copyfileobj(source, target, 1024 * 1024)
    contents = data.read_bytes()
    start = 0
    core = None
    while True:
        start = contents.find(b"7z\xbc\xaf\x27\x1c", start)
        if start < 0:
            break
        offset, size = struct.unpack_from("<QQ", contents, start + 12)
        end = start + 32 + offset + size
        if end > len(contents):
            raise SystemExit("Embedded archive extends past its container")
        candidate = out / "component-payload.7z"
        candidate.write_bytes(contents[start:end])
        listing = subprocess.run(["/usr/bin/bsdtar", "-tf", str(candidate)],
                                 capture_output=True, text=True, check=True).stdout.splitlines()
        if "macOS/lib/libvulkan_kosmickrisp.dylib" in listing:
            core = (candidate, listing)
            break
        start = end
    if core is None:
        raise SystemExit("Pinned open-source driver payload was not found")
    selected = [name for name in core[1] if allowed(name, args.compiler_support)]
    if not selected or any(name.startswith("/") or ".." in Path(name).parts for name in selected):
        raise SystemExit("Refusing unsafe or empty archive selection")
    destination = out / "sdk-open-components"
    destination.mkdir(exist_ok=True)
    subprocess.run(["/usr/bin/bsdtar", "-xf", str(core[0]), "-C", str(destination), "--"] + selected,
                   check=True, timeout=120)
    marker = b"Copyright 2016-2026 LunarG Inc."
    notice_at = contents.find(marker)
    if notice_at >= 4:
        notice_size = struct.unpack_from(">I", contents, notice_at - 4)[0]
        (destination / "SDK-LICENSE-NOTICE.txt").write_bytes(contents[notice_at:notice_at + notice_size])
    record = {"version": VERSION, "url": URL, "size": SIZE, "sha256": SHA256,
              "method": "static ZIP/QtIFW archive extraction; installer never executed",
              "selected": selected}
    (out / "sdk-download.json").write_text(json.dumps(record, indent=2) + "\n")
    print(destination / "macOS")


if __name__ == "__main__":
    main()
