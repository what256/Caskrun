# Source licenses and third-party components

Caskrun is an independent project. Its original app, Rust core, catalog metadata, build tooling, and Caskrun artwork are covered by the [MIT license](LICENSE). The patches and imported community data below retain their separate licenses.

## Wine and DXMT patches

The changes in `runtime-patches/wine-*.patch` and `runtime-patches/dxmt-*.patch` are offered under **LGPL-2.1-or-later**, matching their respective upstream projects. The complete [LGPL 2.1 text](runtime-patches/licenses/COPYING.LIB) is included. See [the patch directory notice](runtime-patches/README.md) for the pinned source revisions and patch order.

- [Wine 11.0](https://gitlab.winehq.org/wine/wine/-/tree/wine-11.0): Copyright (c) 1993–2026 the Wine project authors; its source contains the individual author notices.
- [DXMT at the reviewed revision](https://github.com/3Shain/dxmt/tree/e94c312f5c054263acf261cfa109edf13e757587): Copyright (c) 2023–2026 Feifan He for CodeWeavers. This is separately licensed public source, not code taken from an installed CrossOver application.

Caskrun's MIT license does not replace these licenses or upstream copyright notices. Original MIT-licensed helpers and plist templates in the same directory are not Wine or DXMT source.

## MSync and Vulkan engine work

The original Wine 11 MSync port starts from the public generic
[marzent/wine-msync](https://github.com/marzent/wine-msync/tree/be7f3e2ff40670018cd7aa9042bad487fa7a83ec)
development patch, under **LGPL-2.1-or-later**. Its original author notices,
pinned input, exact upstream license and local port notes are retained in
[`runtime-patches/msync/`](runtime-patches/msync/README.md). The complete local
corresponding change is [`wine-11.0-msync.patch`](runtime-patches/wine-11.0-msync.patch).
No CX-specific variant or installed application source is used.

The candidate's hybrid DXVK modules retain their separate **Zlib** licenses:

- [Metalsharp DXVK-MacOS](https://github.com/metalsharp/DXVK-MacOS/tree/bd8921a3db6c4928f5b34672791121f7c14e38c2),
  tag `v3.1-macos1.0`, revision `bd8921a3db6c4928f5b34672791121f7c14e38c2`,
  supplies D3D10/11. The original Wine Vulkan portability patch records its
  public source attribution in
  [`runtime-patches/vulkan-portability/UPSTREAM.json`](runtime-patches/vulkan-portability/UPSTREAM.json)
  and retains the [upstream license](runtime-patches/vulkan-portability/DXVK-MACOS-LICENSE).
- [tairasu D9VK](https://github.com/tairasu/d9vk/tree/7437b616f05bc678d2ff4b4b75832f9cbddeed75),
  version `1.10.3-20230507-async-macos`, revision
  `7437b616f05bc678d2ff4b4b75832f9cbddeed75`, supplies source-built D3D9.
  D3D8 remains Wine's original implementation.

Official [MoltenVK 1.4.2](https://github.com/KhronosGroup/MoltenVK/tree/db66022459ffb663aa2b50f6b018bc2e124f5edf)
and the [Vulkan Loader](https://github.com/KhronosGroup/Vulkan-Loader/tree/fa218055ff4948b159f31d43f800c808a13d51df)
retain **Apache-2.0** and their supplied component notices. The official SDK
1.4.363.0 input, architectures, hashes and source provenance are pinned by the
development tooling. Candidate packages retain each translator's separate
source/build provenance and notices; binaries/build caches are excluded from
this Git repository. Wine's builtin DOS-stub marker is the only change made to
the selected translator PE files; original/runtime hashes are recorded and the
altered packaging is disclosed. Caskrun's MIT license does not relicense these
components or approve binary distribution.

The source-built D3D9 component also retains the supplied GCC 16.1 GPLv3 text
and GCC Runtime Library Exception, MinGW-w64 14.0 ZPL notices/authors, and
winpthreads notices. Their original texts and public source hashes accompany
the local runtime under `source/d9vk/`; the translator's own license is Zlib.

Apple's Game Porting Toolkit/D3DMetal evaluation component is **separately
licensed by Apple**. A privately supplied official image was inspected and
staged only after the actual local evaluation terms were accepted. Its payload,
license/download account data and raw diagnostics are not included in this
source repository or the shared engine. No working D3DMetal capability or
redistribution permission is implied by staging it.

## AnyPS5 patches

`runtime-patches/anyps5/` contains modifications to the separately licensed [AnyPS5 project](https://github.com/boykopovar/AnyPS5/tree/8b393b6e9a8ade9188d4f74a0293e0efd9828597), under **GPL-2.0-only**. Its exact upstream license is included as [COPYING](runtime-patches/anyps5/COPYING). The [patch notes](runtime-patches/anyps5/README.md) preserve the source revision, application order and validation limits. No AnyPS5 binaries, console firmware, keys or game files are included in this source repository.

## Community compatibility data

`catalog/game-compatibility.json` contains a metadata-only snapshot from [Highball's community database](https://github.com/gauthierpiarrette/highball-db/tree/d8d165834cd0b77613ac0f00300e8fac0b98b143), under **CC0-1.0**. Its original [license text](catalog/licenses/highball-db-CC0.txt) is included. The snapshot records its source revision and links each report to its public source; it does not import contributor identity fields or report prose. See [data provenance](docs/GAME_COMPATIBILITY.md). Caskrun's MIT license does not replace the snapshot's CC0 terms.

## MaPC / MacadeKit source adaptations

The automatic graphics inspection and EA/Ubisoft store handling were reviewed
against [MacadeKit](https://github.com/abduzeedo/mapc-engine/tree/37bb046423dbb85c5cb98fee742868dccb96002d),
Copyright 2026 Steale LLC, under **Apache-2.0**. Caskrun's Rust adaptations in
`core/src/graphics_probe.rs`, `publisher_install.rs`, `publisher_library.rs` and
`publisher_launch.rs` preserve this attribution. See the exact
[license](third-party/mapc-engine/LICENSE), [upstream notice](third-party/mapc-engine/NOTICE)
and [adaptation record](third-party/mapc-engine/README.md).

The adaptations add bounded no-follow reads, strict publisher identifiers,
conservative renderer evidence and Caskrun's existing bottle/session checks.
MaPC's proprietary interface, trademarks, artwork, runtime binaries and
CrossOver-derived engine sources are not included. Caskrun's original code
retains its MIT license; that does not replace the terms on adapted material.

## External runtimes and dependencies

Wine, Wine Mono, Wine Gecko, FreeType, GnuTLS, and their dependencies retain their respective licenses. They are downloaded or built separately; their binaries and source archives are excluded from this Git repository. Rust dependencies retain their own licenses, with versions recorded in `core/Cargo.lock`.

The original compressed-backup format uses pinned [tar 0.4.46](https://docs.rs/tar/0.4.46/tar/)
and [flate2 1.1.10](https://docs.rs/flate2/1.1.10/flate2/) with its explicitly selected
Rust `miniz_oxide` backend. Their licences and dependency notices remain separate
from Caskrun's licence. The build collects the locked ARM64 dependency packages'
provided licence and notice texts in `Contents/Resources/RUST_DEPENDENCIES.txt`;
the generated file contains package names, versions and public source links,
without local registry paths. Runtime notices remain in the runtime package.

A future binary release requires a separate audit of corresponding source, license texts, notices, redistribution obligations, and embedded private build paths. The current local development DMG is not a GitHub release.

## Sparkle application updater

The app build downloads the official [Sparkle 2.10.0 SDK](https://github.com/sparkle-project/Sparkle/releases/tag/2.10.0), verifies its pinned archive digest and checks cached files against that archive. Sparkle uses the MIT license with additional component notices in its complete [license file](https://github.com/sparkle-project/Sparkle/blob/2.10.0/LICENSE). The framework and full license are copied into local app builds; SDK binaries are excluded from this source repository. Original Caskrun integration code remains MIT-licensed.

## Publisher software and artwork

Store and launcher names, trademarks, installers, and artwork belong to their respective publishers. Caskrun references official download locations and fetches artwork at runtime; downloaded installers and artwork caches are not included here. Publisher software and pictures are not relicensed by Caskrun. Remote picture provenance and the absence of a blanket reuse license are documented in [the browser catalog](catalog/BROWSER_GAMES.md); publishing these links does not grant permission to redistribute the pictures or games.

No CrossOver code, runtime, recipes, or artwork is included in this project. User-directed bottle import copies user application data locally and leaves the source unchanged.
