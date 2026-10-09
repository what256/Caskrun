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
