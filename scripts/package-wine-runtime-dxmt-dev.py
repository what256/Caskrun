#!/usr/bin/env python3
"""Create an isolated, experimental DXMT package; never launch or migrate Wine.

All inputs and outputs must be real paths beneath this worktree's ignored
.build directory. --check validates inputs without creating a package. The
receipt describes built components, not game compatibility or release approval.
"""
from __future__ import annotations

import argparse
import ctypes
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile


REPO = Path(__file__).resolve().parent.parent
BASE_IDENTITY = "9dcab17f21490998ad03f6ee4f553da0992a1d9234dfe40efd82956b63fd47ce"
DXMT_REVISION = "e94c312f5c054263acf261cfa109edf13e757587"
LLVM_REVISION = "8dfdcc7b7bf66834a761bd8de445840ef68e4d1a"
PATCHED_DXMT_SOURCES = {
    "src/d3d11/d3d11_swapchain.cpp": "5166895d45e2ed77dab3e2c90d07743a11289417dbd8649d65c30c668d74510e",
    "src/winemetal/Metal.hpp": "12d5754fd3b7531e333b84ddb73b0ac2572ca5c1982a28d853b6fa289bd9824d",
    "src/winemetal/unix/winemetal_unix.c": "18d7f510147c0102b2bd946448eb609d8bbde074b6ff7ef0ada7f9e5a6504c7e",
    "src/winemetal/winemetal.h": "c047dbcb73a3c187386c4822d16cfd711fe36372b598667723082752172cadcf",
    "src/winemetal/winemetal_thunks.c": "74eda8fae35ff944fc2630464cbb8e699dcf3793fffbe7ccef67a9504fd5027f",
}
SUBMODULES = {
    "include/native/directx": "9df86f2341616ef1888ae59919feaa6d4fad693d",
    "external/nvapi": "d08488fcc82eef313b0464db37d2955709691e94",
}
PATCHES = {
    "wine-11.0-winemac-metal-exports.patch": "ada2df6599a9883689cda250cf53e46eb030205503054b1774d7bb1b67fe8235",
    "dxmt-e94c312f-wine-11-win-data.patch": "fc013f0920ae111ac201dd1f0b1519585e4502bff9904d504c290213c85684fe",
    "dxmt-e94c312f-wine-11-client-surface.patch": "b7a65dc90d596a8018d261458d7cbc3b162beea572572f77c749605873f33191",
    "wine-11.0-dxmt-client-surface.patch": "9e6cc0516e87b37dc2dca98660726098d740a2449aff5a169a92a0c4d9b63a0a",
}
EXPORTS = {"_get_win_data", "_release_win_data", "_macdrv_view_create_metal_view",
           "_macdrv_view_get_metal_layer", "_macdrv_view_release_metal_view",
           "_caskrun_macdrv_metal_surface_create_v1", "_caskrun_macdrv_metal_surface_release_v1",
           "_caskrun_macdrv_metal_surface_prepare_v1"}
DLLS = {"d3d11": "d3d11", "dxgi": "dxgi", "d3d10core": "d3d10", "winemetal": "winemetal"}
MACH_MAGICS = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"}


class PackageError(Exception):
    pass


def run(*args: object, cwd: Path | None = None) -> str:
    result = subprocess.run([str(arg) for arg in args], cwd=cwd,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if result.returncode:
        raise PackageError(f"{Path(str(args[0])).name} failed: "
                           + result.stderr.decode("utf-8", errors="replace").strip())
    return result.stdout.decode("utf-8", errors="strict").strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def no_symlink_parents(path: Path) -> Path:
    path = Path(os.path.abspath(path))
    for part in [*reversed(path.parents), path]:
        if part.is_symlink():
            raise PackageError("Input/output paths must not contain symlinks")
    return path


def owned_path(path: Path, repo: Path = REPO) -> Path:
    path = no_symlink_parents(path)
    build = repo / ".build"
    if path == build or build not in path.parents:
        raise PackageError("Inputs and output must be beneath this project's .build directory")
    if subprocess.run(["git", "check-ignore", "-q", "--", str(path)], cwd=repo).returncode:
        raise PackageError("Input/output path is not ignored by Git")
    return path


def inspect_tree(root: Path) -> list[Path]:
    """Never follow source links while enumerating; require internal targets."""
    if root.is_symlink() or not root.is_dir():
        raise PackageError("Package root must be a real directory")
    entries = []
    for current, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(current) / name
            if path.is_symlink():
                target = Path(os.readlink(path))
                if target.is_absolute() or root not in path.resolve(strict=True).parents:
                    raise PackageError("Package contains an external or absolute symlink")
            elif not path.is_file() and not path.is_dir():
                raise PackageError("Package contains a special file")
            entries.append(path)
    return entries


def tree_identity(root: Path) -> str:
    """Matches core runtime_identity.rs's package content digest."""
    inspect_tree(root)
    digest = hashlib.sha256(b"caskrun-bundled-wine-tree-v1\0")

    def data(value: bytes) -> None:
        digest.update(len(value).to_bytes(8, "big"))
        digest.update(value)

    def walk(directory: Path) -> None:
        for path in sorted(directory.iterdir(), key=lambda item: os.fsencode(item.name)):
            relative = os.fsencode(path.relative_to(root))
            if path.is_symlink():
                digest.update(b"L")
                data(relative)
                data(os.fsencode(os.readlink(path)))
            elif path.is_dir():
                digest.update(b"D")
                data(relative)
                walk(path)
            else:
                digest.update(b"F")
                data(relative)
                digest.update((path.stat().st_mode & 0o777).to_bytes(8, "big"))
                digest.update(path.stat().st_size.to_bytes(8, "big"))
                with path.open("rb") as stream:
                    for block in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(block)
    walk(root)
    return digest.hexdigest()


def pe_machine(path: Path) -> int:
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise PackageError("Translator is not a Windows PE image")
        offset = struct.unpack_from("<I", header, 60)[0]
        if offset > path.stat().st_size - 24:
            raise PackageError("Translator PE header is outside the file")
        stream.seek(offset)
        header = stream.read(24)
        if header[:4] != b"PE\0\0":
            raise PackageError("Invalid translator PE signature")
        return struct.unpack_from("<H", header, 4)[0]


def macho(path: Path) -> bool:
    with path.open("rb") as stream:
        return stream.read(4) in MACH_MAGICS


def require_revision(source: Path, revision: str) -> None:
    if run("git", "rev-parse", "HEAD", cwd=source) != revision:
        raise PackageError("Source revision does not match the pinned input")


def require_patched_dxmt_sources(source: Path) -> None:
    # Reverse application alone permits unrelated edits in the same file.
    # This digest pins the complete result of the pinned revision plus patch.
    for name, expected in PATCHED_DXMT_SOURCES.items():
        if sha256(source / name) != expected:
            raise PackageError("DXMT source differs from the exact reviewed patched input")


def build_options(build: Path) -> dict:
    options = json.loads((build / "meson-info/intro-buildoptions.json").read_text())
    values = {entry["name"]: entry["value"] for entry in options}
    required = {"wine_builtin_dll": True, "enable_nvapi": False,
                "enable_nvngx": False, "enable_d3d12": False,
                "build_airconv_for_windows": False}
    if any(values.get(name) != value for name, value in required.items()):
        raise PackageError("Unexpected DXMT build options; optional backends are not reviewed")
    return required


def validate_inputs(args: argparse.Namespace) -> dict:
    paths = {name: owned_path(getattr(args, name)) for name in
             ("base", "wine_build", "dxmt_source", "dxmt_build64", "dxmt_build32", "output")}
    output = paths["output"]
    if output.exists() or output.is_symlink():
        raise PackageError("Candidate output already exists; refusing to replace it")
    if any(output in path.parents or path in output.parents or output == path
           for name, path in paths.items() if name != "output"):
        raise PackageError("Output and inputs must be disjoint trees")
    if tree_identity(paths["base"]) != BASE_IDENTITY:
        raise PackageError("Base package is not the reviewed native-v2 runtime")
    source = paths["dxmt_source"]
    require_revision(source, DXMT_REVISION)
    diff = run("git", "diff", "--name-only", "HEAD", cwd=source)
    if diff.splitlines() != sorted(PATCHED_DXMT_SOURCES):
        raise PackageError("DXMT must contain only the reviewed Wine 11 bridge changes")
    expected_patch = REPO / "runtime-patches/dxmt-e94c312f-wine-11-client-surface.patch"
    run("git", "apply", "--reverse", "--check", str(expected_patch), cwd=source)
    require_patched_dxmt_sources(source)
    for name, revision in SUBMODULES.items():
        require_revision(source / name, revision)
        if run("git", "status", "--porcelain", "--untracked-files=no", cwd=source / name):
            raise PackageError("Pinned submodule has tracked modifications")
    llvm = owned_path(source.parent / "toolchains/llvm-project")
    require_revision(llvm, LLVM_REVISION)
    if run("git", "status", "--porcelain", "--untracked-files=no", cwd=llvm):
        raise PackageError("Pinned LLVM source has tracked modifications")
    for name, expected in PATCHES.items():
        if sha256(REPO / "runtime-patches" / name) != expected:
            raise PackageError("Reviewed runtime patch hash differs")
    winemac = paths["wine_build"] / "dlls/winemac.drv/winemac.so"
    exports = {line.split()[-1] for line in run("/usr/bin/nm", "-gU", winemac).splitlines()}
    if not EXPORTS <= exports:
        raise PackageError("Wine driver lacks the translator and versioned bridge exports; rebuild it first")
    inputs = {"runtime/lib/wine/x86_64-unix/winemac.so": winemac,
              "runtime/lib/wine/x86_64-unix/winemetal.so": paths["dxmt_build64"] / "src/winemetal/unix/winemetal.so"}
    options = {}
    for name, build, machine in [("x86_64", paths["dxmt_build64"], 0x8664),
                                 ("i386", paths["dxmt_build32"], 0x14c)]:
        options[name] = build_options(build)
        for dll, directory in DLLS.items():
            path = no_symlink_parents(build / "src" / directory / (dll + ".dll"))
            if pe_machine(path) != machine:
                raise PackageError("Wrong translator PE architecture")
            inputs[f"runtime/lib/wine/{name}-windows/{dll}.dll"] = path
    for path in list(inputs.values())[:2]:
        no_symlink_parents(path)
        if run("/usr/bin/lipo", "-archs", path) != "x86_64":
            raise PackageError("Translator Unix components must be exactly x86_64")
    licenses = {
        "DXMT-LICENSE.txt": source / "LICENSE",
        "DXMT-LGPL-2.1.txt": source / "COPYING.LIB",
        "DXVK-zlib.txt": source / "src/util/dxvk.LICENSE",
        "MinGW-DirectX-Headers.txt": source / "include/native/directx/COPYING.MinGW-w64.txt",
        "NVIDIA-NVAPI.txt": source / "external/nvapi/License.txt",
        "LLVM-LICENSE.txt": llvm / "llvm/LICENSE.TXT",
    }
    for path in licenses.values():
        if not path.is_file() or path.is_symlink():
            raise PackageError("Missing upstream license notice")
    if "GNU LESSER GENERAL PUBLIC LICENSE" not in licenses["DXMT-LGPL-2.1.txt"].read_text():
        raise PackageError("DXMT full LGPL text is missing")
    return {"paths": paths, "inputs": inputs, "licenses": licenses,
            "llvm": llvm, "options": options,
            "hashes": {name: sha256(path) for name, path in inputs.items()}}


def dependencies(path: Path) -> list[str]:
    return [line.strip().split(" (", 1)[0]
            for line in run("/usr/bin/otool", "-L", path).splitlines()[1:]]


def package_dependency(root: Path, image: Path, dependency: str) -> Path | None:
    if dependency.startswith(("/usr/lib/", "/System/Library/")):
        return None
    if dependency.startswith("@loader_path/"):
        target = image.parent / dependency[len("@loader_path/"):]
    elif dependency.startswith("@rpath/"):
        suffix = dependency[len("@rpath/"):]
        choices = [image.parent / suffix, root / "runtime/lib/wine/x86_64-unix" / suffix,
                   root / "lib" / suffix]
        target = next((choice for choice in choices if choice.is_file()), choices[0])
    else:
        raise PackageError("Mach-O dependency is not a system or package-relative path")
    target = target.resolve(strict=True)
    if root not in target.parents or not target.is_file():
        raise PackageError("Mach-O dependency escapes the candidate")
    return target


def repair_image(root: Path, image: Path) -> None:
    # The ID is listed by otool -L as well, so normalize it before resolving.
    run("/usr/bin/install_name_tool", "-id", "@rpath/" + image.name, image)
    for dependency in dependencies(image):
        if dependency == "@rpath/" + image.name:
            continue
        target = package_dependency(root, image, dependency)
        if target is not None:
            replacement = "@loader_path/" + os.path.relpath(target, image.parent)
            if dependency != replacement:
                run("/usr/bin/install_name_tool", "-change", dependency, replacement, image)
    # Build rpaths can name the developer's checkout. These new images now
    # use explicit loader-relative dependencies and need no search rpaths.
    lines = run("/usr/bin/otool", "-l", image).splitlines()
    for index, line in enumerate(lines):
        if line.strip() == "cmd LC_RPATH":
            value = lines[index + 2].strip().removeprefix("path ").split(" (offset", 1)[0]
            run("/usr/bin/install_name_tool", "-delete_rpath", value, image)
    run("/usr/bin/codesign", "--force", "--sign", "-", image)


def audit(root: Path) -> None:
    for path in inspect_tree(root):
        if path.is_symlink() or not path.is_file() or not macho(path):
            continue
        expected = "arm64" if path == root / "bin/native-icon" else "x86_64"
        if run("/usr/bin/lipo", "-archs", path) != expected:
            raise PackageError("Unexpected packaged Mach-O architecture")
        for dependency in dependencies(path):
            package_dependency(root, path, dependency)
        lines = run("/usr/bin/otool", "-l", path).splitlines()
        for index, line in enumerate(lines):
            if line.strip() == "cmd LC_RPATH":
                value = lines[index + 2].strip().removeprefix("path ").split(" (offset", 1)[0]
                if not value.startswith(("@loader_path", "@executable_path", "/usr/lib/", "/System/Library/")):
                    raise PackageError("Packaged Mach-O contains an external runtime search path")
        run("/usr/bin/codesign", "--verify", "--strict", path)


def archive(source: Path, revision: str, output: Path) -> None:
    # git archive emits only committed source; no .git/config, logs, absolute
    # checkout paths, compiler objects or unrelated worktree files are copied.
    with output.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw,
                                                compresslevel=1, mtime=0) as compressed:
        process = subprocess.Popen(["git", "archive", "--format=tar", revision], cwd=source,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        try:
            shutil.copyfileobj(process.stdout, compressed)
        finally:
            process.stdout.close()
        if process.wait():
            raise PackageError("Unable to archive pinned corresponding source")


def publish_exclusively(source: Path, destination: Path) -> None:
    # Darwin sys/stdio.h: RENAME_EXCL=4. A plain rename may replace an empty
    # destination directory created between validation and publication.
    rename = ctypes.CDLL(None, use_errno=True).renamex_np
    rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(os.fsencode(source), os.fsencode(destination), 4):
        raise OSError(ctypes.get_errno(), "Could not publish candidate without replacing a path")


def create_package(plan: dict) -> None:
    paths = plan["paths"]
    output = paths["output"]
    output.parent.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix=".caskrun-dxmt-package-", dir=output.parent))
    candidate = scratch / "runtime-package"
    try:
        run("/bin/cp", "-cR", paths["base"], candidate)
        for relative, source in plan["inputs"].items():
            if sha256(source) != plan["hashes"][relative]:
                raise PackageError("An input changed after validation")
            target = candidate / relative
            no_symlink_parents(target)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        for relative in list(plan["inputs"])[:2]:
            repair_image(candidate, candidate / relative)
        notices = candidate / "licenses"
        for name, source in plan["licenses"].items():
            shutil.copyfile(source, notices / name)
        sources = candidate / "source/dxmt"
        sources.mkdir()
        for name in PATCHES:
            shutil.copyfile(REPO / "runtime-patches" / name, candidate / "source/patches" / name)
        upstream = [("dxmt", paths["dxmt_source"], DXMT_REVISION),
                    ("llvm-project", plan["llvm"], LLVM_REVISION)]
        upstream += [(name.replace("/", "-"), paths["dxmt_source"] / name, revision)
                     for name, revision in SUBMODULES.items()]
        archives = {}
        for name, source, revision in upstream:
            target = sources / (name + "-" + revision + ".tar.gz")
            archive(source, revision, target)
            archives[str(target.relative_to(candidate))] = sha256(target)
        receipt = {
            "format": "caskrun-runtime-components", "version": 1,
            "status": "experimental", "game_validated": False,
            "base_runtime_identity": "sha256:" + BASE_IDENTITY,
            "components": [{"name": "DXMT", "revision": DXMT_REVISION,
                            "license": "LGPL-2.1-or-later", "status": "built",
                            "windows_architectures": ["x86_64", "i386"],
                            "unix_architecture": "x86_64", "submodules": SUBMODULES,
                            "options": plan["options"]}],
            "llvm": {"revision": LLVM_REVISION, "version": "15.0.7", "linkage": "static"},
            "patches": PATCHES, "archives": archives,
            "patched_dxmt_sources": PATCHED_DXMT_SOURCES,
            "artifacts": {name: {"input_sha256": plan["hashes"][name],
                                 "packaged_sha256": sha256(candidate / name)}
                          for name in plan["inputs"]},
            "validation": {"device_creation": False, "shader_readback": False,
                           "window_presentation": False, "game_play": False},
        }
        (candidate / "components.json").write_text(json.dumps(receipt, indent=2) + "\n")
        with (candidate / "SOURCES.txt").open("a") as stream:
            stream.write("\nExperimental DXMT candidate; existing Caskrun runtimes and bottles are unchanged.\n"
                         "DXMT: https://github.com/3Shain/dxmt, revision " + DXMT_REVISION + "\n"
                         "LLVM: https://github.com/llvm/llvm-project, revision " + LLVM_REVISION + "\n"
                         "Exact upstream source archives: source/dxmt/; local modifications: source/patches/.\n"
                         "Full DXMT LGPL and dependency notices: licenses/. NVAPI is disabled; its source\n"
                         "notice is retained because it is an upstream submodule. LLVM is linked statically.\n"
                         "components.json records input and packaged hashes; no rendering or game gate is claimed.\n"
                         "The Wine driver exports the translator entrypoints and versioned managed\n"
                         "client-surface bridge; corresponding DXMT patches append Unix call 146.\n"
                         "Apply the Wine layout adaptation before the managed client-surface patch.\n"
                         "Use the original build scripts\n"
                         "and the pinned source revisions; do not extract any installed vendor runtime.\n")
        audit(candidate)
        if tree_identity(paths["base"]) != BASE_IDENTITY:
            raise PackageError("Base runtime changed while packaging; candidate not published")
        # No replace: an independently created destination must never be lost.
        if output.exists() or output.is_symlink():
            raise PackageError("Candidate output appeared while packaging")
        no_symlink_parents(output)
        publish_exclusively(candidate, output)
    finally:
        shutil.rmtree(scratch)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    defaults = {"base": ".build/native-apps-wine-runtime-dev-v2",
                "wine-build": ".build/dxmt/wine/build", "dxmt-source": ".build/dxmt/src",
                "dxmt-build64": ".build/dxmt/src/build", "dxmt-build32": ".build/dxmt/src/build32",
                "output": ".build/dxmt-wine-runtime-dev-v3"}
    for name, value in defaults.items():
        parser.add_argument("--" + name, type=Path, default=REPO / value)
    parser.add_argument("--check", action="store_true", help="validate all inputs without creating output")
    args = parser.parse_args()
    try:
        plan = validate_inputs(args)
        if not args.check:
            create_package(plan)
        print("DXMT candidate inputs verified." if args.check else
              "Isolated experimental DXMT package created; no Wine process was launched.")
        return 0
    except (PackageError, OSError, ValueError, KeyError) as error:
        print(f"DXMT packaging refused: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
