#!/usr/bin/env python3
"""Inspect/stage a user-obtained Apple D3DMetal redist, without launching it.

This is a private development-component importer, not backend enablement. It
does not download, mount an image, accept a license, copy a Wine engine, patch
Apple binaries, or modify a Caskrun runtime/bottle. First mount the official
Apple evaluation image yourself, read its actual license, then use inspect.
Stage requires the exact inspected license SHA-256 supplied by the person who
reviewed it. Output stays in this checkout's ignored .build tree.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import tempfile
from urllib.parse import urlparse


REPO = Path(__file__).resolve().parents[1]
CORE_DLLS = ("d3d11", "d3d12", "dxgi")
OPTIONAL_DLLS = ("d3d10", "d3d10core", "d3d12core", "nvapi64", "nvngx-on-metalfx")
DOCS = ("License.rtf", "Acknowledgements.rtf", "Read Me.rtf")
MAX_COMPONENT_SIZE = 512 * 1024 * 1024
MAX_FILES = 10000


class ImportError(Exception):
    pass


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def run(*args: object) -> str:
    result = subprocess.run([str(a) for a in args], stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, check=False, timeout=60)
    if result.returncode:
        # Avoid publishing source paths, usernames or downloaded-image metadata.
        raise ImportError(f"{Path(str(args[0])).name} validation failed")
    return result.stdout.decode("utf-8", errors="strict")


def real_path(path: Path) -> Path:
    path = Path(os.path.abspath(path))
    for part in (*reversed(path.parents), path):
        if part.is_symlink():
            raise ImportError("Input/output root has a symlink parent")
    return path


def output_path(path: Path) -> Path:
    path = real_path(path)
    build = REPO / ".build"
    if build not in path.parents:
        raise ImportError("Output must be beneath this checkout's .build directory")
    if subprocess.run(["git", "check-ignore", "-q", "--", str(path)],
                      cwd=REPO, check=False).returncode:
        raise ImportError("Output must be ignored by Git")
    if path.exists():
        raise ImportError("Output already exists; refusing to replace it")
    return path


def validate_url(value: str) -> str:
    parsed = urlparse(value)
    if (parsed.scheme != "https" or parsed.hostname != "download.developer.apple.com"
            or parsed.username or parsed.password or parsed.port or parsed.query
            or parsed.fragment or not parsed.path.startswith("/Developer_Tools/")
            or not parsed.path.lower().endswith(".dmg")):
        raise ImportError("Source URL must name an official Apple Developer DMG")
    return value


def entries(root: Path) -> list[Path]:
    """Validate the complete framework, preserving only internal relative links."""
    if root.is_symlink() or not root.is_dir():
        raise ImportError("Component directory must be a real directory")
    result = []
    total = 0
    for current, dirs, files in os.walk(root, followlinks=False):
        for name in sorted(dirs + files):
            path = Path(current) / name
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode):
                target = Path(os.readlink(path))
                try:
                    resolved = path.resolve(strict=True)
                except (OSError, RuntimeError) as error:
                    raise ImportError("Component has a broken or looping link") from error
                if target.is_absolute() or not resolved.is_relative_to(root):
                    raise ImportError("Component has an external/absolute symlink")
            elif stat.S_ISREG(mode):
                total += path.stat().st_size
            elif not stat.S_ISDIR(mode):
                raise ImportError("Component has a special file")
            result.append(path)
            if total > MAX_COMPONENT_SIZE or len(result) > MAX_FILES:
                raise ImportError("Component exceeds the bounded import size")
    return result


def pe_machine(path: Path) -> str:
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) < 64 or header[:2] != b"MZ":
            raise ImportError("Apple Windows component is not a PE image")
        offset = struct.unpack_from("<I", header, 60)[0]
        if offset > path.stat().st_size - 24:
            raise ImportError("Windows component PE header is outside its file")
        stream.seek(offset)
        header = stream.read(24)
        if header[:4] != b"PE\0\0":
            raise ImportError("Windows component has no PE signature")
        machine = struct.unpack_from("<H", header, 4)[0]
        if machine != 0x8664:
            raise ImportError("This importer supports only Apple's x86_64 redist")
        return "x86_64"


def apple_framework_signature(framework: Path) -> dict:
    # Do not ad-hoc re-sign or remove signature/quarantine attributes. Require
    # Apple's original framework signature, not an arbitrary Developer ID.
    run("/usr/bin/codesign", "--verify", "--strict", "--deep", "-R=anchor apple",
        framework)
    result = subprocess.run(["/usr/bin/codesign", "-d", "--verbose=4", str(framework)],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            check=False, timeout=30)
    text = result.stderr.decode("utf-8", errors="strict")
    if result.returncode or "Identifier=com.apple.D3DMetal\n" not in text:
        raise ImportError("Framework is not Apple's signed D3DMetal component")
    cdhash = re.search(r"^CDHash=([0-9a-f]+)$", text, re.MULTILINE)
    if not cdhash:
        raise ImportError("Apple framework signature has no CDHash")
    return {"identifier": "com.apple.D3DMetal", "anchor": "apple",
            "cdhash": cdhash.group(1), "verified": True}


def macho_arches(path: Path) -> list[str]:
    architectures = run("/usr/bin/lipo", "-archs", path).split()
    if "x86_64" not in architectures:
        raise ImportError("Apple host component has no x86_64 architecture")
    return sorted(architectures)


def file_record(path: Path, relative: str) -> dict:
    if path.is_symlink():
        return {"path": relative, "type": "symlink", "target": os.readlink(path)}
    if path.is_dir():
        return {"path": relative, "type": "directory"}
    return {"path": relative, "type": "file", "size": path.stat().st_size,
            "sha256": sha256(path)}


def inspect(args: argparse.Namespace) -> tuple[dict, dict[str, Path]]:
    root = real_path(args.evaluation_root)
    if not root.is_dir():
        raise ImportError("Evaluation image directory is missing")
    # Exact evaluation layout only. Never search another installed application's
    # contents or import a bundled engine merely because it has matching names.
    if any(part.lower().endswith(".app") or "crossover" in part.lower()
           for part in root.parts):
        raise ImportError("An installed application is not an official evaluation image")
    lib = root / "redist/lib"
    framework = lib / "external/D3DMetal.framework"
    for directory in (lib, lib / "external", lib / "wine/x86_64-windows",
                      lib / "wine/x86_64-unix", framework):
        real_path(directory)
    image = real_path(args.image)
    if not image.is_file() or image.suffix.lower() != ".dmg":
        raise ImportError("The downloaded official evaluation DMG is required for provenance")
    image_hash = sha256(image)
    if args.expected_image_sha256 and image_hash != args.expected_image_sha256:
        raise ImportError("Apple image hash does not match the reviewed download")
    selected: dict[str, Path] = {}
    for name in DOCS:
        source = root / name
        if source.is_symlink() or not source.is_file():
            raise ImportError(f"Evaluation image is missing {name}")
        selected[f"notices/{name}"] = source
    framework_entries = entries(framework)
    signature = apple_framework_signature(framework)
    binary = framework / "Versions/A/D3DMetal"
    architectures = {"D3DMetal.framework": macho_arches(binary)}
    selected["lib/external/D3DMetal.framework"] = framework
    shared = lib / "external/libd3dshared.dylib"
    if shared.is_symlink() or not shared.is_file():
        raise ImportError("Evaluation image is missing the shared D3DMetal Unix bridge")
    architectures["libd3dshared.dylib"] = macho_arches(shared)
    selected["lib/external/libd3dshared.dylib"] = shared
    dlls = {}
    for name in CORE_DLLS + OPTIONAL_DLLS:
        dll = lib / "wine/x86_64-windows" / (name + ".dll")
        unix = lib / "wine/x86_64-unix" / (name + ".so")
        if not dll.exists() and name not in CORE_DLLS:
            continue
        if dll.is_symlink() or not dll.is_file():
            raise ImportError(f"Evaluation image is missing {name}.dll")
        dlls[name] = pe_machine(dll)
        selected[f"lib/wine/x86_64-windows/{name}.dll"] = dll
        # Preserve Apple's original Unix name/link layout. No alias generation
        # or mutation of framework/DLL/bridge contents is performed here.
        if not unix.exists():
            raise ImportError(f"Evaluation image is missing {name}.so")
        if unix.is_symlink():
            if os.readlink(unix) != "../../external/libd3dshared.dylib":
                raise ImportError("Apple Unix bridge has an unreviewed link target")
        elif unix.is_file():
            architectures[name + ".so"] = macho_arches(unix)
        else:
            raise ImportError("Apple Unix bridge is not a file or internal relative link")
        selected[f"lib/wine/x86_64-unix/{name}.so"] = unix
    manifest = []
    for relative, path in sorted(selected.items()):
        manifest.append(file_record(path, relative))
        if path == framework:
            manifest.extend(file_record(item, relative + "/" + item.relative_to(path).as_posix())
                            for item in sorted(framework_entries))
    if (len(manifest) > MAX_FILES
            or sum(item.get("size", 0) for item in manifest) > MAX_COMPONENT_SIZE):
        raise ImportError("Selected component exceeds the bounded import size")
    receipt = {"schema": 1, "component": "apple-d3dmetal-user-supplied",
               "official_source_url": validate_url(args.source_url),
               "source_image_sha256": image_hash,
               "source_image_size": image.stat().st_size,
               "license_sha256": sha256(root / "License.rtf"),
               "apple_framework_signature": signature,
               "host_architectures": architectures, "windows_dlls": dlls,
               "supports_x86_windows_apps": False,
               "wine_host_abi_verified": False,
               "d3d11_device_render_verified": False,
               "d3d12_device_render_verified": False,
               "backend_available": False,
               "distribution": {"private_import_only": True,
                                "public_release_approved": False,
                                "include_in_diagnostics": False},
               "files": manifest}
    return receipt, selected


def stage(args: argparse.Namespace, receipt: dict, selected: dict[str, Path]) -> None:
    if args.license_reviewed_sha256 != receipt["license_sha256"]:
        raise ImportError("A person must review the actual Apple license and supply its exact SHA-256")
    destination = output_path(args.output)
    source = real_path(args.evaluation_root)
    if destination == source or destination.is_relative_to(source) or source.is_relative_to(destination):
        raise ImportError("Output and official image directory must be disjoint")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".apple-component-", dir=destination.parent) as tmp:
        temporary = Path(tmp) / "component"
        temporary.mkdir()
        for relative, path in sorted(selected.items()):
            target = temporary / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if path.is_symlink():
                target.symlink_to(os.readlink(path))
            elif path.is_dir():
                # ditto preserves original framework signatures, links and
                # attributes. Re-signing Apple components is never implicit.
                run("/usr/bin/ditto", path, target)
            else:
                run("/usr/bin/ditto", path, target)
        for record in receipt["files"]:
            target = temporary / record["path"]
            if record["type"] == "file" and sha256(target) != record["sha256"]:
                raise ImportError("Imported component bytes changed during copying")
            if record["type"] == "symlink" and os.readlink(target) != record["target"]:
                raise ImportError("Imported component link changed during copying")
        apple_framework_signature(temporary / "lib/external/D3DMetal.framework")
        receipt["license_reviewed_sha256"] = args.license_reviewed_sha256
        (temporary / "component-receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        # Rename only to a new path, never replace an existing component.
        if destination.exists() or destination.is_symlink():
            raise ImportError("Output appeared during import; refusing to replace it")
        temporary.rename(destination)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("inspect", "stage"))
    parser.add_argument("--evaluation-root", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--source-url", required=True)
    parser.add_argument("--expected-image-sha256")
    parser.add_argument("--license-reviewed-sha256")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.operation == "stage" and (not args.output or not args.license_reviewed_sha256):
        parser.error("stage requires --output and --license-reviewed-sha256")
    if args.operation == "inspect" and (args.output or args.license_reviewed_sha256):
        parser.error("inspect never stages or accepts a license")
    for value in (args.expected_image_sha256, args.license_reviewed_sha256):
        if value is not None and not re.fullmatch(r"[0-9a-f]{64}", value):
            parser.error("SHA-256 values must be 64 lowercase hexadecimal characters")
    try:
        receipt, selected = inspect(args)
        if args.operation == "stage":
            stage(args, receipt, selected)
        print(json.dumps(receipt, indent=2, sort_keys=True))
        return 0
    except (ImportError, OSError, subprocess.SubprocessError, ValueError) as error:
        # Only our deliberately path-free diagnostics are printed publicly.
        message = str(error) if isinstance(error, ImportError) else type(error).__name__
        print("D3DMetal import refused: " + message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
