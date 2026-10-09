# AnyPS5 Vulkan initialization diagnostics

Original MIT-licensed Caskrun probes, using Vulkan declarations from separately
licensed Khronos headers. No AnyPS5, game, Wine or driver code/assets are included.

The Windows counterpart dynamically loads `vulkan-1.dll`; the native counterpart
uses an explicitly supplied loader with `dlopen`. Both query the same bounded
capabilities and create/destroy one minimal graphics queue/device per enumerated
GPU. They enable no optional device features. Standard portability enumeration
and the required portability-subset extension are enabled only when advertised.

**The two capability probes are initialization evidence only.** Neither creates a surface,
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
probe_headers=.build/anyps5-host-oct09/graphics-deps/headers/include
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

The reviewed package is `.build/anyps5-graphics-oct09/runtime-candidate`, with its local loader,
official MoltenVK ICD and matching driver. Do not substitute an arbitrary
system-wide loader/driver or edit the normal app/runtime to make a test pass.
For a comparison against that exact local package:

```sh
probe_runtime=.build/anyps5-graphics-oct09/runtime-candidate
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

## Required-feature device and real presentation diagnostics

`vulkan-required-features-native.cpp` repeats the native capability comparison
but creates a device with the pinned renderer's required core, eight-bit storage
and buffer-device-address features enabled. It retains the actual false null
feature. A final argument `--no-portability` deliberately omits portability
negotiation for the negative comparison.

`vulkan-presentation-windows.cpp` is an original Win32 surface, transfer and
swapchain diagnostic. It enables the same required feature subset, checks the
BGRA surface and transfer formats, performs GPU upload/readback of 4,096 pixels,
and presents bounded frames to its window. It does not run SDL, AnyPS5, a guest
shader, audio or a game; it makes no performance claim.

Build with the pinned Khronos headers already installed in the ignored graphics
dependency folder:

```sh
x86_64-w64-mingw32-g++ -std=c++17 -O2 -Wall -Wextra -Wpedantic -Werror \
  -static -static-libgcc -static-libstdc++ \
  -I .build/anyps5-host-oct09/graphics-deps/headers/include \
  scripts/diagnostics/anyps5-vulkan/vulkan-presentation-windows.cpp -luser32 \
  -o .build/anyps5-host-oct09/presentation.exe

clang++ -std=c++17 -O2 -arch x86_64 -mmacosx-version-min=14.0 \
  -Wall -Wextra -Wpedantic -Werror \
  -I .build/anyps5-host-oct09/graphics-deps/headers/include \
  scripts/diagnostics/anyps5-vulkan/vulkan-required-features-native.cpp \
  -o .build/anyps5-host-oct09/required-native
```

Windows arguments are test seconds (1–120, default 5), optionally followed by
`--no-portability`. Use an owned disposable prefix, remove host mappings and the
root drive before running, and enforce an external timeout. The native program
requires an explicit loader path; set only that child's official ICD path and
suppress driver informational logs. Never use an existing user bottle.

The tested Windows v2 diagnostic returned zero, enumerated 15 instance
extensions, one physical device, 126 device extensions, 60 surface formats and
two swapchain images; 4,096/4,096 pixels matched and 183 frames were presented.
The earlier equivalent v1 run presented 336 frames; its count labels reported
Vulkan result codes, which v2 now distinguishes from actual counts. Both the
Wine and native no-portability comparisons returned
`VK_ERROR_INCOMPATIBLE_DRIVER` (-9) at instance creation. These observations are
specific to the rebuilt local candidate and official MoltenVK 1.4.2. They are
presentation evidence, not PS5 gameplay evidence.
