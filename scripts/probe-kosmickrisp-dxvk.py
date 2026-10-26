#!/usr/bin/env python3
"""Run an original bounded offscreen Vulkan test with one explicit local ICD.

No Wine, game, user bottle, system driver installation or implicit ICD lookup.
The result does not establish D3D or presentation support. Driver libraries
must already have been acquired and reviewed independently.
"""

import argparse
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", required=True, type=Path)
    parser.add_argument("--icd", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--arch", choices=("arm64", "x86_64"), required=True)
    args = parser.parse_args()
    sdk, icd, out = args.sdk.resolve(), args.icd.resolve(), args.output.resolve()
    repo = Path(__file__).resolve().parent.parent
    if not icd.is_file() or not (sdk / "include/vulkan/vulkan.h").is_file():
        parser.error("A real local SDK and explicit ICD are required")
    out.mkdir(parents=True, exist_ok=True)
    subprocess.run([
        str(sdk / "bin/glslc"), str(repo / "scripts/probes/kosmickrisp-dxvk-readback.comp"),
        "-o", str(out / "readback.spv"),
    ], check=True, timeout=60)
    subprocess.run([
        "/usr/bin/clang++", "-arch", args.arch, "-std=c++17", "-O2",
        "-I" + str(sdk / "include"), str(repo / "scripts/probes/kosmickrisp-dxvk-capabilities.cpp"),
        "-L" + str(sdk / "lib"), "-lvulkan", "-Wl,-rpath," + str(sdk / "lib"),
        "-o", str(out / "probe"),
    ], check=True, timeout=60)
    env = os.environ.copy()
    for key in ("VK_ICD_FILENAMES", "VK_ADD_DRIVER_FILES", "VK_INSTANCE_LAYERS",
                "VK_LAYER_PATH", "VK_ADD_LAYER_PATH", "MESA_VK_DEVICE_SELECT"):
        env.pop(key, None)
    env.update({"VK_DRIVER_FILES": str(icd), "VK_LOADER_LAYERS_DISABLE": "~all~",
                "MESA_SHADER_CACHE_DISABLE": "true"})
    result = subprocess.run([str(out / "probe"), str(out / "readback.spv")],
                            env=env, text=True, capture_output=True, timeout=25)
    (out / "result.txt").write_text(result.stdout + "\n" + result.stderr)
    print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
