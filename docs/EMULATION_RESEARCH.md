# Retro gaming and console ports

Research snapshot: **9 October 2026**. The demo now has an optional external KytyPS5 adapter and local imports; actual console emulation remains the selected engine’s responsibility, and game conversion is not implemented. No extension, emulator, game or BIOS was installed or executed for this work.

## RetroBrowser+

[RetroBrowser+](https://chromewebstore.google.com/detail/retrobrowser%2B/bfnjglgpfagpjodchbnolljodolgalpf) is a Chrome extension for organising and playing user-selected retro game files. The listing describes artwork retrieval, save states, controllers and display filters. Version 1.3.15 was updated on 7 October 2026.

The listing says commercial games are not included and some systems require user-provided BIOS files. It does not establish that every advertised system works on Apple Silicon. No public source repository or reuse licence was found in the listing or its linked developer support page; free availability must not be described as open-source permission.

A possible first step is an optional link to the official store listing. This is an extension destination, not a directly playable browser game. Embedding its code or automatically installing it is outside the current work.

## AnyPS5: conversion is separate from execution

[AnyPS5](https://github.com/boykopovar/AnyPS5) is licensed GPL-2.0-only. It relinks PS5 executables into Windows or Linux formats and implements replacement system libraries.

Its [build instructions](https://github.com/boykopovar/AnyPS5/blob/main/docs/dev/BUILD.md) explicitly support building the **relinker tool** on macOS, including Apple Silicon. Outputs remain x86-64 Windows PE or Linux ELF; this mode does **not** provide macOS game execution. Running Windows output through Caskrun would add another untested Wine, CPU translation and graphics compatibility path.

The [progress page](https://boykopovar.github.io/AnyPS5/) reported the following at review:

| Counter | Snapshot | What is counted |
| --- | --- | --- |
| GPU shader instructions | 1,166 / 1,166, 100% | Instructions recognised by the decoder, not verified game rendering |
| System library functions | 2,575 / 3,038, 84.76% | Declared functions without the named throwing placeholder; not every PS5 firmware function |

The denominator can change as declarations are added. These figures supersede the older social-post numbers and must never be presented as game compatibility percentages. The project's [tested compatibility list](https://github.com/boykopovar/AnyPS5/blob/main/docs/user/COMPATIBILITY.md) records Dreaming Sarah as playable on Windows and marks Linux unknown.

## KytyPS5: experimental Mac execution

[KytyPS5](https://github.com/KytyPS5/KytyPS5) is a separate GPL-2.0-only PS5 emulator. Its README describes experimental Apple Silicon execution using an x86-64 build under Rosetta 2 and MoltenVK. A small number of titles have reached gameplay; Windows receives most testing. [Mac release archives](https://github.com/KytyPS5/KytyPS5/releases) exist, but their availability does not validate Caskrun integration or any particular game.

The optional external adapter can invoke a user-selected Mac executable with the documented game argument. Engine data remains outside Wine bottles. This is source implementation and original process-fixture validation, not evidence that a PS5 game runs.

## Static recompilation alternatives

The unspecified project that converts console games into Windows versions has not been identified. AnyPS5 may be the intended reference; other possibilities include:

| Project | Scope and limits |
| --- | --- |
| [N64Recomp](https://github.com/N64Recomp/N64Recomp) | MIT. Converts N64 code into C. [CI includes Mac ARM64](https://github.com/N64Recomp/N64Recomp/blob/main/.github/workflows/validate.yml), but playable ports still need a runtime and game-specific work. |
| [XenonRecomp](https://github.com/hedge-dev/XenonRecomp) | MIT. Converts Xbox 360 code into C++; ARM64 instruction support exists. It supplies no game runtime. |
| [PS2Recomp](https://github.com/ran-j/PS2Recomp) | GPLv3 licence file. Experimental PS2-to-C++ toolkit and runtime; its README reports partial hardware support and poor vector/graphics performance. |

These tools are not universal one-click game converters. Prefer importing finished, individually verified native macOS ports before adding conversion workflows.

## Proposed future scope

Start with native Mac ports or optional external engine adapters, a pinned engine version and one validated title. Verify rendering, saves, controllers and shutdown before claiming support. Use user-owned game files and any required BIOS; bundle no games, BIOS, firmware or proprietary assets. Review each engine and dependency licence before redistribution.

## Current adapter and remaining work

The demo implements an optional KytyPS5 Mac adapter. Import and process ownership are checked with original fixtures; no real engine or game was launched. The current upstream
[Mac build and launch instructions](https://github.com/KytyPS5/KytyPS5#building-on-macos)
document an x86-64 executable running under Rosetta 2, with MoltenVK alongside
it. This avoids introducing AnyPS5's additional conversion and Wine path in the
first version.

### 1. Import and launch

- Add **Import PS5 game** to the existing + menu. Keep game cards in Collection;
  no additional Library screen is needed.
- Accept an explicitly selected game folder, ELF or `.zar` dump using the
  [upstream input formats](https://github.com/KytyPS5/KytyPS5#running). Preserve
  the original files and use local game artwork when available.
- Attach a user-installed engine initially. Validate its executable,
  architecture, Rosetta prerequisite and local MoltenVK dependency. The engine version is recorded as unknown; Caskrun does not execute a selected binary to probe it.
  Keep its records and settings separate from Wine bottles.
- Invoke the documented `kyty_emulator --game` command with an argument array.
  The inspector should show engine-specific controls and an Experimental label.
- Track only Caskrun-started sessions, including identity, window focus, logs,
  stopping and full Quit. Unrelated emulator sessions must remain untouched.

This integration can be developed and checked with original test fixtures, but
an import or launched process does not establish game compatibility.

### 2. Validate one title, then manage the engine

Test one user-owned title on Apple Silicon: cold start, rendering, audio,
controllers, save/load, relaunch, performance, window/Dock identity and shutdown.
Record the exact engine version. Keep compatibility status unknown until those
checks succeed.

After that, consider a managed download pinned to an
[official release](https://github.com/KytyPS5/KytyPS5/releases), with a reviewed
package hash, atomic updates and rollback. Review bundled component licences
and preserve corresponding engine source and notices for any binary Caskrun
redistributes. No games or proprietary firmware are included.

### 3. Investigate AnyPS5 separately

An optional later workflow would convert a user-selected PS5 executable and its
modules into a complete Windows output, then attach that output to a dedicated
Caskrun bottle. The [AnyPS5 build](https://github.com/boykopovar/AnyPS5/blob/main/docs/dev/BUILD.md)
and [runtime layout](https://github.com/boykopovar/AnyPS5/blob/main/docs/user/USAGE.md)
must be satisfied first, including its Windows library toolchain. Wine,
Rosetta and graphics compatibility then require a separate real-title test.
Conversion completion and technology-coverage counters are not acceptance
criteria for playable games.

## Local metadata and lifecycle boundaries

PS5 imports are references to original files. The private versioned JSON is
bounded to 512 KiB and 256 games and uses atomic replacement; symbolic-link
paths are refused. It is separate from bottle backups, diagnostics and Git.
For explicitly selected game folders, local `sce_sys/icon0.png` or `icon0.png`
is decoded asynchronously within byte and dimension limits, with at most two
readers. Optional picture reads never hold up import. ELF/ZAR file imports do
not scan neighbouring files; selecting one file grants no sibling-folder scope. Removing an entry removes only its reference.

The selected engine is structurally checked as a supported Mach-O executable
named `kyty_emulator`; this does not authenticate a publisher or prove it works.
The user selects a complete trusted local engine folder or app, including its
MoltenVK dependency. Caskrun stores the validated executable inside that package. Caskrun invokes it directly
with `--game` and the imported path, preserving filenames literally.

Each owned process has a private session log capped at 4 MiB. Stop and full Quit
signal only retained Process objects started by this Caskrun instance; a bounded
shutdown failure leaves Caskrun open. Independent engine sessions are untouched.
The adapter does not manage engines’ own child processes, controllers, saves or
graphics settings. Those limits, native Dock/window identity and real-title
rendering/audio/performance require user testing before any compatibility claim.

AnyPS5 conversion and managed engine downloads remain planned. No engine,
MoltenVK binary, game, firmware or BIOS is redistributed by this adapter.

## Engine comparison and management proposal

Rechecked upstream documentation on **9 October 2026**. This is research;
no additional engine was installed or run, and Caskrun currently supports
only its external KytyPS5 adapter.

| Project | Upstream functionality and Mac route | Proposed place in Caskrun |
| --- | --- | --- |
| [KytyPS5](https://github.com/KytyPS5/KytyPS5) | Experimental x86-64 Mac build via Rosetta and MoltenVK; game folders, ELF and streamed read-only ZAR archives; graphics/debug controls. | Keep the existing adapter as the first option. |
| [SharpEmu](https://github.com/sharpemu/sharpemu) | Independent C# PS5 engine, Mac x64 via Rosetta with bundled MoltenVK; eboot input and per-game/global environment controls. | Best candidate for a second adapter, with its own input and lifecycle validation. |
| [KytyPlus](https://github.com/Coder787-source/KytyPlus) | KytyPS5 derivative; Mac x64 via Rosetta, pipeline caching, FSR 1.0 and configurable presentation. Upstream explicitly says game-level optimization benefits remain unvalidated. | Optional later alternative; avoid implying a performance improvement on this Mac. |
| [AnyPS5](https://github.com/boykopovar/AnyPS5/blob/main/docs/dev/BUILD.md) | Relinker builds on Mac ARM64 but outputs x86-64 Windows/Linux programs; it does not provide Mac game execution. | Separate conversion workflow, not a drop-in Mac engine. |
| [prosper](https://github.com/mattias800/prosper) | Experimental PS5 compatibility layer, with documented results primarily on Linux. Its README states no licence is granted yet. | Research only; do not redistribute it. |

Recommendation: support multiple engine records and a per-game selection, while
showing only engines whose adapter is implemented. Preserve each engine's
version, arguments, save/cache roots and owned sessions independently. Validate
a user-owned title's rendering, audio, input, saves and shutdown before marking
that engine/game pair tested on Apple Silicon. Upstream screenshots or technology
coverage percentages do not establish this.

A small gear at the right of the existing floating header could open an
**Engines** panel. Keep it focused: installed version/status, Add local engine,
reviewed official download, update and rollback. Downloads should use a pinned
release and hash, bounded staging, architecture/dependency checks, corresponding
licence/source notices and atomic replacement. Never update an engine with an
owned running session. Unsupported or unreviewed packages should not have a
working Download action. This panel, downloads and extra adapters remain
proposals pending implementation; they are not part of the browser UI update.

## AnyPS5 on Mac: feasibility and first experiment

Upstream's [tested compatibility list](https://github.com/boykopovar/AnyPS5/blob/main/docs/user/COMPATIBILITY.md)
currently records Dreaming Sarah as playable on Windows; Linux is marked unknown.
This is an upstream report, not a Caskrun result. Neither the relinker nor a
converted game was run for this review.

Sharing Unix conventions does not provide binary compatibility. The
[build instructions](https://github.com/boykopovar/AnyPS5/blob/main/docs/dev/BUILD.md)
allow the converter to run on Apple Silicon, but its outputs stay x86-64 Linux
ELF or Windows PE, with target-specific system libraries. The
[relinker implementation](https://github.com/boykopovar/AnyPS5/blob/main/core/relinker/main.cpp)
selects only these two output formats; the
[library export patcher](https://github.com/boykopovar/AnyPS5/blob/main/core/libs/nid/src/BinaryPatcherFactory.cpp)
also handles only ELF and PE. A native Mac output requires Mach-O generation and
linking, ported system-library behaviour, and a supported CPU execution path.
Keeping x86-64 and investigating Rosetta would be smaller than adding an ARM
translation backend. This is an engineering assessment, not implemented support.

The smaller first experiment is Windows output plus Windows-built AnyPS5
libraries through an isolated Caskrun Wine/Rosetta bottle, using a user-owned
title already working upstream. The project's
[graphics pipeline](https://github.com/boykopovar/AnyPS5/blob/main/docs/dev/ARCHITECTURE.md)
targets Vulkan/SPIR-V. [MoltenVK](https://github.com/KhronosGroup/MoltenVK) provides
a Vulkan subset over Metal, so required features need checking before treating
that as a viable bridge. Current D3D Metal/DXMT settings do not by themselves
implement this game's Vulkan requirements.

Compare rendering, frame times, audio, input, saves, relaunch and shutdown before
claiming Windows-like behaviour. A Windows result on different hardware cannot
establish equal Mac performance. The Wine experiment, native Mac output and
performance comparison remain untested; no conversion control is added to the
app by this research.
