# AnyPS5 Vulkan initialization diagnostics

Original MIT-licensed Caskrun probes, using Vulkan declarations from separately
licensed Khronos headers. No AnyPS5, game, Wine or driver code/assets are included.

The Windows counterpart dynamically loads `vulkan-1.dll`; the native counterpart
uses an explicitly supplied loader with `dlopen`. Both query the same bounded
capabilities and create/destroy one minimal graphics queue/device per enumerated
GPU. They enable no optional device features. Standard portability enumeration
and the required portability-subset extension are enabled only when advertised.

**This is initialization evidence only.** Neither probe creates a surface,
swapchain or window, runs AnyPS5, converts a game, draws a frame or validates
gameplay. Enumeration has finite limits; the caller must also enforce a process
timeout and, for Wine, own and clean up the disposable prefix.

## Pinned, targeted requirement checks

- Release: [AnyPS5 v0.1.1](https://github.com/boykopovar/AnyPS5/tree/v0.1.1).
- Development snapshot:
  [8b393b6e9a8ade9188d4f74a0293e0efd9828597](https://github.com/boykopovar/AnyPS5/tree/8b393b6e9a8ade9188d4f74a0293e0efd9828597).

Reported device checks cover Vulkan 1.1, 8-bit buffer storage, buffer device
addresses, shader float controls and signed-zero/Inf/NaN preservation, shader
64-bit integers, vertex/fragment writes and atomics, anisotropy and BC textures.
The pinned development snapshot additionally checks robustness2/null descriptors.
These fields describe that snapshot, not an unpinned future `main` revision.

Windows requirement conjunctions also include advertised `VK_KHR_surface`,
`VK_KHR_win32_surface` and device `VK_KHR_swapchain`. Advertising these extensions
does not establish supported surface formats, present modes or actual presentation.
The native Mac probe normally lacks the Windows surface extension. It reports
device-feature comparisons and native surface/swapchain extension availability
separately, without a Windows requirement conjunction. Compare its individual
required feature values with Wine's; loader versions and extension lists can
differ. This isolates driver feature reporting from Wine translation and does
not replace the real Windows integration test.

## Build from the repository root

Use the retained official Khronos Vulkan Headers **v1.4.350**, an existing MinGW
x64 compiler and Apple's command-line compiler. Keep outputs in ignored `.build`:

```sh
probe_headers=.build/graphics-probe/stage/headers/include
probe_build=.build/anyps5-vulkan-diagnostics
mkdir -p "$probe_build"
x86_64-w64-mingw32-g++ -std=c++17 -O2 -Wall -Wextra -Wpedantic -Werror \
  -static -static-libgcc -static-libstdc++ -I "$probe_headers" \
  scripts/diagnostics/anyps5-vulkan/vulkan-capabilities-windows.cpp \
  -o "$probe_build/vulkan-capabilities-windows-x64.exe"
clang++ -std=c++17 -O2 -arch x86_64 -mmacosx-version-min=14.0 \
  -Wall -Wextra -Wpedantic -Werror -I "$probe_headers" \
  scripts/diagnostics/anyps5-vulkan/vulkan-capabilities-native.cpp \
  -o "$probe_build/vulkan-capabilities-native-x64"
```

The Windows executable uses only Windows system/UCRT imports and loads Vulkan
dynamically. Static MinGW runtime linkage avoids separate GCC/C++ runtime DLLs.
The native x64 executable matches the retained independent Wine candidate's
loader architecture and runs through Rosetta on Apple Silicon.

## Native comparison with the retained candidate

The reviewed package is `.build/vulkan-candidate-runtime`, with its local loader,
official MoltenVK ICD and matching driver. Do not substitute an arbitrary
system-wide loader/driver or edit the normal app/runtime to make a test pass.
For a comparison against that exact local package:

```sh
probe_runtime=.build/vulkan-candidate-runtime
env -i PATH=/usr/bin:/bin \
  VK_DRIVER_FILES="$PWD/$probe_runtime/lib/vulkan/MoltenVK_icd.json" \
  MVK_CONFIG_LOG_LEVEL=0 \
  /usr/bin/arch -x86_64 "$probe_build/vulkan-capabilities-native-x64" \
  "$PWD/$probe_runtime/lib/vulkan/libvulkan.1.dylib"
```

Suppress MoltenVK informational logging with `MVK_CONFIG_LOG_LEVEL=0` in **every**
native/Wine runner: driver logs can expose GPU names/UUIDs. Original probe output
omits names, UUIDs, account paths and serials. Keep all raw logs private/ignored.
Run the Windows counterpart only through a reviewed independent runtime in an
owned disposable prefix, with an enforced timeout; never use an existing bottle.

See [the experiment report](../../../docs/ANYPS5_WINE.md) for measured results,
remaining barriers and cleanup. A capability result never overrides a false
driver feature bit or establishes that a real game works.
