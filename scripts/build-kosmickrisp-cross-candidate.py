#!/usr/bin/env python3
"""Build a local x86_64 KosmicKrisp candidate on an Apple Silicon host.

Requires already reviewed Mesa, LLVM 20, SPIR-V translator sources and SDK
components. Does not download, install globally, change the working runtime
or launch Wine. Native compiler tools stay separate from the target driver.
Only the target driver and a relative ICD are staged in output/target.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def run(command, env, log):
    with log.open("ab") as stream:
        subprocess.run(list(map(str, command)), env=env, stdout=stream,
                       stderr=subprocess.STDOUT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("mesa", "llvm", "translator-source", "sdk", "meson-bin", "output", "zstd-library"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--jobs", type=int, choices=range(1, 5), default=3)
    args = parser.parse_args()
    mesa, llvm, translator, sdk, tools, out, zstd = (
        getattr(args, name).resolve() for name in (
            "mesa", "llvm", "translator_source", "sdk", "meson_bin", "output", "zstd_library"))
    build_root = Path(__file__).resolve().parent.parent / ".build"
    if build_root not in out.parents or build_root not in llvm.parents:
        parser.error("Output and the LLVM copy to relocate must both be inside this project's ignored .build directory")
    if not (mesa / "src/kosmickrisp").is_dir() or not (llvm / "bin/llvm-config").is_file():
        parser.error("Reviewed Mesa source and LLVM prefix are required")
    if not zstd.is_file() or not (tools / "meson").is_file():
        parser.error("Local compiler tools and an existing native zstd library are required")
    out.mkdir(parents=True, exist_ok=True)
    native = out / "native-deps"
    env = os.environ.copy()
    env.update({"CC": "/usr/bin/clang", "CXX": "/usr/bin/clang++",
                "OBJC": "/usr/bin/clang", "OBJCXX": "/usr/bin/clang++",
                "PATH": str(tools) + ":" + env["PATH"], "MACOSX_DEPLOYMENT_TARGET": "26.0",
                "PYTHONDONTWRITEBYTECODE": "1"})
    log = out / "build.log"
    run(["cmake", "-S", translator, "-B", out / "translator-build", "-G", "Ninja",
         "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_C_COMPILER=/usr/bin/clang",
         "-DCMAKE_CXX_COMPILER=/usr/bin/clang++", "-DCMAKE_INSTALL_PREFIX=" + str(native),
         "-DLLVM_DIR=" + str(llvm / "lib/cmake/llvm"),
         "-DLLVM_EXTERNAL_SPIRV_HEADERS_SOURCE_DIR=" + str(sdk),
         "-DLLVM_SPIRV_INCLUDE_TESTS=OFF", "-DBUILD_SHARED_LIBS=ON"], env, log)
    run(["cmake", "--build", out / "translator-build", "-j", args.jobs], env, log)
    run(["cmake", "--install", out / "translator-build"], env, log)
    env.update({"LLVM_CONFIG": str(llvm / "bin/llvm-config"),
                "PKG_CONFIG_PATH": str(native / "lib/pkgconfig") + ":" + str(sdk / "lib/pkgconfig"),
                "DYLD_LIBRARY_PATH": str(native / "lib") + ":" + str(llvm / "lib") + ":" + str(zstd.parent)})
    common = ["--buildtype=release", "--prefix=/usr/local", "-Dplatforms=macos",
              "-Dvulkan-drivers=kosmickrisp", "-Dgallium-drivers=", "-Dopengl=false",
              "-Dzstd=disabled", "--prefer-static"]
    native_build = out / "native-build"
    run([tools / "meson", "setup", native_build, mesa] + common + [
        "-Dinstall-mesa-clc=true", "-Dinstall-precomp-compiler=true", "-Dshared-llvm=enabled"], env, log)
    run([tools / "meson", "compile", "-C", native_build, "-j", args.jobs,
         "mesa_clc", "vtn_bindgen2", "kk_clc"], env, log)
    executables = [native_build / "src/compiler/clc/mesa_clc",
                   native_build / "src/compiler/spirv/vtn_bindgen2",
                   native_build / "src/kosmickrisp/clc/kk_clc"]
    # A macOS protected interpreter may strip DYLD_* for its child. Resolve
    # native build-tool dependencies explicitly inside this private prefix.
    for path in [llvm / "lib/libLLVM.dylib", llvm / "lib/libclang-cpp.dylib",
                 native / "lib/libLLVMSPIRVLib.dylib"] + executables:
        links = subprocess.check_output(["otool", "-L", str(path)], text=True)
        for line in links.splitlines()[1:]:
            dependency = line.strip().split(" (")[0]
            target = None
            if dependency.startswith("@@HOMEBREW_PREFIX@@/opt/llvm@20/lib/"):
                target = llvm / "lib" / Path(dependency).name
            elif dependency.startswith("@@HOMEBREW_PREFIX@@/opt/zstd/"):
                target = zstd
            if target:
                run(["install_name_tool", "-change", dependency, target, path], env, log)
        if path in (executables[0], executables[2]):
            run(["install_name_tool", "-add_rpath", native / "lib", path], env, log)
        run(["codesign", "--force", "--sign", "-", path], env, log)
    cross = out / "x86_64.ini"
    cross.write_text("""[binaries]
c = '/usr/bin/clang'
cpp = '/usr/bin/clang++'
objc = '/usr/bin/clang'
objcpp = '/usr/bin/clang++'
ar = '/usr/bin/ar'
strip = '/usr/bin/strip'
[built-in options]
c_args = ['-arch', 'x86_64']
cpp_args = ['-arch', 'x86_64']
objc_args = ['-arch', 'x86_64']
objcpp_args = ['-arch', 'x86_64']
c_link_args = ['-arch', 'x86_64']
cpp_link_args = ['-arch', 'x86_64']
objc_link_args = ['-arch', 'x86_64']
objcpp_link_args = ['-arch', 'x86_64']
[host_machine]
system = 'darwin'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
""")
    env.update({"PATH": ":".join(str(path.parent) for path in executables) + ":" + env["PATH"],
                "PKG_CONFIG_PATH": "", "PKG_CONFIG_LIBDIR": ""})
    target_build = out / "target-build"
    run([tools / "meson", "setup", target_build, mesa, "--cross-file=" + str(cross)] + common + [
        "-Dmesa-clc=system", "-Dprecomp-compiler=system", "-Dspirv-tools=disabled", "-Dllvm=disabled"], env, log)
    run([tools / "meson", "compile", "-C", target_build, "-j", args.jobs,
         "vulkan_kosmickrisp", "kosmickrisp_mesa_icd"], env, log)
    target = out / "target"
    library = target / "lib/libvulkan_kosmickrisp.dylib"
    library.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(target_build / "src/kosmickrisp/vulkan/libvulkan_kosmickrisp.dylib", library)
    run(["codesign", "--force", "--sign", "-", library], env, log)
    icd = target / "share/vulkan/icd.d/libkosmickrisp_icd.json"
    icd.parent.mkdir(parents=True, exist_ok=True)
    icd.write_text(json.dumps({"file_format_version": "1.0.0", "ICD": {
        "api_version": "1.4.0", "library_path": "../../../lib/libvulkan_kosmickrisp.dylib"}}, indent=2) + "\n")
    print(json.dumps({"driver_sha256": hashlib.sha256(library.read_bytes()).hexdigest(),
                      "architecture": "x86_64", "validation": "built, not game-tested"}))


if __name__ == "__main__":
    main()
