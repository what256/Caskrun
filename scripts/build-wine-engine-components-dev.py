#!/usr/bin/env python3
"""Build Vulkan-capable Wine components without replacing a working runtime.

Use only exact Wine sources and Caskrun's reviewed native-window patches.
Development headers match the retained font/TLS libraries; those libraries are
not rebuilt or replaced. Nothing launches Wine, an installer, or a user bottle.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import shutil
import subprocess

REPO = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("reopen_build", REPO / "scripts/build-wine-steam-reopen-dev.py")
REOPEN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REOPEN)
PACKAGE = REOPEN.PACKAGE
BASE_IDENTITY = "398d08ea874ad53c8c8f6519e291a990f9d598dde0dfebc88fb149fe6d6c1c54"
MSYNC_PATCH = "wine-11.0-msync.patch"
MSYNC_SHA256 = "eb08bab72993586aba3d32d694acaf2719a94987a4edbd503b0f009473b088cd"
PORTABILITY_PATCH = "wine-11.0-vulkan-portability.patch"
PORTABILITY_SHA256 = "555fad8796196118400e04eeb69fbfd4888fb91d11ab846b5b877d2cd0c76044"
SOURCES = (
    ("freetype-2.14.3", "https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz",
     "36bc4f1cc413335368ee656c42afca65c5a3987e8768cc28cf11ba775e785a5f"),
    ("gnutls-3.8.13", "https://www.gnupg.org/ftp/gcrypt/gnutls/v3.8/gnutls-3.8.13.tar.xz",
     "ffed8ec1bf09c2426d4f14aae377de4753b53e537d685e604e99a8b16ca9c97e"),
)


def headers(base: Path, work: Path) -> Path:
    deps = work / "deps"
    deps.mkdir(exist_ok=True)
    for name, url, digest in SOURCES:
        archive = deps / (name + ".tar.xz")
        if not archive.exists():
            PACKAGE.run("/usr/bin/curl", "-q", "--fail", "--location", "--proto", "=https",
                        "--proto-redir", "=https", "--max-time", "180", "--max-filesize", "15000000",
                        url, "-o", archive)
        if PACKAGE.sha256(archive) != digest:
            raise PACKAGE.PackageError("Dependency source checksum differs: " + name)
        if not (deps / name).exists():
            PACKAGE.run("/usr/bin/tar", "-xJf", archive, "-C", deps)
    prefix = work / "development-headers"
    include = prefix / "include"
    lib = prefix / "lib"
    include.mkdir(parents=True, exist_ok=True)
    lib.mkdir(exist_ok=True)
    shutil.copytree(deps / "freetype-2.14.3/include", include / "freetype2", dirs_exist_ok=True)
    gnutls = deps / "gnutls-3.8.13/lib/includes/gnutls"
    shutil.copytree(gnutls, include / "gnutls", dirs_exist_ok=True)
    # These are configure.ac's substitutions for this exact Darwin version.
    # No ABI structure is invented; sys/uio.h supplies the target's own iovec.
    substitutions = {"VERSION": "3.8.13", "MAJOR_VERSION": "3", "MINOR_VERSION": "8",
                     "PATCH_VERSION": "13", "NUMBER_VERSION": "0x03080d",
                     "DEFINE_IOVEC_T": "#include <sys/uio.h>\ntypedef struct iovec giovec_t;"}
    text = (gnutls / "gnutls.h.in").read_text()
    for name, value in substitutions.items():
        text = text.replace("@" + name + "@", value)
    if re.search(r"@[A-Za-z_0-9]+@", text):
        raise PACKAGE.PackageError("Unresolved GnuTLS public-header substitution")
    (include / "gnutls/gnutls.h").write_text(text)
    for source, name in ((base / "lib/libfreetype.6.dylib", "freetype"),
                         (base / "lib/libgnutls.30.dylib", "gnutls")):
        link = lib / ("lib" + name + ".dylib")
        if link.exists() or link.is_symlink():
            if not link.is_symlink() or link.resolve() != source.resolve():
                raise PACKAGE.PackageError("Development library link differs")
        else:
            link.symlink_to(source)
    return prefix


def configure(base: Path, work: Path, sdk: Path, bison: Path) -> Path:
    if PACKAGE.tree_identity(base) != BASE_IDENTITY:
        raise PACKAGE.PackageError("Base differs from the exact reviewed V14 runtime")
    work.mkdir(parents=True, exist_ok=True)
    archive = base / "source/wine-11.0.tar.xz"
    if hashlib.sha512(archive.read_bytes()).hexdigest() != REOPEN.TEXT.WINE_SHA512:
        raise PACKAGE.PackageError("Wine source archive checksum differs")
    if (work / "src/wine-11.0").exists():
        raise PACKAGE.PackageError("Use a fresh work directory; a partially patched source tree is not resumed")
    source = REOPEN.prepare_source(archive, work, False)
    msync = REPO / "runtime-patches" / MSYNC_PATCH
    if PACKAGE.sha256(msync) != MSYNC_SHA256:
        raise PACKAGE.PackageError("Reviewed MSync port checksum differs")
    REOPEN.TEXT.apply_patch(source, MSYNC_PATCH)
    if PACKAGE.sha256(REPO / "runtime-patches" / PORTABILITY_PATCH) != PORTABILITY_SHA256:
        raise PACKAGE.PackageError("Reviewed Vulkan portability patch checksum differs")
    REOPEN.TEXT.apply_patch(source, PORTABILITY_PATCH)
    prefix = headers(base, work)
    if not (sdk / "include/vulkan/vulkan.h").is_file() or not (sdk / "lib/libvulkan.dylib").is_file():
        raise PACKAGE.PackageError("Vulkan SDK headers and loader are required")
    if "x86_64" not in PACKAGE.run("/usr/bin/lipo", "-archs", sdk / "lib/libvulkan.dylib"):
        raise PACKAGE.PackageError("Vulkan loader has no x86_64 slice")
    build = work / "build"
    build.mkdir(exist_ok=True)
    environment = dict(os.environ)
    for name in ("PKG_CONFIG_PATH", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "LIBS",
                 "OBJC", "OBJCFLAGS", "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH"):
        environment.pop(name, None)
    empty = work / "empty-pkgconfig"
    empty.mkdir(exist_ok=True)
    environment.update(PKG_CONFIG_LIBDIR=str(empty),
                       CC="/usr/bin/clang -target x86_64-apple-macos13",
                       CXX="/usr/bin/clang++ -target x86_64-apple-macos13", BISON=str(bison),
                       FREETYPE_CFLAGS="-I" + str(prefix / "include/freetype2"),
                       FREETYPE_LIBS="-L" + str(prefix / "lib") + " -lfreetype",
                       GNUTLS_CFLAGS="-I" + str(prefix / "include"),
                       GNUTLS_LIBS="-L" + str(prefix / "lib") + " -lgnutls",
                       CPPFLAGS="-I" + str(sdk / "include"), LDFLAGS="-L" + str(sdk / "lib"))
    if not (build / "Makefile").exists():
        REOPEN.logged(["/usr/bin/arch", "-x86_64", "/bin/sh", str(source / "configure"),
                       "--build=x86_64-apple-darwin", "--enable-archs=i386,x86_64", "--disable-tests",
                       "--without-x", "--without-gstreamer", "--with-gnutls", "--with-freetype",
                       "--with-vulkan", "--prefix=" + str(work / "unused-stage")],
                      build, work / "configure.log", environment)
    config = (build / "include/config.h").read_text()
    required = ('#define SONAME_LIBVULKAN "libvulkan.1.dylib"',
                '#define SONAME_LIBFREETYPE "libfreetype.6.dylib"',
                '#define SONAME_LIBGNUTLS "libgnutls.30.dylib"',
                '#define HAVE_GNUTLS_CIPHER_INIT 1', '#define HAVE_MTLDEVICE_REGISTRYID 1')
    if not all(line + "\n" in config for line in required):
        raise PACKAGE.PackageError("Vulkan/font/TLS/Metal configuration gate failed")
    return build


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--bison", type=Path, required=True)
    parser.add_argument("--configure-only", action="store_true")
    args = parser.parse_args()
    base, work, sdk, bison = (p.resolve() for p in (args.base, args.work, args.sdk, args.bison))
    for path in (base, work, sdk, bison):
        PACKAGE.owned_path(path)
    build = configure(base, work, sdk, bison)
    if not args.configure_only:
        REOPEN.logged(["/usr/bin/make", "-j4", "dlls/winemac.drv/all", "dlls/win32u/all",
                       "dlls/winevulkan/all", "dlls/vulkan-1/all", "dlls/ntdll/ntdll.so",
                       "server/wineserver"], build, work / "components-build.log")
        artifacts = work / "artifacts/base"
        artifacts.mkdir(parents=True, exist_ok=True)
        modules = (("winemac.drv", "winemac.so", "winemac.drv"),
                   ("win32u", "win32u.so", "win32u.dll"),
                   ("winevulkan", "winevulkan.so", "winevulkan.dll"),
                   ("vulkan-1", None, "vulkan-1.dll"))
        shutil.copy2(build / "server/wineserver", work / "artifacts/wineserver")
        target = artifacts / "x86_64-unix/ntdll.so"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(build / "dlls/ntdll/ntdll.so", target)
        for directory, unix, pe in modules:
            if unix:
                target = artifacts / "x86_64-unix" / unix
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(build / "dlls" / directory / unix, target)
            for arch in ("x86_64", "i386"):
                target = artifacts / (arch + "-windows") / pe
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(build / "dlls" / directory / (arch + "-windows") / pe, target)
        # Both graphics views need Vulkan, but DXMT must retain its own Metal
        # client-surface bridge and exports. Never reuse the base driver there.
        for name in REOPEN.TEXT.DXMT_PATCHES:
            REOPEN.apply(work / "src/wine-11.0", name)
        REOPEN.build_driver(build, work, "dxmt")
    print("Independent Vulkan-capable Wine components prepared")


if __name__ == "__main__":
    main()
