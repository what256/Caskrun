#!/usr/bin/env python3
"""Package reviewed Wine 11 Vulkan/MSync components in a new local runtime.

Never launches Wine or changes an installed app or bottle. A validation receipt
must bind the exact inputs to both renderer-view synchronization test logs.
Only the new output is signed/relocated. Corresponding source, upstream notices,
and existing native-window fixes are preserved. This is not release approval.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("engine_build", REPO / "scripts/build-wine-engine-components-dev.py")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)
P = BUILD.PACKAGE
BASE_IDENTITY = BUILD.BASE_IDENTITY
LOADER_SHA256 = "2f2bacd1f5d3dcc3b7103801bd9371b5b40fbe0cf68869ed419dc77ab380233c"
LOADER_REVISION = "fa218055ff4948b159f31d43f800c808a13d51df"
SDK_SHA256 = "163431e0203c8215286a17cf393643ce6b951f77d1b4f74ab30821a1f0beb09b"
DXVK_REVISION = "bd8921a3db6c4928f5b34672791121f7c14e38c2"
DXVK_RELEASE_SHA256 = "8b37482116da0136b2b4feaa0118f9441bcf148b9532de474edc5a89c3e55687"
DXVK_SOURCE_SHA256 = "292764b48fb57fcecdce33606acb3d2c8351b00efc117162a589eceebce8200f"
D9VK_REVISION = "7437b616f05bc678d2ff4b4b75832f9cbddeed75"
D9VK_SOURCE_SHA256 = "5de64ebd967b5a13d155a246c4f2715189948e88fb0ee8b2c9dbb4d117fd7cb5"
DXVK_PER_API = {
    "d3d9": {"revision": D9VK_REVISION, "version": "1.10.3-20230507-async-macos"},
    "d3d10_11": {"revision": DXVK_REVISION, "version": "3.1-macos1.0"},
}
DXVK_DLLS = ("d3d9", "d3d10core", "d3d11", "dxgi")
DXVK_RAW_SHA256 = {
    "x86_64-windows/d3d9.dll": "676797daf1cb32320a08b00376948cfebba33e89014f377800ba99293266fb39",
    "x86_64-windows/d3d10core.dll": "b0b51e959532926ef3d1397a26b752bf10459db01a4baaf4ecb194c0ab7f45ea",
    "x86_64-windows/d3d11.dll": "a9b2d981686e3ebe2a4eeda2b11f053a7e46a99af667fdffd4197b4753ae6e8f",
    "x86_64-windows/dxgi.dll": "2aeb1716ede8738532f8f7fa19cc7a3c9353da5cca0b988aadd0d9e135ce0e8a",
    "i386-windows/d3d9.dll": "c50001f0a8830554d3df5170a66a54a499ef6d2b3fd43c1b9eaa1c278543e6c0",
    "i386-windows/d3d10core.dll": "9bcd33daba0de152dfdf72440c17ddce45dc602aa42d0884312ddf2d66949e2d",
    "i386-windows/d3d11.dll": "266b2d19cf8d0446dc506fea6d9e089f6b9a24522cbbb3f2328068f8a238fb4e",
    "i386-windows/dxgi.dll": "ddc8064e0fe41eadd5bf5d176b357f0aaa6360f8bf0a1e32e09cd691e940a755",
}
BUILTIN_MARKER = b"Wine builtin DLL".ljust(32, b"\0")


def regular(path: Path, limit: int = 256 * 1024 * 1024) -> Path:
    P.no_symlink_parents(path)
    if not path.is_file() or path.stat().st_size > limit:
        raise P.PackageError("Input must be a bounded regular file")
    return path


def read_json(path: Path, limit: int = 1024 * 1024) -> dict:
    regular(path, limit)
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise P.PackageError("A component receipt must be an object")
    return value


def shared_server_links(candidate: Path, views: tuple[str, ...]) -> set[str]:
    """Wine tries bin_dir/wineserver before WINESERVER; route every view alike."""
    result = set()
    server = candidate / "runtime/bin/wineserver"
    regular(server)
    for view in views:
        link = candidate / view / "bin/wineserver"
        if link.exists() or link.is_symlink():
            if link.is_dir():
                raise P.PackageError("Secondary server entry is a directory")
            link.unlink()
        link.symlink_to(os.path.relpath(server, link.parent))
        if link.resolve(strict=True) != server.resolve(strict=True):
            raise P.PackageError("Secondary view does not reach the common wineserver")
        result.add(str(link.relative_to(candidate)))
    return result


def mark_wine_builtin(path: Path, winebuild: Path, machine: int) -> dict:
    """Use Wine's documented packaging tool, changing only DOS-stub padding.

    A runtime builtin is a Wine loader classification, not a GPU capability.
    Do not touch an Authenticode-signed image or change PE code/data/flags.
    Wine 11 make_builtin_files() writes this exact 32-byte marker at offset64.
    """
    regular(path)
    regular(winebuild)
    before = path.read_bytes()
    if len(before) < 96 or before[:2] != b"MZ":
        raise P.PackageError("DXVK input is not a PE with reserved DOS-stub space")
    pe = struct.unpack_from("<I", before, 60)[0]
    if pe < 96 or pe > len(before) - 24 or before[pe:pe + 4] != b"PE\0\0":
        raise P.PackageError("DXVK PE header overlaps builtin marker space")
    if struct.unpack_from("<H", before, pe + 4)[0] != machine:
        raise P.PackageError("DXVK input architecture differs")
    optional = pe + 24
    size = struct.unpack_from("<H", before, pe + 20)[0]
    if optional + size > len(before) or size < 2:
        raise P.PackageError("DXVK optional header is truncated")
    magic = struct.unpack_from("<H", before, optional)[0]
    directory = {0x10b: 96, 0x20b: 112}.get(magic)
    if directory is None or size < directory + 5 * 8:
        raise P.PackageError("DXVK security directory is missing")
    number = struct.unpack_from("<I", before, optional + directory - 4)[0]
    if number < 5:
        raise P.PackageError("DXVK security directory cannot be checked")
    if any(struct.unpack_from("<II", before, optional + directory + 4 * 8)):
        raise P.PackageError("Refusing to alter an Authenticode-signed DXVK image")
    P.run(winebuild, "--builtin", path)
    after = path.read_bytes()
    if (len(after) != len(before) or after[:64] != before[:64]
            or after[96:] != before[96:] or after[64:96] != BUILTIN_MARKER):
        raise P.PackageError("Wine builtin conversion changed more than its exact marker")
    return {"original_sha256": hashlib.sha256(before).hexdigest(),
            "runtime_sha256": hashlib.sha256(after).hexdigest(),
            "conversion": "wine11-winebuild-builtin-dos-stub-only-v1"}


def artifact_inputs(wine_build: Path) -> dict[str, Path]:
    inputs = {"runtime/bin/wineserver": wine_build / "build/server/wineserver",
              "bin/_dispatch": REPO / "scripts/wine-runtime-launch.sh"}
    for view in ("runtime", "components/dxmt/runtime"):
        inputs[view + "/lib/wine/x86_64-unix/ntdll.so"] = wine_build / "build/dlls/ntdll/ntdll.so"
        for directory, unix, pe in (("winemac.drv", "winemac.so", "winemac.drv"),
                                    ("win32u", "win32u.so", "win32u.dll"),
                                    ("winevulkan", "winevulkan.so", "winevulkan.dll"),
                                    ("vulkan-1", None, "vulkan-1.dll")):
            if unix:
                if directory == "winemac.drv":
                    source = wine_build / ("artifacts/dxmt/winemac.so" if view.startswith("components/")
                                           else "artifacts/base/x86_64-unix/winemac.so")
                else:
                    source = wine_build / "build/dlls" / directory / unix
                inputs[view + "/lib/wine/x86_64-unix/" + unix] = source
            for arch in ("x86_64", "i386"):
                source = (wine_build / "artifacts/base" / (arch + "-windows") / pe
                          if directory == "winemac.drv" else
                          wine_build / "build/dlls" / directory / (arch + "-windows") / pe)
                inputs[view + "/lib/wine/" + arch + "-windows/" + pe] = source
    return inputs


def input_manifest(inputs: dict[str, Path]) -> dict[str, str]:
    return {name: P.sha256(regular(path)) for name, path in sorted(inputs.items())}


def validate_source(base: Path, work: Path) -> dict[str, str]:
    archive = base / "source/wine-11.0.tar.xz"
    if hashlib.sha512(regular(archive).read_bytes()).hexdigest() != BUILD.REOPEN.TEXT.WINE_SHA512:
        raise P.PackageError("Wine corresponding source archive differs")
    patches = dict(BUILD.REOPEN.TEXT.PATCHES)
    patches[BUILD.REOPEN.REOPEN_PATCH] = BUILD.REOPEN.REOPEN_SHA256
    patches[BUILD.MSYNC_PATCH] = BUILD.MSYNC_SHA256
    patches[BUILD.PORTABILITY_PATCH] = BUILD.PORTABILITY_SHA256
    for name, digest in patches.items():
        if P.sha256(regular(REPO / "runtime-patches" / name)) != digest:
            raise P.PackageError("Reviewed corresponding patch differs: " + name)
    with tempfile.TemporaryDirectory(prefix=".engine-source-check-", dir=REPO / ".build") as temporary:
        source = BUILD.REOPEN.prepare_source(archive, Path(temporary), False)
        for name in (BUILD.MSYNC_PATCH, BUILD.PORTABILITY_PATCH, *BUILD.REOPEN.TEXT.DXMT_PATCHES):
            BUILD.REOPEN.TEXT.apply_patch(source, name)
        if BUILD.REOPEN.TEXT.tree_files(source) != BUILD.REOPEN.TEXT.tree_files(work / "src/wine-11.0"):
            raise P.PackageError("Compiler source differs from the complete reviewed Wine replay")
    config = (regular(work / "build/include/config.h")).read_text()
    required = ('#define SONAME_LIBVULKAN "libvulkan.1.dylib"',
                '#define SONAME_LIBFREETYPE "libfreetype.6.dylib"',
                '#define SONAME_LIBGNUTLS "libgnutls.30.dylib"',
                '#define HAVE_GNUTLS_CIPHER_INIT 1', '#define HAVE_MTLDEVICE_REGISTRYID 1')
    if not all(value + "\n" in config for value in required):
        raise P.PackageError("Wine configuration lost Vulkan, fonts, TLS or Metal support")
    protocol = (work / "src/wine-11.0/include/wine/server_protocol.h").read_text()
    if "#define SERVER_PROTOCOL_VERSION 932\n" not in protocol:
        raise P.PackageError("Wine client/server protocol differs from the reviewed port")
    return patches


def synchronization_proof(receipt_path: Path, hashes: dict[str, str]) -> dict:
    receipt = read_json(receipt_path)
    if (receipt.get("schema") != 1 or receipt.get("kind") != "caskrun-engine-api-validation"
            or receipt.get("base_identity") != "sha256:" + BASE_IDENTITY
            or receipt.get("inputs") != hashes
            or receipt.get("msync_patch_sha256") != BUILD.MSYNC_SHA256
            or receipt.get("portability_patch_sha256") != BUILD.PORTABILITY_SHA256):
        raise P.PackageError("API validation receipt does not bind these exact engine inputs")
    runs = receipt.get("runs")
    if not isinstance(runs, list) or len(runs) != 2:
        raise P.PackageError("MSync validation requires both renderer-view logs")
    result = []
    seen = set()
    for run in runs:
        if not isinstance(run, dict) or set(run) != {"backend", "log", "sha256"}:
            raise P.PackageError("Invalid MSync validation run")
        backend = run["backend"]
        if backend not in ("wined3d", "dxmt") or backend in seen:
            raise P.PackageError("MSync validation must cover each retained view once")
        seen.add(backend)
        relative = run["log"]
        if (not isinstance(relative, str) or Path(relative).is_absolute()
                or ".." in Path(relative).parts):
            raise P.PackageError("Validation logs must remain relative to their local receipt")
        log = receipt_path.parent / relative
        regular(log, 8 * 1024 * 1024)
        if P.sha256(log) != run["sha256"]:
            raise P.PackageError("MSync validation log changed after its recorded result")
        text = log.read_text()
        if "MSync regression PASSED\n" not in text or "MSync regression FAILED" in text:
            raise P.PackageError("MSync test run did not complete successfully")
        pattern = re.compile(r"^" + backend + r" WINEMSYNC=([01]) (x86_64|i686): RESULT checks=(\d+) failures=0$", re.MULTILINE)
        cases = pattern.findall(text)
        if (len(cases) != 4 or {(mode, arch) for mode, arch, _ in cases}
                != {(mode, arch) for mode in ("0", "1") for arch in ("x86_64", "i686")}
                or any(int(count) < 37 for _, _, count in cases)):
            raise P.PackageError("MSync log does not prove the complete architecture/mode matrix")
        result.append({"backend": backend, "log_sha256": run["sha256"],
                       "cases": [{"mode": int(mode), "architecture": arch, "checks": int(count), "failures": 0}
                                 for mode, arch, count in sorted(cases)]})
    return {"kind": receipt["kind"], "input_sha256": hashes, "runs": sorted(result, key=lambda item: item["backend"])}


def component_path(receipt: Path, relative: str) -> Path:
    if (not isinstance(relative, str) or Path(relative).is_absolute()
            or ".." in Path(relative).parts or not relative):
        raise P.PackageError("Component inputs must have safe receipt-relative paths")
    return P.owned_path(receipt.parent / relative)


def notice_files(root: Path, destination: str) -> dict[str, Path]:
    P.owned_path(root)
    result = {}
    for current, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(current) / name
            P.no_symlink_parents(path)
            if not path.is_dir() and not path.is_file():
                raise P.PackageError("Provenance contains a special file")
        for name in files:
            path = regular(Path(current) / name, 4 * 1024 * 1024)
            result[destination + "/" + path.relative_to(root).as_posix()] = path
            if len(result) > 256:
                raise P.PackageError("Provenance exceeds the file-count limit")
    if not result:
        raise P.PackageError("Component provenance/notices are missing")
    return result


def dxvk_render_proof(summary_path: Path, proof_paths: set[Path],
                      marked: dict[str, str], base: Path) -> dict:
    """Parse the real four-cell GPU workload, not receipt assertion booleans."""
    summary = read_json(summary_path)
    required = ("runtime_modules_unchanged", "native_images_unchanged", "base_wine_d3d8_retained",
                "runtime_view_validation_passed", "prefix_graphics_dlls_unchanged",
                "d3d9_validation_passed", "d3d11_validation_passed",
                "app_directories_contain_only_original_probes")
    native = {"moltenvk": "1fb45b3f53eaf61d5c8f2a925e1c764dd4b06bc736be068307889239d201a0bc",
              "vulkan_loader": "c0b109bf57aaa14fe738a724e129de5e7d990a45e4f71ad25ad1609f90612647"}
    if (summary.get("schema") != 1 or summary.get("implementation") != "caskrun-dxvk-hybrid-v1"
            or summary.get("loading_model") != "wine-builtin-runtime-view-v1"
            or any(summary.get(key) is not True for key in required)
            or summary.get("selected_runtime_module_sha256") != marked
            or summary.get("native_sha256") != native):
        raise P.PackageError("DXVK GPU proof does not bind the reviewed runtime images")
    prefix = {directory + "/" + name + ".dll": P.sha256(regular(base / "runtime/lib/wine" / arch / (name + ".dll")))
              for directory, arch in (("system32", "x86_64-windows"), ("syswow64", "i386-windows"))
              for name in ("d3d8", *DXVK_DLLS)}
    d3d8 = {arch: prefix[directory + "/d3d8.dll"]
            for directory, arch in (("system32", "x86_64-windows"), ("syswow64", "i386-windows"))}
    if (summary.get("prefix_graphics_dll_sha256_before") != prefix
            or summary.get("prefix_graphics_dll_sha256_after") != prefix
            or summary.get("base_wine_d3d8_sha256") != d3d8):
        raise P.PackageError("DXVK proof did not preserve the exact ten prefix graphics images")
    rows = summary.get("results")
    if not isinstance(rows, list) or len(rows) != 4:
        raise P.PackageError("DXVK needs four complete API/architecture GPU results")
    spec = importlib.util.spec_from_file_location("dxvk_view_validation", REPO / "scripts/test-dxvk-view-runtime.py")
    probe = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(probe)
    seen, verified, used = set(), [], {summary_path}
    for row in rows:
        if not isinstance(row, dict):
            raise P.PackageError("Invalid DXVK GPU result row")
        api, architecture = row.get("api"), row.get("architecture")
        cell = (api, architecture)
        if api not in ("d3d9", "d3d11") or architecture not in ("x64", "x86") or cell in seen:
            raise P.PackageError("DXVK GPU matrix duplicates or omits a required cell")
        seen.add(cell)
        arch = "x86_64-windows" if architecture == "x64" else "i386-windows"
        modules = ("d3d9.dll",) if api == "d3d9" else ("d3d11.dll", "dxgi.dll")
        if (row.get("selected_module_sha256") != {name: marked[arch + "/" + name] for name in modules}
                or row.get("mapped_runtime_modules") != [arch + "/" + name for name in modules]
                or row.get("observed_native_sha256") != native
                or any(row.get(key) is not True for key in (
                    "actual_host_mapping_verified", "native_moltenvk_observed",
                    "no_wined3d_at_renderer_identity_gate", "prefix_graphics_dlls_unchanged"))):
            raise P.PackageError("DXVK GPU row is missing actual selected PE/host-image proof")
        log = summary_path.parent / (architecture + "-" + api) / "result.jsonl"
        if log not in proof_paths:
            raise P.PackageError("DXVK GPU result has no hash-bound original workload log")
        regular(log, 4 * 1024 * 1024)
        records = [json.loads(line) for line in log.read_text().splitlines() if line.startswith("{")]
        if len(records) > 4096:
            raise P.PackageError("DXVK GPU result log exceeds the record limit")
        try:
            directory = "system32" if architecture == "x64" else "syswow64"
            logical = {name: prefix[directory + "/" + name] for name in modules}
            actual = probe.validate(records, architecture, api,
                                    logical_expected=logical if api == "d3d11" else None)
        except (RuntimeError, KeyError, TypeError, ValueError) as error:
            raise P.PackageError("DXVK original GPU workload failed validation") from error
        if any(row.get(key) != value for key, value in actual.items()):
            raise P.PackageError("DXVK typed GPU summary disagrees with its original workload")
        verified.append(actual)
        used.add(log)
    if used != proof_paths:
        raise P.PackageError("DXVK proof inventory is not the exact summary and four workloads")
    return {"summary_sha256": P.sha256(summary_path), "native_sha256": native,
            "results": sorted(verified, key=lambda row: (row["api"], row["architecture"]))}


def dxvk_plan(path: Path | None, base: Path) -> dict | None:
    if path is None:
        return None
    P.owned_path(path)
    receipt = read_json(path)
    if (receipt.get("schema") != 1 or receipt.get("component") != "caskrun-dxvk-hybrid-v1"
            or receipt.get("per_api") != DXVK_PER_API or receipt.get("moltenvk_version") != "1.4.2"):
        raise P.PackageError("DXVK component does not match the reviewed source pins")
    gates = receipt.get("validation", {})
    if (gates.get("loading_model") != "wine-builtin-runtime-view-v1"
            or any(gates.get(key) is not True for key in
                   ("runtime_view_validation_passed", "d3d9_validation_passed", "d3d11_validation_passed"))):
        raise P.PackageError("DXVK is missing an actual runtime-view/device/render gate")
    expected = {arch + "-windows/" + name + ".dll"
                for arch in ("x86_64", "i386") for name in DXVK_DLLS}
    files = receipt.get("raw_files")
    if not isinstance(files, dict) or set(files) != expected:
        raise P.PackageError("DXVK component must contain the exact eight reviewed PE images")
    inputs = {}
    for name, item in files.items():
        if not isinstance(item, dict) or set(item) != {"path", "sha256"}:
            raise P.PackageError("Invalid DXVK image record")
        source = regular(component_path(path, item["path"]))
        if item["sha256"] != DXVK_RAW_SHA256[name] or P.sha256(source) != DXVK_RAW_SHA256[name]:
            raise P.PackageError("Raw DXVK image changed after validation")
        if P.pe_machine(source) != (0x8664 if name.startswith("x86_64") else 0x14c):
            raise P.PackageError("Raw DXVK image has the wrong architecture")
        inputs[name] = source
    driver = receipt.get("driver", {})
    source = regular(component_path(path, driver.get("path")))
    expected_driver = "583db6b034852823ad40c0c80fe0f5f145bd55b031310d356c29d685efe8d61c"
    if driver.get("sha256") != expected_driver or P.sha256(source) != expected_driver:
        raise P.PackageError("MoltenVK differs from the pinned official SDK artifact")
    if set(P.run("/usr/bin/lipo", "-archs", source).split()) != {"x86_64", "arm64"}:
        raise P.PackageError("Official MoltenVK architectures differ")
    roots = receipt.get("provenance_roots", {})
    if set(roots) != {"dxvk", "d9vk", "moltenvk"}:
        raise P.PackageError("Both DXVK sources and MoltenVK need separate provenance/notices")
    notices = {}
    for name, relative in roots.items():
        notices.update(notice_files(component_path(path, relative), "source/" + name))
    upstream = read_json(component_path(path, roots["dxvk"]) / "UPSTREAM.json")
    if (upstream.get("tag_commit") != DXVK_REVISION
            or upstream.get("source_archive_sha256") != DXVK_SOURCE_SHA256
            or upstream.get("release_archive_sha256") != DXVK_RELEASE_SHA256
            or upstream.get("license") != "Zlib"):
        raise P.PackageError("DXVK source/release provenance differs from the reviewed pins")
    d9 = read_json(component_path(path, roots["d9vk"]) / "UPSTREAM.json")
    if (d9.get("commit") != D9VK_REVISION or d9.get("sha256") != D9VK_SOURCE_SHA256
            or d9.get("license") != "Zlib"):
        raise P.PackageError("D9VK corresponding source provenance differs")
    # The source-built D3D9 PE statically links MinGW/GCC runtime components.
    # Their notices accompany the translator even in a private local package.
    toolchain_notices = {
        "GCC-COPYING3.txt": "8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903",
        "GCC-RUNTIME-EXCEPTION.txt": "9d6b43ce4d8de0c878bf16b54d8e7a10d9bd42b75178153e3af6a815bdc90f74",
        "MINGW-COPYING.txt": "99a69660981156c21336fdb5661f89341b013c94e4bf9e1c7467b4745718397f",
        "WINPTHREADS-COPYING.txt": "63263614cdd29f2f93cba85e992f041b31f9fc7b4033692f31269489a8a1b177",
        "MINGW-AUTHORS.txt": "8f308316e7a94b8410f871868bd3948f126526dc1775aeee97a0e365f9a0fd9f",
    }
    for name, digest in toolchain_notices.items():
        if P.sha256(regular(component_path(path, roots["d9vk"]) / name)) != digest:
            raise P.PackageError("A required D9VK toolchain notice differs or is missing")
    proofs = receipt.get("proof_logs")
    if not isinstance(proofs, list) or not proofs or len(proofs) > 32:
        raise P.PackageError("DXVK component needs bounded actual probe logs")
    proof_hashes = []
    proof_paths = set()
    for proof in proofs:
        if not isinstance(proof, dict) or set(proof) != {"path", "sha256"}:
            raise P.PackageError("Invalid DXVK proof-log record")
        log = regular(component_path(path, proof["path"]), 16 * 1024 * 1024)
        if P.sha256(log) != proof["sha256"]:
            raise P.PackageError("DXVK proof log changed")
        if log in proof_paths:
            raise P.PackageError("DXVK proof log is duplicated")
        proof_paths.add(log)
        proof_hashes.append(proof["sha256"])
    summaries = [entry for entry in proof_paths if entry.name == "summary.json"]
    if len(summaries) != 1:
        raise P.PackageError("DXVK component requires one typed GPU proof summary")
    marked = {}
    for name, image in inputs.items():
        value = bytearray(image.read_bytes())
        value[64:96] = BUILTIN_MARKER
        marked[name] = hashlib.sha256(value).hexdigest()
    gpu = dxvk_render_proof(summaries[0], proof_paths, marked, base)
    return {"inputs": inputs, "hashes": input_manifest(inputs),
            "driver": source, "driver_sha256": expected_driver, "notices": notices,
            "validation": dict(gates), "gpu_proof": gpu, "proof_logs_sha256": sorted(proof_hashes)}


def validate(args: argparse.Namespace) -> dict:
    paths = {name: P.owned_path(getattr(args, name)) for name in
             ("base", "wine_build", "sdk", "loader_provenance", "validation_receipt", "output")}
    base, output = paths["base"], paths["output"]
    if output.exists() or output.is_symlink():
        raise P.PackageError("Output exists; refusing to replace a runtime")
    for name, path in paths.items():
        if name != "output" and (output == path or output in path.parents or path in output.parents):
            raise P.PackageError("Output and build inputs must be disjoint")
    if P.tree_identity(base) != BASE_IDENTITY:
        raise P.PackageError("Base is not the exact reviewed V14 package")
    patches = validate_source(base, paths["wine_build"])
    inputs = artifact_inputs(paths["wine_build"])
    loader = regular(paths["sdk"] / "lib/libvulkan.1.4.363.dylib")
    if P.sha256(loader) != LOADER_SHA256:
        raise P.PackageError("Vulkan loader differs from the pinned official SDK component")
    inputs["lib/libvulkan.1.dylib"] = loader
    for name, source in inputs.items():
        regular(source)
        if name == "bin/_dispatch":
            if not source.read_bytes().startswith(b"#!/bin/sh\n"):
                raise P.PackageError("Reviewed dispatcher is not the expected shell entrypoint")
            continue
        if P.macho(source):
            arches = set(P.run("/usr/bin/lipo", "-archs", source).split())
            if arches != ({"x86_64", "arm64"} if name.startswith("lib/") else {"x86_64"}):
                raise P.PackageError("Wine/loader Mach-O architecture differs")
        elif P.pe_machine(source) != (0x14c if "/i386-windows/" in name else 0x8664):
            raise P.PackageError("Wine Windows image architecture differs")
    symbols = lambda path: {line.split()[-1] for line in P.run("/usr/bin/nm", "-gU", path).splitlines()}
    for view in ("runtime", "components/dxmt/runtime"):
        for name in ("winemac.so", "ntdll.so", "win32u.so"):
            relative = view + "/lib/wine/x86_64-unix/" + name
            if symbols(inputs[relative]) != symbols(base / relative):
                raise P.PackageError("A replacement driver changed its reviewed native exports")
    upstream = read_json(paths["loader_provenance"] / "UPSTREAM.json")
    if (upstream.get("commit") != LOADER_REVISION or upstream.get("sdk_zip_sha256") != SDK_SHA256
            or upstream.get("universal_loader_sha256") != LOADER_SHA256):
        raise P.PackageError("Vulkan loader provenance differs from the reviewed upstream pin")
    if "Apache License" not in regular(paths["loader_provenance"] / "LICENSE.txt").read_text():
        raise P.PackageError("Vulkan loader Apache license is missing")
    notices = notice_files(paths["loader_provenance"], "source/vulkan-loader")
    for name in ("msync", "vulkan-portability"):
        for file in P.inspect_tree(REPO / "runtime-patches" / name):
            if file.is_file():
                notices["source/patches/" + name + "/" + str(file.relative_to(REPO / "runtime-patches" / name))] = file
    for name, digest in patches.items():
        source = regular(REPO / "runtime-patches" / name)
        if P.sha256(source) != digest:
            raise P.PackageError("Source patch changed during validation")
        notices["source/patches/" + name] = source
    notices["source/patches/wine-runtime-launch.sh"] = REPO / "scripts/wine-runtime-launch.sh"
    hashes = input_manifest(inputs)
    proof = synchronization_proof(paths["validation_receipt"], hashes)
    dxvk = dxvk_plan(args.dxvk_component_receipt, base)
    return {"paths": paths, "inputs": inputs, "hashes": hashes, "patches": patches,
            "notices": notices, "proof": proof, "dxvk": dxvk}


def copy_component_files(candidate: Path, inputs: dict[str, Path], hashes: dict[str, str]) -> None:
    for name, source in inputs.items():
        if P.sha256(source) != hashes[name]:
            raise P.PackageError("An engine input changed during packaging")
        target = P.no_symlink_parents(candidate / name)
        target.parent.mkdir(parents=True, exist_ok=True)
        if name == "lib/libvulkan.1.dylib":
            P.run("/usr/bin/lipo", source, "-thin", "x86_64", "-output", target)
        else:
            shutil.copy2(source, target)
        if name == "bin/_dispatch":
            target.chmod(0o755)
    for name in inputs:
        target = candidate / name
        if P.macho(target):
            P.repair_image(candidate, target)


def add_dxvk(candidate: Path, plan: dict, winebuild: Path) -> tuple[dict, set[str]]:
    component = candidate / "components/dxvk"
    component.mkdir()
    view = component / "runtime"
    P.run("/bin/cp", "-cRp", candidate / "runtime", view)
    modified = {}
    for name, source in plan["inputs"].items():
        if P.sha256(source) != plan["hashes"][name]:
            raise P.PackageError("A raw DXVK input changed during packaging")
        target = view / "lib/wine" / name
        shutil.copy2(source, target)
        if P.sha256(target) != plan["hashes"][name]:
            raise P.PackageError("A raw DXVK image changed while copying")
        modified[name] = mark_wine_builtin(target, winebuild, 0x8664 if name.startswith("x86_64") else 0x14c)
    driver = component / "vulkan/lib/libMoltenVK.dylib"
    driver.parent.mkdir(parents=True)
    if P.sha256(plan["driver"]) != plan["driver_sha256"]:
        raise P.PackageError("MoltenVK input changed during packaging")
    P.run("/usr/bin/lipo", plan["driver"], "-thin", "x86_64", "-output", driver)
    if P.sha256(plan["driver"]) != plan["driver_sha256"]:
        raise P.PackageError("MoltenVK input changed while thinning")
    P.repair_image(candidate, driver)
    icd = component / "vulkan/icd.d/MoltenVK_icd.json"
    icd.parent.mkdir()
    icd.write_text(json.dumps({"file_format_version": "1.0.0", "ICD": {
        "library_path": "../lib/libMoltenVK.dylib", "api_version": "1.4.357",
        "is_portability_driver": True}}, indent=2) + "\n")
    linked = (icd.parent / json.loads(icd.read_text())["ICD"]["library_path"]).resolve(strict=True)
    if linked != driver.resolve(strict=True):
        raise P.PackageError("DXVK ICD does not reach its exact packaged MoltenVK driver")
    for name, source in plan["notices"].items():
        target = candidate / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    (candidate / "dxvk-layout.txt").write_text("caskrun-dxvk-view-v1\n")
    return ({"implementation": "caskrun-dxvk-hybrid-v1", "per_api": DXVK_PER_API,
             "moltenvk_version": "1.4.2", "windows_architectures": ["x86_64", "i386"],
             "loading_model": "wine-builtin-runtime-view-v1", "runtime_view_validation_passed": True,
             "api_validation_passed": True, "d3d9_validation_passed": True,
             "d3d11_validation_passed": True, "d3d12_supported": False,
             "d3d8_api_validation_passed": False, "game_validated": False,
             "pe_packaging": modified, "driver_packaged_sha256": P.sha256(driver),
             "gpu_proof": plan["gpu_proof"], "proof_logs_sha256": plan["proof_logs_sha256"]},
            set(plan["notices"]) | {"dxvk-layout.txt"})


def package(plan: dict) -> str:
    paths = plan["paths"]
    output = paths["output"]
    output.parent.mkdir(parents=True, exist_ok=True)
    before = BUILD.REOPEN.TEXT.tree_files(paths["base"])
    with tempfile.TemporaryDirectory(prefix=".engine-package-", dir=output.parent) as temporary:
        candidate = Path(temporary) / "runtime-package"
        P.run("/bin/cp", "-cRp", paths["base"], candidate)
        copy_component_files(candidate, plan["inputs"], plan["hashes"])
        receipt = read_json(candidate / "components.json")
        for name, source in plan["inputs"].items():
            section = "artifacts" if name.startswith("components/") else "base_artifacts"
            receipt.setdefault(section, {})[name] = {"input_sha256": plan["hashes"][name],
                                                    "packaged_sha256": P.sha256(candidate / name)}
        added = set()
        for name, source in plan["notices"].items():
            target = P.no_symlink_parents(candidate / name)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
            if P.sha256(source) != P.sha256(target):
                raise P.PackageError("Corresponding source changed during copying")
            added.add(name)
        receipt["patches"].update(plan["patches"])
        receipt["dispatcher_sha256"] = P.sha256(candidate / "bin/_dispatch")
        receipt["engine_components_predecessor_identity"] = "sha256:" + BASE_IDENTITY
        receipt["msync"] = {"implementation": "caskrun-wine11-msync-v1", "protocol": 932,
                            "default_enabled": False, "api_validation_passed": True,
                            "game_validated": False, "upstream_variant": "generic-msync-devel",
                            "validation": plan["proof"]}
        receipt["wine_vulkan"] = {"implementation": "caskrun-wine11-vulkan-v1",
                                  "windows_architectures": ["x86_64", "i386"],
                                  "loader_revision": LOADER_REVISION,
                                  "loader_sdk_sha256": SDK_SHA256, "game_validated": False}
        secondary = ["components/dxmt/runtime"]
        if plan["dxvk"] is not None:
            dxvk, dxvk_added = add_dxvk(candidate, plan["dxvk"], paths["wine_build"] / "build/tools/winebuild/winebuild")
            receipt["dxvk"] = dxvk
            receipt["backend_roots"]["dxvk"] = "components/dxvk/runtime"
            for name, record in dxvk["pe_packaging"].items():
                relative = "components/dxvk/runtime/lib/wine/" + name
                receipt["artifacts"][relative] = {"input_sha256": record["original_sha256"],
                                                    "packaged_sha256": record["runtime_sha256"],
                                                    "conversion": record["conversion"]}
            for name in plan["inputs"]:
                if name.startswith("runtime/lib/"):
                    relative = "components/dxvk/" + name
                    if relative not in receipt["artifacts"]:
                        receipt["artifacts"][relative] = {"input_sha256": plan["hashes"][name],
                                                            "packaged_sha256": P.sha256(candidate / relative)}
            relative = "components/dxvk/vulkan/lib/libMoltenVK.dylib"
            receipt["artifacts"][relative] = {"input_sha256": P.sha256(plan["dxvk"]["driver"]),
                                                "packaged_sha256": P.sha256(candidate / relative)}
            secondary.append("components/dxvk/runtime")
            added |= dxvk_added
        added |= shared_server_links(candidate, tuple(secondary))
        receipt["shared_wineserver"] = {"path": "runtime/bin/wineserver", "protocol": 932,
                                       "secondary_views_use_internal_relative_links": True}
        (candidate / "components.json").write_text(json.dumps(receipt, indent=2) + "\n")
        with (candidate / "SOURCES.txt").open("a") as stream:
            stream.write("\nWine 11 coordinated Vulkan/MSync engine components.\n"
                         "Original generic MSync Wine11 port and Vulkan portability patch: source/patches/.\n"
                         "MSync is opt-in; all clients and the common wineserver use protocol932.\n"
                         "Vulkan loader: source/vulkan-loader/UPSTREAM.json and supplied licenses.\n"
                         "All existing DXMT translator, TLS/font and native-window source/notices retained.\n"
                         "No storefront/game compatibility or public binary release approval is claimed.\n")
            if plan["dxvk"] is not None:
                stream.write("D3D9 D9VK1.10.3 / D3D10-11 Metalsharp3.1 / official MoltenVK1.4.2: source/d9vk, source/dxvk, source/moltenvk.\n"
                             "DXVK runtime images are altered only by Wine11 winebuild's builtin DOS-stub marker; original and runtime hashes are recorded.\n")
        P.audit(candidate)
        if P.tree_identity(paths["base"]) != BASE_IDENTITY:
            raise P.PackageError("Reviewed base changed while packaging")
        # No unrelated old file can disappear or change. Entire new DXVK view
        # is derived from the already validated base view and the typed inputs.
        after = BUILD.REOPEN.TEXT.tree_files(candidate)
        allowed = set(plan["inputs"]) | added | {"components.json", "SOURCES.txt"}
        for name in before:
            if before[name] != after.get(name) and name not in allowed:
                raise P.PackageError("Packaging changed an unrelated retained engine file")
        identity = P.tree_identity(candidate)
        P.publish_exclusively(candidate, output)
    return identity


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("base", "wine-build", "sdk", "loader-provenance", "validation-receipt", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--dxvk-component-receipt", type=Path)
    parser.add_argument("--check", action="store_true", help="validate exact sources, components and proof without staging")
    args = parser.parse_args()
    try:
        plan = validate(args)
        if args.check:
            print("Exact reviewed engine sources, component images and validation receipts verified")
        else:
            print("Candidate runtime identity: sha256:" + package(plan))
        return 0
    except (P.PackageError, OSError, ValueError, subprocess.SubprocessError) as error:
        print("Engine packaging refused: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
