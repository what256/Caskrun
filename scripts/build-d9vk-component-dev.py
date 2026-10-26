#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build the pinned legacy D3D9 half of Caskrun's hybrid DXVK candidate.

Consumes already downloaded independent upstream source and reviewed SDK
headers/compiler. Does not download, install system packages, run Windows
programs or modify prefixes. Outputs remain in a new ignored project folder.
Compiler versions are recorded; binary hashes can vary with the toolchain and
must pass the real runtime-view GPU probes before any package accepts them.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tarfile

PROJECT = Path(__file__).resolve().parent.parent
REVISION = "7437b616f05bc678d2ff4b4b75832f9cbddeed75"
SOURCE_SHA = "5de64ebd967b5a13d155a246c4f2715189948e88fb0ee8b2c9dbb4d117fd7cb5"
SDK_FILES = {
    "bin/glslangValidator": "77e39324822e5c51329770c3b542807419e3b6f2e6bdf0127f9520f98aff482a",
    "include/vulkan/vulkan.h": "096f50152d0b298d8df84a561a37d8190c8a60af34dff0e2cc8328b24491a640",
    "include/vulkan/vulkan_core.h": "2f64fd6c7c3f342b0e44f039f1c359b7f0e6cfcb71a7dca1cc5156282faa3b51",
    "include/spirv/unified1/spirv.hpp": "c173fe7353f817bc7c50729a47f9961ab661d1223614850a91b176b59237c729",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def owned(path):
    path = path.absolute()
    if any(parent.is_symlink() for parent in (path, *path.parents)):
        raise ValueError("Build inputs and outputs cannot have symbolic-link ancestors")
    path = path.resolve()
    if PROJECT / ".build" not in path.parents:
        raise ValueError("Inputs and outputs must stay inside the project's ignored build directory")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source-archive", "sdk", "tools", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--jobs", type=int, choices=(1, 2, 3, 4), default=3)
    args = parser.parse_args()
    archive, sdk, tools, output = (owned(path) for path in (args.source_archive, args.sdk, args.tools, args.output))
    if output.exists() or digest(archive) != SOURCE_SHA:
        parser.error("Use the exact source archive and a new output directory")
    if any(output == path or output in path.parents or path in output.parents for path in (archive, sdk, tools)):
        parser.error("Build outputs and source/SDK/tool inputs must be disjoint")
    if any(digest(sdk / name) != expected for name, expected in SDK_FILES.items()):
        parser.error("SDK headers or shader compiler differ from the reviewed build inputs")
    for name in ("meson", "ninja"):
        if not os.access(tools / name, os.X_OK):
            parser.error("A project-local Meson/Ninja tool directory is required")
    output.mkdir(parents=True)
    source = output / "source"
    source.mkdir()
    root_name = "d9vk-" + REVISION
    with tarfile.open(archive, "r:gz") as handle:
        members = handle.getmembers()
        if sum(member.size for member in members) > 64 * 1024 * 1024:
            raise ValueError("Unexpected source archive size")
        for member in members:
            parts = Path(member.name).parts
            if (not parts or parts[0] != root_name or ".." in parts or Path(member.name).is_absolute()
                    or not (member.isdir() or member.isfile())):
                raise ValueError("Source archive contains an unexpected path or file type")
            target = source.joinpath(*parts[1:])
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with handle.extractfile(member) as stream:
                    target.write_bytes(stream.read())
    # Upstream names these submodule include directories at configure time.
    # The separately pinned complete SDK include tree supplies both headers.
    for name in ("include/vulkan/include", "include/spirv/include"):
        (source / name).mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update({"PATH": str(sdk / "bin") + os.pathsep + str(tools) + os.pathsep + env.get("PATH", ""),
                "CFLAGS": shlex.quote("-I" + str(sdk / "include")),
                "CXXFLAGS": shlex.quote("-I" + str(sdk / "include"))})
    results = {}
    for architecture, cross, compiler in (("x86_64-windows", "build-win64.txt", "x86_64-w64-mingw32"),
                                          ("i386-windows", "build-win32.txt", "i686-w64-mingw32")):
        build = output / ("build-" + architecture)
        commands = [[str(tools / "meson"), "setup", str(build), str(source), "--cross-file", str(source / cross),
                     "--buildtype", "release", "-Denable_d3d9=true", "-Denable_dxgi=false",
                     "-Denable_d3d10=false", "-Denable_d3d11=false"],
                    [str(tools / "ninja"), "-C", str(build), "-j" + str(args.jobs)]]
        with (output / (architecture + ".log")).open("wb") as log:
            for command in commands:
                subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=900)
        dll = output / architecture / "d3d9.dll"
        dll.parent.mkdir()
        shutil.copy2(build / "src/d3d9/d3d9.dll", dll)
        subprocess.run([compiler + "-strip", "--strip-all", str(dll)], check=True, timeout=30)
        version = subprocess.check_output([compiler + "-g++", "--version"], text=True).splitlines()[0]
        results[architecture] = {"sha256": digest(dll), "compiler": version}
    shutil.copy2(source / "LICENSE", output / "LICENSE")
    (output / "BUILD.json").write_text(json.dumps({"schema": 1, "revision": REVISION,
        "source_archive_sha256": SOURCE_SHA, "sdk_version": "1.4.363.0", "sdk_input_sha256": SDK_FILES,
        "source_code_patches": [], "raw_files": results,
        "runtime_validation": "not_run; raw outputs must pass the actual dedicated-view GPU gates"}, indent=2) + "\n")
    print(json.dumps({"built": list(results), "validation": "required_before_packaging"}))


if __name__ == "__main__":
    main()
