#!/bin/sh
# Resume the pinned, isolated DXMT experiment. This does not package or run Wine.
# Stage the reviewed sources/toolchain first; this script never downloads them.
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
exec python3 - "$repo" "$@" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
from datetime import datetime, timezone

repo = Path(sys.argv[1])
if sys.argv[2:] not in ([], ["--check"]):
    sys.exit("Usage: scripts/build-dxmt-dev.sh [--check]")
check_only = sys.argv[2:] == ["--check"]
root = repo / ".build/dxmt"
source = root / "src"
llvm = root / "toolchains/llvm"
wine = root / "wine"
native = root / "native-x86_64.txt"
pins = {
    source: "e94c312f5c054263acf261cfa109edf13e757587",
    source / "external/nvapi": "d08488fcc82eef313b0464db37d2955709691e94",
    source / "include/native/directx": "9df86f2341616ef1888ae59919feaa6d4fad693d",
    root / "toolchains/llvm-project": "8dfdcc7b7bf66834a761bd8de445840ef68e4d1a",
}
patches = {
    "dxmt-e94c312f-wine-11-win-data.patch": "fc013f0920ae111ac201dd1f0b1519585e4502bff9904d504c290213c85684fe",
    "dxmt-e94c312f-wine-11-client-surface.patch": "b7a65dc90d596a8018d261458d7cbc3b162beea572572f77c749605873f33191",
    "wine-11.0-winemac-metal-exports.patch": "ada2df6599a9883689cda250cf53e46eb030205503054b1774d7bb1b67fe8235",
    "wine-11.0-dxmt-client-surface.patch": "9e6cc0516e87b37dc2dca98660726098d740a2449aff5a169a92a0c4d9b63a0a",
}
patched_sources = {
    "src/d3d11/d3d11_swapchain.cpp": "5166895d45e2ed77dab3e2c90d07743a11289417dbd8649d65c30c668d74510e",
    "src/winemetal/Metal.hpp": "12d5754fd3b7531e333b84ddb73b0ac2572ca5c1982a28d853b6fa289bd9824d",
    "src/winemetal/unix/winemetal_unix.c": "18d7f510147c0102b2bd946448eb609d8bbde074b6ff7ef0ada7f9e5a6504c7e",
    "src/winemetal/winemetal.h": "c047dbcb73a3c187386c4822d16cfd711fe36372b598667723082752172cadcf",
    "src/winemetal/winemetal_thunks.c": "74eda8fae35ff944fc2630464cbb8e699dcf3793fffbe7ccef67a9504fd5027f",
}

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

def run(*args, cwd=repo, env=None):
    result = subprocess.run([str(a) for a in args], cwd=cwd, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    require(result.returncode == 0, f"{args[0]} failed: {result.stderr.strip()}")
    return result.stdout.strip()

def owned(path):
    path = Path(os.path.abspath(path))
    require(repo / ".build" in path.parents, "Build input/output escapes the project's .build")
    for parent in [*reversed(path.parents), path]:
        require(not parent.is_symlink(), f"Build path contains a symlink: {parent}")
    require(subprocess.run(["git", "check-ignore", "-q", "--", str(path)], cwd=repo).returncode == 0,
            "Build paths must be ignored by Git")
    return path

def build_command(log, *args):
    with log.open("x") as stream:
        result = subprocess.run([str(a) for a in args], cwd=source, env=build_env,
                                stdout=stream, stderr=subprocess.STDOUT)
    require(result.returncode == 0, f"Build step failed; inspect {log}")

lock = None
try:
    require(sys.platform == "darwin" and run("/usr/bin/uname", "-m") == "arm64",
            "This experiment requires an Apple Silicon Mac")
    require(run("/usr/bin/arch", "-x86_64", "/usr/bin/uname", "-m") == "x86_64",
            "Install Rosetta before resuming the x86_64 runtime build")
    for path in [root, source, llvm, wine / "src/wine-11.0", wine / "build", native,
                 source / "build", source / "build32", root / "logs", root / "build-script.lock", *pins]:
        owned(path)
    for path, revision in pins.items():
        require(path.is_dir(), f"Missing staged source: {path}; restore the pinned build inputs")
        require(run("git", "rev-parse", "HEAD", cwd=path) == revision,
                f"Source revision does not match the reviewed pin: {path}")
        if path != source:
            require(not run("git", "status", "--porcelain", "--untracked-files=no", cwd=path),
                    f"Pinned toolchain/submodule has tracked modifications: {path}")
    require(run("git", "diff", "--name-only", "HEAD", cwd=source).splitlines() ==
            sorted(patched_sources), "Unexpected tracked DXMT changes")
    for name, digest in patches.items():
        patch = repo / "runtime-patches" / name
        require(hashlib.sha256(patch.read_bytes()).hexdigest() == digest, f"Patch hash changed: {name}")
    # The new bridge supersedes the layout adaptation's hunk. Pin both patches
    # and every resulting source file, and reverse-check the last applied patch.
    run("git", "apply", "--reverse", "--check",
        repo / "runtime-patches/dxmt-e94c312f-wine-11-client-surface.patch", cwd=source)
    for relative, digest in patched_sources.items():
        require(hashlib.sha256((source / relative).read_bytes()).hexdigest() == digest,
                f"Patched DXMT source differs from the reviewed bridge: {relative}")
    require((wine / "src/wine-11.0/VERSION").read_text().strip() == "Wine version 11.0",
            "The translator patches require Wine 11.0")
    with (repo / "runtime-patches/wine-11.0-dxmt-client-surface.patch").open() as stream:
        result = subprocess.run(["/usr/bin/patch", "--dry-run", "--batch", "--reverse", "-p1", "-F", "0"],
                                cwd=wine / "src/wine-11.0", stdin=stream,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    require(result.returncode == 0, "Wine's reviewed managed Metal surface patch is not applied")
    exports = {line.split()[-1] for line in run("/usr/bin/nm", "-gU", wine / "build/dlls/winemac.drv/winemac.so").splitlines()}
    require(exports == {"___wine_unix_call_funcs", "___wine_unix_call_wow64_funcs", "_get_win_data",
                        "_release_win_data", "_macdrv_view_create_metal_view", "_macdrv_view_get_metal_layer",
                        "_macdrv_view_release_metal_view", "_caskrun_macdrv_metal_surface_create_v1",
                        "_caskrun_macdrv_metal_surface_release_v1", "_caskrun_macdrv_metal_surface_prepare_v1"},
            "Rebuild the isolated Wine driver; expected exactly ten reviewed exports including the v1 surface API")
    require((wine / "build/tools/winebuild/winebuild").is_file(), "Missing isolated Wine build tools")
    header = (llvm / "include/llvm/Config/llvm-config.h").read_text()
    require(all(re.search(rf"#define LLVM_VERSION_{key} {value}\b", header)
                for key, value in [("MAJOR", 15), ("MINOR", 0), ("PATCH", 7)]),
            "Stage LLVM 15.0.7 headers/static libraries for x86_64; LLVM is not rebuilt automatically")
    libraries = re.findall(r"'-l(LLVM\w+)'", (source / "src/airconv/meson.build").read_text())
    for name in libraries:
        archive = owned(llvm / "lib" / ("lib" + name + ".a"))
        require(archive.is_file() and run("/usr/bin/lipo", "-archs", archive) == "x86_64",
                f"Missing or incorrect x86_64 LLVM static library: {archive}")
    meson = root / "toolchains/venv/bin/meson"
    ninja = root / "toolchains/venv/bin/ninja"
    require(meson.is_file() and ninja.is_file(), "Stage project-local Meson and Ninja in .build/dxmt/toolchains/venv")
    require(run(meson, "--version") == "1.11.2" and run(ninja, "--version").startswith("1.13.2"),
            "The reviewed toolchain uses Meson 1.11.2 and Ninja 1.13.2")
    for arch in ["x86_64", "i686"]:
        for tool in ["gcc", "g++", "ar", "strip", "windres"]:
            require(shutil.which(f"{arch}-w64-mingw32-{tool}"), f"Install the missing MinGW tool: {arch}-w64-mingw32-{tool}")
    for tool in ["clang", "clang++", "xxd"]:
        require(shutil.which(tool), f"Missing build tool: {tool}")
    metal = subprocess.run(["/usr/bin/xcrun", "-sdk", "macosx", "metal", "--version"],
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    require(metal.returncode == 0,
            "Apple's Metal Toolchain is unavailable. Install it with xcodebuild -downloadComponent MetalToolchain, then retry")
    expected_native = """[binaries]
c = 'clang'
cpp = 'clang++'
objc = 'clang'
objcpp = 'clang++'
[built-in options]
""" + "".join(f"{kind}{suffix} = ['-arch', 'x86_64']\n" for suffix in ["_args", "_link_args"]
               for kind in ["c", "cpp", "objc", "objcpp"]) + """[build_machine]
system = 'darwin'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
"""
    if native.exists():
        normalize = lambda text: [line.strip() for line in text.splitlines() if line.strip() and not line.lstrip().startswith('#')]
        require(normalize(native.read_text()) == normalize(expected_native), "Native configuration differs from the reviewed x86_64 flags")
    for directory in [source / "build", source / "build32"]:
        options_file = directory / "meson-info/intro-buildoptions.json"
        if options_file.is_file():
            options = {item["name"]: item["value"] for item in json.loads(options_file.read_text())}
            if check_only:
                require(all(options.get(key) == [] for key in ["c_args", "cpp_args", "c_link_args", "cpp_link_args"]),
                        "Host compiler flags are contaminated; run the script without --check to reconfigure them")
            require(all(options.get(key) == ["-arch", "x86_64"] for key in ["build.c_args", "build.cpp_args"]),
                    "Existing build-machine compiler flags must target x86_64")
            require(Path(options.get("native_llvm_path", "")).resolve() == llvm and
                    Path(options.get("wine_build_path", "")).resolve() == wine / "build",
                    "Existing Meson build references a different LLVM or Wine tree")
    if check_only:
        print("Pinned DXMT, LLVM, Wine patches, compiler prerequisites and native configuration checked. No build performed.")
        sys.exit(0)
    jobs = os.environ.get("CASKRUN_DXMT_JOBS", "8")
    require(jobs.isdecimal() and 1 <= int(jobs) <= 128, "CASKRUN_DXMT_JOBS must be 1–128")
    root.joinpath("build-script.lock").mkdir()
    lock = root / "build-script.lock"
    if not native.exists():
        native.write_text(expected_native)
    logs = root / "logs" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + f"-{os.getpid()}")
    logs.mkdir(parents=True)
    build_env = dict(os.environ)
    for name in ["CFLAGS", "CXXFLAGS", "OBJCFLAGS", "OBJCXXFLAGS", "CPPFLAGS", "LDFLAGS",
                 "CFLAGS_FOR_BUILD", "CXXFLAGS_FOR_BUILD", "OBJCFLAGS_FOR_BUILD", "OBJCXXFLAGS_FOR_BUILD",
                 "LDFLAGS_FOR_BUILD", "CC", "CXX", "AR", "LD", "CC_FOR_BUILD", "CXX_FOR_BUILD"]:
        build_env.pop(name, None)
    build_env["PATH"] = str(ninja.parent) + os.pathsep + build_env.get("PATH", "")
    for label, directory, cross in [("x64", source / "build", "build-win64.txt"),
                                     ("x86", source / "build32", "build-win32.txt")]:
        arguments = [str(meson), "setup"]
        if (directory / "meson-private/coredata.dat").is_file():
            arguments.append("--reconfigure")
        arguments += [str(directory), str(source), "--cross-file", str(source / cross),
                      "--native-file", str(native), "--buildtype=release", "-Dc_args=[]", "-Dcpp_args=[]",
                      "-Dc_link_args=[]", "-Dcpp_link_args=[]", f"-Dnative_llvm_path={llvm}",
                      f"-Dwine_build_path={wine / 'build'}", "-Dwine_builtin_dll=true", "-Denable_tests=false",
                      "-Denable_nvapi=false", "-Denable_nvngx=false", "-Denable_d3d12=false",
                      "-Dbuild_airconv_for_windows=false"]
        build_command(logs / (label + "-setup.log"), *arguments)
        build_command(logs / (label + "-compile.log"), meson, "compile", "-C", directory, "-j", jobs)
        for dll, subdir in [("d3d11", "d3d11"), ("dxgi", "dxgi"), ("d3d10core", "d3d10"), ("winemetal", "winemetal")]:
            image = owned(directory / "src" / subdir / (dll + ".dll"))
            with image.open("rb") as stream:
                header = stream.read(64)
                require(len(header) == 64 and header[:2] == b"MZ", "Translator is not a PE image")
                offset = struct.unpack_from("<I", header, 60)[0]
                require(offset <= image.stat().st_size - 24, "PE header is outside the translator")
                stream.seek(offset)
                pe = stream.read(24)
            require(pe[:4] == b"PE\0\0" and struct.unpack_from("<H", pe, 4)[0] ==
                    (0x8664 if label == "x64" else 0x14c), f"Unexpected translator architecture: {image}")
    require(run("/usr/bin/lipo", "-archs", source / "build/src/winemetal/unix/winemetal.so") == "x86_64",
            "Translator Unix library must be exactly x86_64")
    print(f"Both isolated translator builds completed. Logs: {logs}\nNothing packaged, launched, or marked game-compatible.")
except (RuntimeError, OSError, ValueError) as error:
    sys.exit(f"DXMT build stopped: {error}")
finally:
    if lock is not None:
        lock.rmdir()
PY
