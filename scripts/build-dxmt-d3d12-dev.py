#!/usr/bin/env python3
"""Build an isolated experimental DXMT D3D12 PE DLL; never install or run it.

Reuses the reviewed V15 DXMT bridge and its shader compiler. No LLVM build,
network access, Wine session, game, installed app, or runtime modification.
Passing the separate offscreen probe would not establish game compatibility.
"""

import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile

REPO = Path(__file__).resolve().parents[1]
PIN = "e94c312f5c054263acf261cfa109edf13e757587"
HEADERS_PIN = "9df86f2341616ef1888ae59919feaa6d4fad693d"
INPUTS = {
    f"source/dxmt/dxmt-{PIN}.tar.gz": "4bfc553fad02f8b6240e0421e8825ff3f627e41f3bec66de5fc15381648e8882",
    f"source/dxmt/include-native-directx-{HEADERS_PIN}.tar.gz": "91da8c96cea1709f2e9b6778d278bf8917894c4d0c78beefb658e137d6412be1",
    "components/dxmt/runtime/lib/wine/x86_64-windows/dxgi.dll": "3f943a9786418c9d314fbbbaae32d5f752b97666001ef1afef90e7a2b2f08cb3",
    "components/dxmt/runtime/lib/wine/x86_64-windows/winemetal.dll": "fa446a6156f68fa6cdc66b8040510ac7444cafac4bc609099ed5345118b8ca0e",
    "components/dxmt/runtime/lib/wine/x86_64-unix/winemetal.so": "db667347e1ff7b944f5ef50235f96eabf50b44b11e6f26aef9e7e48a24b8317a",
    "components/dxmt/runtime/lib/wine/x86_64-windows/libgdi32.a": "b97c98217dea2ca3c0f1a5018a6e6032a66cbc99c6eefbfb40f47a4676dfd2cc",
    "components/dxmt/runtime/bin/winebuild": "502e911b25c4463d380fdf5dc13fc1e99d0828ba5b116d47e0c892d789179130",
}
PATCHES = {
    "dxmt-e94c312f-wine-11-win-data.patch": "fc013f0920ae111ac201dd1f0b1519585e4502bff9904d504c290213c85684fe",
    "dxmt-e94c312f-wine-11-client-surface.patch": "b7a65dc90d596a8018d261458d7cbc3b162beea572572f77c749605873f33191",
    "dxmt-e94c312f-d3d12-client-surface.patch": "88f4a93ca98a6474054670e5fdd65bbc03e15ce4de25636d702ee6f126c942fd",
}


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command, cwd, timeout=180):
    result = subprocess.run(
        [str(part) for part in command], cwd=cwd, timeout=timeout,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    require(result.returncode == 0, result.stdout[-16000:] or "Build command failed")
    return result.stdout


def unpack(archive, target):
    """Only public regular source files/directories, with a small size bound."""
    total = 0
    with tarfile.open(archive) as stream:
        for entry in stream:
            relative = Path(entry.name)
            require(not relative.is_absolute() and ".." not in relative.parts,
                    "Unsafe source archive path")
            require(entry.isfile() or entry.isdir(), "Unexpected source archive entry")
            total += entry.size
            require(total <= 64 * 1024 * 1024, "Source archive exceeds its bound")
            output = target / relative
            if entry.isdir():
                output.mkdir(parents=True, exist_ok=True)
            else:
                output.parent.mkdir(parents=True, exist_ok=True)
                with stream.extractfile(entry) as source:
                    output.write_bytes(source.read())


def tree_hashes(root):
    require(not any(path.is_symlink() for path in root.rglob("*")),
            "Source tree contains a symlink")
    return {str(path.relative_to(root)): digest(path)
            for path in root.rglob("*") if path.is_file()}


def validate_work_tree(root):
    """Refuse links and special files before tools can replace generated files.

    Checking the complete existing tree also covers objects, stamps, import
    libraries, candidate DLLs, generated shaders, licences and provenance.
    Hard links are refused so truncating a generated file cannot affect a
    second name outside this experiment.
    """
    if not root.exists() and not root.is_symlink():
        return
    for path in [root, *root.rglob("*")]:
        metadata = path.lstat()
        require(stat.S_ISDIR(metadata.st_mode) or stat.S_ISREG(metadata.st_mode),
                "Experiment contains a symlink or special file")
        if stat.S_ISREG(metadata.st_mode):
            require(metadata.st_nlink == 1, "Experiment contains a hard-linked file")


def prepare_source(runtime, work):
    source = work / "source"
    with tempfile.TemporaryDirectory(prefix="source-check-", dir=work) as temporary:
        prepared = Path(temporary) / "source"
        prepared.mkdir()
        unpack(runtime / f"source/dxmt/dxmt-{PIN}.tar.gz", prepared)
        unpack(runtime / f"source/dxmt/include-native-directx-{HEADERS_PIN}.tar.gz",
               prepared / "include/native/directx")
        for name in PATCHES:
            run(["/usr/bin/patch", "-p1", "-F", "0", "--batch", "-i",
                 REPO / "runtime-patches" / name], prepared)
        expected = tree_hashes(prepared)
        if source.exists():
            require(tree_hashes(source) == expected,
                    "Existing experimental source differs from the pinned patched source")
        else:
            prepared.rename(source)
    return source, expected


def build(runtime, work, jobs, tools, app_local):
    validate_work_tree(work)
    work.mkdir(parents=True, exist_ok=True)
    source, source_hashes = prepare_source(runtime, work)
    output = work / "build"
    output.mkdir(exist_ok=True)
    exports = []
    for name in ("winemetal_thunks.c", "airconv_thunks.c"):
        exports += re.findall(r"^(?:WINEMETAL_API|AIRCONV_API)\s+[^;{]+?\s+(\w+)\s*\(",
                              (source / "src/winemetal" / name).read_text(), re.M)
    require(len(exports) == 146 and len(set(exports)) == 146,
            "Pinned Winemetal export declarations changed")
    (output / "winemetal.def").write_text("LIBRARY winemetal.dll\nEXPORTS\n" +
                                           "".join(f"    {name}\n" for name in sorted(exports)))
    run([tools["dlltool"], "--input-def", output / "winemetal.def", "--output-lib",
         output / "libwinemetal.a"], output)
    run([tools["dlltool"], "--input-def", source / "src/dxgi/dxgi.def", "--output-lib",
         output / "libdxgi-dxmt.a"], output)
    (output / "version.h").write_text(
        '#pragma once\n#define DXMT_VERSION "e94c312f-d3d12-caskrun-experiment"\n')
    run(["/usr/bin/xcrun", "-sdk", "macosx", "metal", "-o", output / "dxmt_command.air",
         "-c", source / "src/dxmt/dxmt_command.metal"], output)
    run(["/usr/bin/xcrun", "-sdk", "macosx", "metallib", "-o", output / "dxmt_command.metallib",
         output / "dxmt_command.air"], output)
    run([tools["xxd"], "-n", "dxmt_command", "-i", output / "dxmt_command.metallib",
         output / "dxmt_command.h"], output)
    sources = []
    for folder, variable in (("src/util", "util_src"), ("src/dxmt", "dxmt_src"),
                             ("libs/DXBCParser", "dxbc_parser_src"), ("src/d3d12", "d3d12_src")):
        text = re.sub(r"#.*", "", (source / folder / "meson.build").read_text())
        match = re.search(variable + r"\s*=\s*(?:files\()?\s*\[(.*?)\]", text, re.S)
        require(match is not None, "Pinned source list changed")
        sources += [(source / folder / name).resolve()
                    for name in re.findall(r"'([^']+\.(?:cpp|c))'", match.group(1))]
    sources += [source / "src/util" / name for name in
                ("wsi_monitor_win32.cpp", "wsi_window_win32.cpp", "wsi_platform_win32.cpp")]
    require(len(sources) == 60 and len(set(sources)) == 60, "Pinned source count changed")
    flags = ["-O2", "-DNDEBUG", "-DNOMINMAX", "-D_WIN32_WINNT=0xa00", "-DDXMT_PAGE_SIZE=4096",
             "-Wno-missing-field-initializers", "-Wno-unused-parameter", "-Wno-cast-function-type",
             f"-ffile-prefix-map={source}=dxmt", f"-fmacro-prefix-map={source}=dxmt"]
    includes = ["include", "libs", "src/util", "src/airconv", "src/winemetal", "src/dxmt", "src/dxgi"]
    flags += [f"-I{source / name}" for name in includes] + [f"-I{output}"]
    compiler_identity = {name: run([tools[name], "--version"], output).splitlines()[0]
                         for name in ("gcc", "g++")}
    configuration = hashlib.sha256(json.dumps({
        "source": source_hashes, "flags": flags, "tools": compiler_identity,
        "generated_shader": digest(output / "dxmt_command.h"),
        "generated_version": digest(output / "version.h"),
        "build_recipe": digest(Path(__file__)),
    }, sort_keys=True).encode()).hexdigest()

    def compile_one(path):
        key = hashlib.sha256(str(path.relative_to(source)).encode()).hexdigest()[:12]
        obj = output / f"obj-{key}.o"
        stamp = output / f"obj-{key}.json"
        if not (obj.is_file() and stamp.is_file() and
                json.loads(stamp.read_text()) == {"configuration": configuration, "sha256": digest(obj)}):
            c_source = path.suffix == ".c"
            run([tools["gcc" if c_source else "g++"], "-std=c11" if c_source else "-std=c++20",
                 *flags, "-c", path, "-o", obj], output)
            stamp.write_text(json.dumps({"configuration": configuration, "sha256": digest(obj)}))
        return obj

    print(f"Building {len(sources)} PE source files with {jobs} workers.", flush=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        objects = list(pool.map(compile_one, sources))
    bridge = runtime / "components/dxmt/runtime"
    candidate = output / ("d3d12-native.dll" if app_local else "d3d12.dll")
    run([tools["g++"], "-shared", "-static", "-Wl,--file-alignment=4096", "-o", candidate,
         *objects, source / "src/d3d12/d3d12.def", f"-L{output}", "-ldxgi-dxmt", "-lwinemetal",
         bridge / "lib/wine/x86_64-windows/libgdi32.a", "-ldxguid", "-ldwmapi", "-lversion",
         "-lwinmm", "-lshlwapi", "-lpsapi"], output)
    if not app_local:
        run([bridge / "bin/winebuild", "--builtin", candidate], output)
    shutil.copyfile(source / "LICENSE", output / "DXMT-LICENSE")
    shutil.copyfile(source / "COPYING.LIB", output / "DXMT-COPYING.LIB")
    shutil.copyfile(source / "include/native/directx/COPYING.MinGW-w64.txt",
                    output / "DIRECTX-HEADERS-COPYING.MinGW-w64.txt")
    record = {
        "schema": 1, "experimental": True, "installed": False, "probe_run": False,
        "app_local": app_local, "wine_builtin_marked": not app_local,
        "game_compatibility_verified": False, "dxmt_revision": PIN,
        "directx_headers_revision": HEADERS_PIN, "inputs": INPUTS, "patches": PATCHES,
        "compiler": compiler_identity, "license": "LGPL-2.1-or-later",
        "source": "https://github.com/3Shain/dxmt", "sha256": digest(candidate),
        "bytes": candidate.stat().st_size,
        "limits": ["FL11_1 maximum", "SM5_1 maximum", "incomplete format reporting and barriers",
                   "offscreen probe and presentation still require independent validation"],
    }
    (output / (candidate.stem + "-provenance.json")).write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({"candidate": str(candidate.relative_to(REPO)),
                      "sha256": record["sha256"], "installed": False}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=REPO / ".build/native-text-wine-runtime-dev-v15")
    parser.add_argument("--work-dir", type=Path, default=REPO / ".build/d3dmetal-compat-oct26/dxmt-d3d12")
    parser.add_argument("--jobs", type=int, choices=(1, 2), default=2)
    parser.add_argument("--app-local", action="store_true", help="Build a separate unmarked DLL for a native-override app-local probe")
    parser.add_argument("--build", action="store_true", help="Build the isolated candidate; default only verifies inputs")
    args = parser.parse_args()
    runtime = args.runtime.resolve()
    work = args.work_dir.absolute()
    expected_work = REPO / ".build/d3dmetal-compat-oct26/dxmt-d3d12"
    require(work == expected_work, "Work directory is outside the assigned experiment")
    for path in (REPO / ".build", expected_work.parent, expected_work):
        require(not path.is_symlink(), "Experiment directory must not be a symlink")
    require(work.resolve() == expected_work, "Experiment directory escapes its lexical location")
    require(runtime != work and runtime not in work.parents and work not in runtime.parents,
            "Candidate work directory overlaps its reviewed runtime")
    require((REPO / ".build").resolve() in runtime.parents, "Runtime must be project-local")
    validate_work_tree(work)
    for name, expected in INPUTS.items():
        require(digest(runtime / name) == expected, f"Reviewed runtime input changed: {name}")
    for name, expected in PATCHES.items():
        require(digest(REPO / "runtime-patches" / name) == expected, f"Reviewed patch changed: {name}")
    tools = {name: shutil.which(f"x86_64-w64-mingw32-{name}") for name in ("gcc", "g++", "dlltool")}
    tools["xxd"] = shutil.which("xxd")
    require(all(tools.values()), "Installed MinGW and xxd are required; nothing is downloaded")
    run(["/usr/bin/xcrun", "-sdk", "macosx", "metal", "--version"], REPO)
    if args.build:
        build(runtime, work, args.jobs, tools, args.app_local)
    else:
        print("Pinned sources, bridge, import library, Wine build tool, patches and compiler prerequisites verified. No build or launch performed.")


if __name__ == "__main__":
    main()
