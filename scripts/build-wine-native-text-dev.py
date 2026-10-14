#!/usr/bin/env python3
"""Rebuild reviewed Wine drivers and child loaders in isolated clones.

No Wine process is launched, no bottle is opened and no installed app is
changed. The package keeps the existing translators and TLS stack.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


REPO = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "dxmt_package", REPO / "scripts/package-wine-runtime-dxmt-dev.py")
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)
BASE_IDENTITY = "4d82b50b96eb5995a78d848fb67166979577cd98e36d7db72fbb08636f8f44c5"
WINE_SHA512 = (
    "a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec"
    "611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b"
)
# Exact corresponding source and the reviewed extension.
PATCHES = {
    "wine-11.0-steam-cef-commandline.patch": "408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f",
    "wine-11.0-steam-dock.patch": "33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844",
    "wine-11.0-native-app-identity.patch": "f860b3a32a34d807319827c8a03298b1c24a705fa7075dc3a32cca7567c2d0ab",
    "wine-11.0-native-text-editing.patch": "4bd978c03d8788337bc609ad9ec24dc5c90fe7e2b371e9eaf4440eb5557cc80a",
    "wine-11.0-winemac-metal-exports.patch": "ada2df6599a9883689cda250cf53e46eb030205503054b1774d7bb1b67fe8235",
    "wine-11.0-dxmt-client-surface.patch": "9e6cc0516e87b37dc2dca98660726098d740a2449aff5a169a92a0c4d9b63a0a",
}
STEAM_WINDOW_PATCH = "wine-11.0-steam-native-window.patch"
PATCHES[STEAM_WINDOW_PATCH] = "c34c8c207febd966fc9aa7fb6cd8b31bd9325bcc820b6a35cc266f4164bc5412"
STEAM_BUNDLE_PATCH = "wine-11.0-steam-native-bundle.patch"
PATCHES[STEAM_BUNDLE_PATCH] = "faf14b29f591f66b82b38a4eee97a842675aa29f66960ec499429f27ac6be09a"
BASE_PATCHES = ["wine-11.0-steam-cef-commandline.patch", "wine-11.0-steam-dock.patch",
                "wine-11.0-native-app-identity.patch"]
TEXT_PATCH = "wine-11.0-native-text-editing.patch"
UI_PATCHES = [TEXT_PATCH, STEAM_WINDOW_PATCH, STEAM_BUNDLE_PATCH]
DXMT_PATCHES = ["wine-11.0-winemac-metal-exports.patch", "wine-11.0-dxmt-client-surface.patch"]
DRIVERS = {
    "lib/wine/x86_64-unix/winemac.so": "winemac.so",
    "lib/wine/x86_64-windows/winemac.drv": "x86_64-windows/winemac.drv",
    "lib/wine/i386-windows/winemac.drv": "i386-windows/winemac.drv",
}


def apply_patch(source: Path, name: str) -> None:
    with (REPO / "runtime-patches" / name).open("rb") as stream:
        result = subprocess.run(["/usr/bin/patch", "--batch", "-p1", "-F", "0"],
                                cwd=source, stdin=stream, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT)
    if result.returncode:
        raise PACKAGE.PackageError("Reviewed source patch did not apply exactly: " + name)


def tree_files(root: Path) -> dict:
    entries = {}
    for current, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(current) / name
            relative = str(path.relative_to(root))
            if path.is_symlink():
                entries[relative] = ("link", os.readlink(path))
            elif path.is_dir():
                entries[relative] = ("directory", path.stat().st_mode & 0o777)
            elif path.is_file():
                entries[relative] = ("file", path.stat().st_mode & 0o777, PACKAGE.sha256(path))
            else:
                raise PACKAGE.PackageError("Source contains a special file")
    return entries


def validate(base: Path, seed: Path, output: Path) -> None:
    for path in (base, seed, output):
        PACKAGE.owned_path(path)
    if output.exists() or output.is_symlink():
        raise PACKAGE.PackageError("Output exists; refusing to replace it")
    if any(output == path or output in path.parents or path in output.parents
           for path in (base, seed)):
        raise PACKAGE.PackageError("Output and inputs must be disjoint")
    if PACKAGE.tree_identity(base) != BASE_IDENTITY:
        raise PACKAGE.PackageError("Base is not the exact reviewed switchable runtime")
    archive = base / "source/wine-11.0.tar.xz"
    if hashlib.sha512(archive.read_bytes()).hexdigest() != WINE_SHA512:
        raise PACKAGE.PackageError("Wine source archive differs from the reviewed upstream input")
    for name, expected in PATCHES.items():
        if PACKAGE.sha256(REPO / "runtime-patches" / name) != expected:
            raise PACKAGE.PackageError("Reviewed patch hash differs: " + name)
    for name in ("Makefile", "config.status", "include/config.h"):
        if not (seed / "build" / name).is_file():
            raise PACKAGE.PackageError("Missing configured independent Wine build")
    # Check all Wine sources, rather than reverse-checking only patch hunks.
    with tempfile.TemporaryDirectory(prefix=".native-text-source-", dir=REPO / ".build") as temp:
        scratch = Path(temp)
        PACKAGE.run("/usr/bin/tar", "-xJf", archive, "-C", scratch)
        source = scratch / "wine-11.0"
        for name in BASE_PATCHES:
            apply_patch(source, name)
        if tree_files(source) != tree_files(seed / "src/wine-11.0"):
            raise PACKAGE.PackageError("Seed source differs from exact upstream plus reviewed patches")
        for name in UI_PATCHES:
            apply_patch(source, name)
        for name in DXMT_PATCHES:
            apply_patch(source, name)


def build_driver(seed: Path, target: Path, patches: list[str]) -> None:
    PACKAGE.run("/bin/cp", "-cRp", seed, target)
    build = target / "build"
    for name in ("Makefile", "config.status", "config.log", "include/config.h"):
        path = build / name
        if not path.is_file():
            continue
        original = path.stat()
        path.write_bytes(path.read_bytes().replace(os.fsencode(seed), os.fsencode(target)))
        os.utime(path, ns=(original.st_atime_ns, original.st_mtime_ns))
    for name in patches:
        apply_patch(target / "src/wine-11.0", name)
    # Recompile the entire driver and both Windows halves. Reusing the old
    # Unix driver would leave incompatible event declarations in some objects.
    driver = build / "dlls/winemac.drv"
    for path in driver.rglob("*"):
        if path.is_file() and (path.suffix == ".o" or path.name in ("winemac.so", "winemac.drv")):
            path.unlink()
    # The Steam bundle extension changes only the native child-loader code.
    # Force that object to rebuild even if a copied make timestamp is newer.
    for relative in ("dlls/ntdll/unix/loader.o", "dlls/ntdll/ntdll.so"):
        (build / relative).unlink(missing_ok=True)
    with (target / "driver-build.log").open("wb") as log:
        result = subprocess.run(["/usr/bin/make", "-j8", "dlls/winemac.drv/all", "dlls/ntdll/ntdll.so"],
                                cwd=build, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        raise PACKAGE.PackageError("Isolated driver build failed; see its local driver-build.log")


def driver_inputs(output: Path) -> dict[str, Path]:
    inputs = {
        prefix + "/" + relative: output / variant / "build/dlls/winemac.drv" / artifact
        for variant, prefix in (("base-build", "runtime"),
                                ("dxmt-build", "components/dxmt/runtime"))
        for relative, artifact in DRIVERS.items()
    }
    for variant, prefix in (("base-build", "runtime"),
                            ("dxmt-build", "components/dxmt/runtime")):
        inputs[prefix + "/lib/wine/x86_64-unix/ntdll.so"] = (
            output / variant / "build/dlls/ntdll/ntdll.so")
    return inputs


def verify_drivers(inputs: dict[str, Path], base: Path) -> None:
    for relative, path in inputs.items():
        if relative.endswith(".so"):
            if PACKAGE.run("/usr/bin/lipo", "-archs", path) != "x86_64":
                raise PACKAGE.PackageError("Unexpected Wine driver architecture")
            symbols = {line.split()[-1] for line in PACKAGE.run("/usr/bin/nm", "-gU", path).splitlines()}
            if relative.endswith("/ntdll.so"):
                previous_symbols = {line.split()[-1] for line in PACKAGE.run(
                    "/usr/bin/nm", "-gU", base / relative).splitlines()}
                if symbols != previous_symbols:
                    raise PACKAGE.PackageError("Steam child loader changed its native exports")
                continue
            expected = {"___wine_unix_call_funcs", "___wine_unix_call_wow64_funcs"}
            if relative.startswith("components/"):
                expected |= PACKAGE.EXPORTS
            if symbols != expected:
                raise PACKAGE.PackageError("Rebuilt driver differs from the exact reviewed exports")
        elif PACKAGE.pe_machine(path) != (0x8664 if "x86_64-windows" in relative else 0x14c):
            raise PACKAGE.PackageError("Unexpected Windows driver architecture")


def package(base: Path, output: Path) -> str:
    inputs = driver_inputs(output)
    verify_drivers(inputs, base)
    candidate = output / "runtime-package"
    PACKAGE.run("/bin/cp", "-cRp", base, candidate)
    original = tree_files(base)
    receipt = json.loads((candidate / "components.json").read_text())
    for relative, source in inputs.items():
        destination = candidate / relative
        PACKAGE.no_symlink_parents(destination)
        shutil.copy2(source, destination)
        if relative.endswith(".so"):
            PACKAGE.repair_image(candidate, destination)
        entry = {"input_sha256": PACKAGE.sha256(source),
                 "packaged_sha256": PACKAGE.sha256(destination)}
        section = "artifacts" if relative.startswith("components/") else "base_artifacts"
        receipt[section][relative] = entry
    for name in UI_PATCHES:
        receipt["patches"][name] = PATCHES[name]
    receipt["native_text_editing"] = {
        "format": "caskrun-native-text-editing-v2",
        "context_menu_policy": "verified-original-menu-retired-before-native-menu",
        "clipboard_contents_read_by_menu": False,
        "game_validation": False,
    }
    receipt["native_steam_window"] = {
        "format": "caskrun-steam-signin-window-v1",
        "close_action": "native-hide",
        "client_geometry_changed": False,
        "game_validation": False,
    }
    receipt["native_steam_bundle"] = {
        "format": "caskrun-steam-native-bundle-v1",
        "bundled_process": "steamwebhelper.exe",
        "preprepared_alias": True,
        "child_core_callback": False,
        "explicit_background_helpers_use_raw_loader": True,
        "game_validation": False,
    }
    (candidate / "components.json").write_text(json.dumps(receipt, indent=2) + "\n")
    for name in UI_PATCHES:
        shutil.copyfile(REPO / "runtime-patches" / name, candidate / "source/patches" / name)
    (candidate / "native-text-editing.txt").write_text("caskrun-native-text-editing-v2\n")
    with (candidate / "SOURCES.txt").open("a") as stream:
        stream.write("\nOriginal macOS native text editing extension: source/patches/" + TEXT_PATCH + "\n"
                     "SHA-256: " + PATCHES[TEXT_PATCH] + "\n"
                     "Both base and DXMT Wine drivers rebuilt from the exact corresponding Wine archive\n"
                     "and reviewed patches. No clipboard or field contents are read by the native menu.\n"
                     "Translators, TLS dependencies and all unrelated engine files retained.\n"
                     "Original Steam sign-in native controls: source/patches/" + STEAM_WINDOW_PATCH + "\n"
                     "SHA-256: " + PATCHES[STEAM_WINDOW_PATCH] + "\n"
                     "Only the recognized sign-in window gets native Hide/Minimize controls.\n"
                     "Client geometry, app files and explicit Quit behavior are retained.\n"
                     "Original Steam web-helper bundle reuse: source/patches/" + STEAM_BUNDLE_PATCH + "\n"
                     "SHA-256: " + PATCHES[STEAM_BUNDLE_PATCH] + "\n"
                     "Both native child loaders retain their exports. Only Steam's web helper uses\n"
                     "the preprepared per-bottle app bundle; no child Core callback is added.\n"
                     "Build: scripts/build-wine-native-text-dev.py; not a public binary release.\n")
    allowed = set(inputs) | {"components.json", "SOURCES.txt", "native-text-editing.txt"} | {
        "source/patches/" + name for name in UI_PATCHES}
    updated = tree_files(candidate)
    changes = {name for name in original.keys() | updated.keys()
               if original.get(name) != updated.get(name)}
    if not changes <= allowed:
        raise PACKAGE.PackageError("Package changed an unrelated engine file")
    PACKAGE.audit(candidate)
    if PACKAGE.tree_identity(base) != BASE_IDENTITY:
        raise PACKAGE.PackageError("Base changed while building")
    return PACKAGE.tree_identity(candidate)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, default=REPO / ".build/dxmt-wine-runtime-dev-v4")
    parser.add_argument("--seed", type=Path, default=REPO / ".build/wine-native-apps-11.0-dev")
    parser.add_argument("--output", type=Path, default=REPO / ".build/native-text-runtime-v9")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        base, seed, output = [PACKAGE.owned_path(path) for path in (args.base, args.seed, args.output)]
        validate(base, seed, output)
        if args.check:
            print("Exact base, full Wine source, patch order and disjoint ignored paths verified.")
            return 0
        output.mkdir()
        build_driver(seed, output / "base-build", UI_PATCHES)
        build_driver(seed, output / "dxmt-build", UI_PATCHES + DXMT_PATCHES)
        print("Candidate runtime identity: sha256:" + package(base, output))
        return 0
    except (OSError, PACKAGE.PackageError) as error:
        print(str(error), file=__import__("sys").stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
