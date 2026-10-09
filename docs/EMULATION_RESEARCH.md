# Retro gaming and console ports

Research snapshot: **9 October 2026**. These are future integration candidates; Caskrun does not currently provide console emulation or game conversion. No extension, emulator, game or BIOS was installed or executed for this research.

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

An optional external adapter could eventually launch a local game folder through the Mac executable. Emulator data and version-specific settings would remain separate from Wine bottles.

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

## Recommended Caskrun implementation

For Apple Silicon, start with an optional KytyPS5 Mac adapter. This is a proposed
feature, not an implemented or tested Caskrun engine. The current upstream
[Mac build and launch instructions](https://github.com/KytyPS5/KytyPS5#building-on-macos)
document an x86-64 executable running under Rosetta 2, with MoltenVK alongside
it. This avoids introducing AnyPS5's additional conversion and Wine path in the
first version.

### 1. Import and launch

- Add **Import PS5 game** to the existing + menu. Keep game cards on Home;
  no additional Library screen is needed.
- Accept an explicitly selected game folder, ELF or `.zar` dump using the
  [upstream input formats](https://github.com/KytyPS5/KytyPS5#running). Preserve
  the original files and use local game artwork when available.
- Attach a user-installed engine initially. Validate its executable,
  architecture, engine version, Rosetta prerequisite and graphics dependencies.
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
