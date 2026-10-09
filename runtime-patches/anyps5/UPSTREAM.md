# AnyPS5 contribution notes

Prepared for review, not submitted upstream. Target revision:
`8b393b6e9a8ade9188d4f74a0293e0efd9828597`. These GPL-2.0-only patches
can be reviewed independently in their documented order.

## 1. Negotiate the real null-descriptor capability

Unmodified device initialization refuses a driver without `nullDescriptor`,
including presentation and shaders that contain no null image resources.
The patch keeps the reported feature false and permits those supported paths.
It rejects direct and padded null image slots before guest address preparation,
GPU allocation, descriptor updates or draw, including deferred and cached paths.
Defensive checks remain at null-view creation sites.

Tests cover missing/disabled/enabled feature negotiation, no-image resources,
fully populated heaps, malformed and padded null slots, deferred construction
and cache proofs. This patch does not emulate null-descriptor read/write,
atomic or query semantics, and games requiring them remain unsupported.

## 2. Negotiate Vulkan portability

The unchanged MoltenVK 1.4.2 driver needs the portability instance extension
with its matching enumeration flag. The original flags reproducibly fail
instance creation with `VK_ERROR_INCOMPATIBLE_DRIVER` in native and Wine probes.
The patch enables the advertised extension and flag together, avoids duplicate
names, covers headless initialization, and enables the advertised device subset
extension. No hardware feature bit is forced.

The original Windows diagnostic creates the required-feature device and a
real Win32 swapchain. All 4,096 transfer/readback pixels matched; bounded
presentation completed. This diagnostic is narrower than game compatibility.

## 3. Reserve only memory actually owned

Under Wine on macOS, `VirtualQuery` can describe an interval as free while an
exact reservation fails because a native host mapping occupies part of it.
Retrying the same interval does not progress. Ordinary and placeholder APIs
were compared: placeholder support works, so silently abandoning placeholders
would address the wrong problem.

The patch splits only address conflicts, with a finite attempt budget and
allocation-granularity leaves. Exact successful reservations are owned;
conflicts and all unverified remainder are excluded. Allocation, fixed mapping,
release, commit and shared mapping enforce coverage. Fixed requests spanning
an excluded interval fail before existing guest allocations can be removed.
Other reservation failures remain errors; adjacent exclusions are merged.

Native and Windows reservation tests, real private/shared replacement and alias
tests, and focused guest fixed-mapping refusal tests passed. The full upstream
GuestMemory suite still fails its existing working-set metadata assertion on
Wine's constant getter; that full-suite failure is explicitly retained.

## End-to-end evidence and scope

The supported Windows GCC 15.2 UCRT compiler built all 116 matched libraries.
The actual ShaderDeviceProfile and Graphics regression executables passed.
The public PS5 DOOM homebrew v1.1.1, with its shareware episode, reached
gameplay through its converted PS5 executable and guest library under the
independent Wine 11.0 / MoltenVK path. Keyboard movement, mouse firing,
relaunch, window close, a valid save file and loading it in a fresh process
were verified. Four game sessions completed without timeout.

The game uses its CPU renderer; AnyPS5's framebuffer presentation still uses
GPU compute/transfer operations. Audible output, physical controllers,
sustained play and retail PS5 games were not validated. Further guest error
and module-lookup functions remain throwing stubs. Export presence alone
does not establish function behavior.

See [the experiment report](../../docs/ANYPS5_WINE.md) for pinned public
inputs, reproduction settings and test limits. The contribution material
contains source patches, original diagnostics and sanitized technical results.
No account details, local home paths, device identifiers, raw logs,
screenshots, game files or compiled runtimes are included.
