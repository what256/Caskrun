#!/usr/bin/env python3
"""Rebuild only Wine's Unix Mac drivers from exact reviewed V12 sources.

Build dependencies stay in ignored scratch. The candidate replaces only the
base and DXMT winemac.so images; every other runtime binary is preserved.
Never launches Wine or accesses an installed application or user bottle.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile

REPO = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("native_text_build", REPO / "scripts/build-wine-native-text-dev.py")
TEXT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TEXT)
PACKAGE = TEXT.PACKAGE
BASE_IDENTITY = "cae84f3978b2e0820bbdfc95c1e9dd5fa6c79c8d1f1aca825ac86bd2afb77c29"
REOPEN_PATCH = "wine-11.0-steam-reopen.patch"
REOPEN_SHA256 = "83e4f375644906163239d37581d1f64409cd4d88a21ec3aec82ef63c56fcacbe"
BISON_URL = "https://ftp.gnu.org/gnu/bison/bison-3.8.2.tar.xz"
BISON_SHA256 = "9bba0214ccf7f1079c5d59210045227bcf619519840ebfa80cd3849cff5a5bf2"
DRIVER_PATHS = (
    "runtime/lib/wine/x86_64-unix/winemac.so",
    "components/dxmt/runtime/lib/wine/x86_64-unix/winemac.so",
)


def logged(command: list[str], cwd: Path, log: Path, env: dict | None = None) -> None:
    with log.open("wb") as stream:
        result = subprocess.run(command, cwd=cwd, env=env, stdout=stream, stderr=subprocess.STDOUT)
    if result.returncode:
        raise PACKAGE.PackageError("Driver build step failed; inspect the local " + log.name)


def apply(source: Path, name: str) -> None:
    expected = REOPEN_SHA256 if name == REOPEN_PATCH else TEXT.PATCHES.get(name)
    if expected is None or PACKAGE.sha256(REPO / "runtime-patches" / name) != expected:
        raise PACKAGE.PackageError("Reviewed patch changed before source replay: " + name)
    TEXT.apply_patch(source, name)


def extract(archive: Path, destination: Path) -> Path:
    destination.mkdir(parents=True)
    PACKAGE.run("/usr/bin/tar", "-xJf", archive, "-C", destination)
    return destination / "wine-11.0"


def validate_inputs(base: Path) -> Path:
    if PACKAGE.tree_identity(base) != BASE_IDENTITY:
        raise PACKAGE.PackageError("Base is not the exact reviewed V12 runtime")
    archive = base / "source/wine-11.0.tar.xz"
    if hashlib.sha512(archive.read_bytes()).hexdigest() != TEXT.WINE_SHA512:
        raise PACKAGE.PackageError("Wine source archive differs from the reviewed WineHQ input")
    for name, expected in {**TEXT.PATCHES, REOPEN_PATCH: REOPEN_SHA256}.items():
        if PACKAGE.sha256(REPO / "runtime-patches" / name) != expected:
            raise PACKAGE.PackageError("Reviewed patch hash differs: " + name)
        if name != REOPEN_PATCH and PACKAGE.sha256(base / "source/patches" / name) != expected:
            raise PACKAGE.PackageError("V12 corresponding patch differs: " + name)
    changed_sources = re.findall(r"^(?:--- a/|\+\+\+ b/)([^\t\n]+)",
                                 (REPO / "runtime-patches" / REOPEN_PATCH).read_text(), re.MULTILINE)
    if not changed_sources or set(changed_sources) != {"dlls/winemac.drv/cocoa_app.m"}:
        raise PACKAGE.PackageError("Reopen patch changes more than the Unix Cocoa application delegate")
    return archive


def prepare_source(archive: Path, output: Path, resume: bool) -> Path:
    source = output / "src/wine-11.0"
    if not resume:
        source = extract(archive, output / "src")
        for name in TEXT.BASE_PATCHES + TEXT.UI_PATCHES:
            apply(source, name)
        apply(source, REOPEN_PATCH)
        return source
    # Resume only an exact source replay, never an arbitrary old compiler tree.
    # The small baseline reconstruction can be reused after its source is checked.
    with tempfile.TemporaryDirectory(prefix=".store-reopen-source-check-", dir=REPO / ".build") as temporary:
        expected = extract(archive, Path(temporary) / "src")
        for name in TEXT.BASE_PATCHES + TEXT.UI_PATCHES:
            apply(expected, name)
        current = TEXT.tree_files(source)
        if current == TEXT.tree_files(expected):
            apply(source, REOPEN_PATCH)
        apply(expected, REOPEN_PATCH)
        if TEXT.tree_files(source) != TEXT.tree_files(expected):
            raise PACKAGE.PackageError("Resume source differs from exact V12 plus reviewed reopen patch")
    return source


def build_bison(output: Path) -> Path:
    executable = output / "toolchain/bin/bison"
    archive = output / "tool-sources/bison-3.8.2.tar.xz"
    archive.parent.mkdir(exist_ok=True)
    if not archive.exists():
        PACKAGE.run("/usr/bin/curl", "-q", "--fail", "--silent", "--show-error", "--proto", "=https",
                    "--max-redirs", "0", "--connect-timeout", "15", "--max-time", "120",
                    "--max-filesize", "4000000", BISON_URL, "-o", archive)
    if PACKAGE.sha256(archive) != BISON_SHA256:
        raise PACKAGE.PackageError("Pinned GNU Bison source checksum differs")
    if executable.exists():
        if not PACKAGE.run(executable, "--version").startswith("bison (GNU Bison) 3.8.2\n"):
            raise PACKAGE.PackageError("Existing local Bison is not the pinned version")
        return executable
    source = archive.parent / "bison-3.8.2"
    if not source.exists():
        PACKAGE.run("/usr/bin/tar", "-xJf", archive, "-C", archive.parent)
    logged(["./configure", "--prefix=" + str(output / "toolchain"), "--disable-nls"],
           source, output / "bison-configure.log")
    logged(["/usr/bin/make", "-j4"], source, output / "bison-make.log")
    logged(["/usr/bin/make", "install"], source, output / "bison-install.log")
    return executable


def configure(source: Path, output: Path, bison: Path, resume: bool) -> Path:
    build = output / "base-build"
    build.mkdir(exist_ok=True)
    if resume and (build / "Makefile").exists():
        makefile = (build / "Makefile").read_text()
        if "CC = /usr/bin/clang -target x86_64-apple-macos13 -m64\n" not in makefile or \
                "srcdir = ../src/wine-11.0\n" not in makefile or \
                "CFLAGS = -g -O2 -U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0\n" not in makefile:
            raise PACKAGE.PackageError("Resume compiler configuration differs")
    else:
        empty_pkgconfig = output / "empty-pkgconfig"
        empty_pkgconfig.mkdir(exist_ok=True)
        environment = dict(os.environ)
        for name in ("PKG_CONFIG_PATH", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "LIBS",
                     "OBJC", "OBJCFLAGS", "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH"):
            environment.pop(name, None)
        environment.update(PKG_CONFIG_LIBDIR=str(empty_pkgconfig),
                           CC="/usr/bin/clang -target x86_64-apple-macos13",
                           CXX="/usr/bin/clang++ -target x86_64-apple-macos13", BISON=str(bison))
        logged(["/usr/bin/arch", "-x86_64", "/bin/sh", "../src/wine-11.0/configure",
                "--build=x86_64-apple-darwin", "--enable-archs=i386,x86_64", "--disable-tests",
                "--without-x", "--without-gstreamer", "--without-gnutls", "--without-freetype",
                "--prefix=" + str(output / "unused-stage")], build, output / "base-configure.log", environment)
    expected_configuration = [
        "--build=x86_64-apple-darwin", "--enable-archs=i386,x86_64", "--disable-tests",
        "--without-x", "--without-gstreamer", "--without-gnutls", "--without-freetype",
        "--prefix=" + str(output / "unused-stage"), "build_alias=x86_64-apple-darwin",
        "CC=/usr/bin/clang -target x86_64-apple-macos13",
        "CXX=/usr/bin/clang++ -target x86_64-apple-macos13",
    ]
    if shlex.split(PACKAGE.run("/bin/sh", build / "config.status", "--config")) != expected_configuration:
        raise PACKAGE.PackageError("Configured Wine options differ from the reviewed driver-only build")
    if "#define HAVE_MTLDEVICE_REGISTRYID 1\n" not in (build / "include/config.h").read_text():
        raise PACKAGE.PackageError("Driver would lose the reviewed Metal device registry-ID path")
    return build


def build_driver(build: Path, output: Path, variant: str) -> Path:
    driver = build / "dlls/winemac.drv"
    # Rebuild all driver objects when shared declarations change between views.
    if driver.exists():
        for path in driver.glob("*.o"):
            if path.is_symlink() or not path.is_file():
                raise PACKAGE.PackageError("Unexpected driver object entry")
            path.unlink()
        (driver / "winemac.so").unlink(missing_ok=True)
    logged(["/usr/bin/make", "-j4", "dlls/winemac.drv/winemac.so"],
           build, output / (variant + "-driver-build.log"))
    destination = output / "artifacts" / variant / "winemac.so"
    destination.parent.mkdir(parents=True)
    shutil.copy2(driver / "winemac.so", destination)
    return destination


def verify_driver(path: Path, base: Path, relative: str) -> None:
    if PACKAGE.run("/usr/bin/lipo", "-archs", path) != "x86_64":
        raise PACKAGE.PackageError("Rebuilt Unix driver must be exactly x86_64")
    symbols = lambda image: {line.split()[-1] for line in PACKAGE.run("/usr/bin/nm", "-gU", image).splitlines()}
    if symbols(path) != symbols(base / relative):
        raise PACKAGE.PackageError("Rebuilt driver changed its exact reviewed exports")
    if b"registryID\0" not in path.read_bytes():
        raise PACKAGE.PackageError("Rebuilt driver lost Metal registry-ID support")
    normalize = lambda dependency: dependency.replace("@rpath/", "@loader_path/")
    if {normalize(item) for item in PACKAGE.dependencies(path)} != \
            {normalize(item) for item in PACKAGE.dependencies(base / relative)}:
        raise PACKAGE.PackageError("Rebuilt driver changed its reviewed framework/runtime dependencies")


def package(base: Path, output: Path, drivers: list[Path]) -> str:
    candidate = output / "runtime-package"
    if candidate.exists() or candidate.is_symlink():
        raise PACKAGE.PackageError("Candidate already exists; refusing to replace it")
    original = TEXT.tree_files(base)
    PACKAGE.run("/bin/cp", "-cRp", base, candidate)
    receipt = json.loads((candidate / "components.json").read_text())
    if len(drivers) != len(DRIVER_PATHS):
        raise PACKAGE.PackageError("Both exact Unix driver outputs are required")
    for source, relative in zip(drivers, DRIVER_PATHS):
        verify_driver(source, base, relative)
        destination = PACKAGE.no_symlink_parents(candidate / relative)
        shutil.copy2(source, destination)
        PACKAGE.repair_image(candidate, destination)
        section = "artifacts" if relative.startswith("components/") else "base_artifacts"
        receipt[section][relative] = {"input_sha256": PACKAGE.sha256(source),
                                      "packaged_sha256": PACKAGE.sha256(destination)}
    receipt["patches"][REOPEN_PATCH] = REOPEN_SHA256
    receipt["native_steam_reopen"] = {
        "format": "caskrun-steam-window-reopen-v1",
        "predecessor_runtime_identity": "sha256:" + BASE_IDENTITY,
        "prepared_steam_ui_identity_only": True,
        "requires_prior_drawn_client_window": True,
        "prior_client_window_minimum_points": [600, 320],
        "initial_zero_window_reopen_ignored": True,
        "wine_unminimize_path_retained": True,
        "core_request": "request-steam-show",
        "client_argument": "steam://open/main",
        "cocoa_forces_window_visible": False,
        "show_then_quit_serialized": True,
        "quit_submission_hold_seconds": 5,
        "windows_driver_abi_changed": False,
        "game_validation": False,
    }
    (candidate / "components.json").write_text(json.dumps(receipt, indent=2) + "\n")
    shutil.copyfile(REPO / "runtime-patches" / REOPEN_PATCH, candidate / "source/patches" / REOPEN_PATCH)
    if PACKAGE.sha256(candidate / "source/patches" / REOPEN_PATCH) != REOPEN_SHA256:
        raise PACKAGE.PackageError("Corresponding reopen source changed during packaging")
    with (candidate / "SOURCES.txt").open("a") as stream:
        stream.write("\nOriginal native window reopen: source/patches/" + REOPEN_PATCH + "\nSHA-256: " + REOPEN_SHA256 + "\n"
                     "Prepared visible Steam browser may ask verified Core for steam://open/main; Cocoa never forces a Windows-hidden window visible.\n"
                     "Recovery requires this helper to have presented a drawn client-sized window before Windows ordered it out; initial zero-window reopen submits no request.\n"
                     "Only both Unix winemac.so drivers rebuilt; Windows drivers and all other runtime binaries retained.\n"
                     "Minimal driver build omits unused TLS/font development packages; packaged TLS/font bytes are unchanged.\n"
                     "Build: scripts/build-wine-steam-reopen-dev.py; not a public binary release.\n")
    allowed = set(DRIVER_PATHS) | {"components.json", "SOURCES.txt", "source/patches/" + REOPEN_PATCH}
    updated = TEXT.tree_files(candidate)
    changed = {name for name in original.keys() | updated.keys() if original.get(name) != updated.get(name)}
    if changed != allowed:
        raise PACKAGE.PackageError("Candidate changed an unexpected file or omitted a required update")
    for prefix in ("runtime", "components/dxmt/runtime"):
        for architecture, machine in (("i386", 0x14c), ("x86_64", 0x8664)):
            relative = f"{prefix}/lib/wine/{architecture}-windows/winemac.drv"
            if PACKAGE.pe_machine(candidate / relative) != machine or \
                    PACKAGE.sha256(candidate / relative) != PACKAGE.sha256(base / relative):
                raise PACKAGE.PackageError("Windows driver ABI/image changed")
    if (candidate / "native-text-editing.txt").read_bytes() != (base / "native-text-editing.txt").read_bytes():
        raise PACKAGE.PackageError("Native editing protocol changed")
    PACKAGE.audit(candidate)
    if PACKAGE.tree_identity(base) != BASE_IDENTITY:
        raise PACKAGE.PackageError("Reviewed V12 base changed during the build")
    identity = PACKAGE.tree_identity(candidate)
    (output / "BUILD-RESULT.json").write_text(json.dumps({
        "format": "caskrun-native-reopen-driver-build-v1", "base_identity": "sha256:" + BASE_IDENTITY,
        "candidate_identity": "sha256:" + identity, "changed_files": sorted(changed),
        "wine_sha512": TEXT.WINE_SHA512, "patches": {**TEXT.PATCHES, REOPEN_PATCH: REOPEN_SHA256},
        "target": "x86_64-apple-macos13", "make_target": "dlls/winemac.drv/winemac.so",
        "bison_source_sha256": BISON_SHA256, "tls_font_runtime_preserved": True,
        "live_validation": False,
    }, indent=2) + "\n")
    return identity


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, default=REPO / ".build/native-text-wine-runtime-dev-v12")
    parser.add_argument("--output", type=Path, default=REPO / ".build/store-reopen-runtime")
    parser.add_argument("--resume", action="store_true", help="verify and reuse a V12-only source/configure scratch")
    parser.add_argument("--check", action="store_true", help="verify exact inputs and replay source patches only")
    arguments = parser.parse_args()
    try:
        base, output = (PACKAGE.owned_path(path) for path in (arguments.base, arguments.output))
        if base == output or base in output.parents or output in base.parents:
            raise PACKAGE.PackageError("Base and output must be disjoint")
        archive = validate_inputs(base)
        if arguments.check:
            with tempfile.TemporaryDirectory(prefix=".store-reopen-check-", dir=REPO / ".build") as temporary:
                source = extract(archive, Path(temporary) / "src")
                for name in TEXT.BASE_PATCHES + TEXT.UI_PATCHES + [REOPEN_PATCH] + TEXT.DXMT_PATCHES:
                    apply(source, name)
            print("Exact V12, upstream Wine, reviewed patch pins and both view replays verified.")
            return 0
        if output.exists() and not arguments.resume:
            raise PACKAGE.PackageError("Output exists; use --resume only for reviewed prepared source")
        output.mkdir(exist_ok=True)
        if (output / "artifacts").exists():
            raise PACKAGE.PackageError("Build artifacts already exist; refusing stale driver reuse")
        source = prepare_source(archive, output, arguments.resume)
        build = configure(source, output, build_bison(output), arguments.resume)
        drivers = [build_driver(build, output, "base")]
        for name in TEXT.DXMT_PATCHES:
            apply(source, name)
        drivers.append(build_driver(build, output, "dxmt"))
        print("Candidate runtime identity: sha256:" + package(base, output, drivers))
        return 0
    except (OSError, subprocess.SubprocessError, PACKAGE.PackageError) as error:
        print("Driver rebuild refused: " + str(error), file=__import__("sys").stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
