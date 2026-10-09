# Experimental AnyPS5 patches for Wine on macOS

These patches apply only to upstream AnyPS5 revision
`8b393b6e9a8ade9188d4f74a0293e0efd9828597`. They modify the independently
licensed AnyPS5 project, not the installed Caskrun Windows engine. AnyPS5 and
these modifications are GPL-2.0-only; see COPYING and upstream copyright
notices. No AnyPS5 binary, game files, console firmware or keys are included.

## Patch order

Apply to a fresh checkout of that revision:

```sh
git apply /path/to/Caskrun/runtime-patches/anyps5/anyps5-8b393b6-optional-null-descriptors.patch
git apply /path/to/Caskrun/runtime-patches/anyps5/anyps5-8b393b6-vulkan-portability.patch
git apply /path/to/Caskrun/runtime-patches/anyps5/anyps5-8b393b6-segmented-guest-arena.patch
```

The null-descriptor patch negotiates the actual feature. With it unavailable,
VideoOut and fully populated image heaps may run. Direct and padded null image
slots are refused before address preparation, GPU allocations, descriptor
updates or draw. Construction, deferred completion and cache revalidation
check the same policy; defensive checks remain at the actual null-view sites.
The shader profile continues to report the feature as disabled.

**This is not null-descriptor emulation.** A game using null image elements
remains unsupported. Dummy images alone would give incorrect read, write,
atomic and query behavior; a complete fallback also requires shader handling.

The portability patch enables the instance extension and matching enumeration
flag only when advertised, avoids duplicate extension names, and enables the
mandatory device subset extension only when that device advertises it. It
covers windowed and headless initialization without forcing feature flags.

The guest-arena patch handles an observed Wine/macOS discrepancy: a Windows
query can report a free interval containing occupied native host mappings.
It subdivides address conflicts within a finite reservation budget. Only
exact successful placeholder reservations are owned; conflicting and
unverified intervals are excluded. Allocation, fixed placement, release,
commit and shared mapping cannot use those holes. Fixed mapping requests that
span an excluded range are rejected before any existing mapping is removed.
Other reservation errors remain fatal. Adjacent and overlapping exclusion ranges are merged.

## Build and validation

Use upstream's documented Windows GCC 15.2.0 UCRT compiler and exact submodules.
The Mac's GCC 16.1 cross-compiler produced SjLj exception calls under upstream
flags, while the supported compiler produced the required DWARF metadata.
The converter and runtime libraries must come from the same source revision.

The patch adds upstream ShaderDeviceProfile and Graphics regressions for
feature negotiation, no-image resources, direct/padded null rejection,
address preparation, deferred construction and cache proofs. The standalone
profile/policy check passed 16 cases and the arena check passed 32 cases.
The actual Windows profile and Graphics suites, 18 real segmented mapping
operations and focused fixed-hole refusals also passed. The full GuestMemory
suite still fails Wine's existing working-set metadata assertion; no full-suite
pass is claimed.

The converted PS5 DOOM homebrew reached gameplay under the independent Wine
Vulkan candidate. Four sessions validated scripted and live keyboard/mouse
input, native window close, relaunch, and actual save/load. See
[the experiment report](../../docs/ANYPS5_WINE.md) for evidence and limits.
Audible output, physical controllers and retail PS5 games remain unvalidated.

Original Vulkan presentation diagnostics are under
scripts/diagnostics/anyps5-vulkan/. Their successful window, swapchain and GPU
readback test establishes that specific Wine path; it does not establish
shader translation, SDL integration or PS5 gameplay.
