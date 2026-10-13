# Source licenses and third-party components

Caskrun is an independent project. Its original app, Rust core, catalog metadata, build tooling, and Caskrun artwork are covered by the [MIT license](LICENSE). The patches and imported community data below retain their separate licenses.

## Wine and DXMT patches

The changes in `runtime-patches/wine-*.patch` and `runtime-patches/dxmt-*.patch` are offered under **LGPL-2.1-or-later**, matching their respective upstream projects. The complete [LGPL 2.1 text](runtime-patches/licenses/COPYING.LIB) is included. See [the patch directory notice](runtime-patches/README.md) for the pinned source revisions and patch order.

- [Wine 11.0](https://gitlab.winehq.org/wine/wine/-/tree/wine-11.0): Copyright (c) 1993–2026 the Wine project authors; its source contains the individual author notices.
- [DXMT at the reviewed revision](https://github.com/3Shain/dxmt/tree/e94c312f5c054263acf261cfa109edf13e757587): Copyright (c) 2023–2026 Feifan He for CodeWeavers. This is separately licensed public source, not code taken from an installed CrossOver application.

Caskrun's MIT license does not replace these licenses or upstream copyright notices. Original MIT-licensed helpers and plist templates in the same directory are not Wine or DXMT source.

## AnyPS5 patches

`runtime-patches/anyps5/` contains modifications to the separately licensed [AnyPS5 project](https://github.com/boykopovar/AnyPS5/tree/8b393b6e9a8ade9188d4f74a0293e0efd9828597), under **GPL-2.0-only**. Its exact upstream license is included as [COPYING](runtime-patches/anyps5/COPYING). The [patch notes](runtime-patches/anyps5/README.md) preserve the source revision, application order and validation limits. No AnyPS5 binaries, console firmware, keys or game files are included in this source repository.

## Community compatibility data

`catalog/game-compatibility.json` contains a metadata-only snapshot from [Highball's community database](https://github.com/gauthierpiarrette/highball-db/tree/d8d165834cd0b77613ac0f00300e8fac0b98b143), under **CC0-1.0**. Its original [license text](catalog/licenses/highball-db-CC0.txt) is included. The snapshot records its source revision and links each report to its public source; it does not import contributor identity fields or report prose. See [data provenance](docs/GAME_COMPATIBILITY.md). Caskrun's MIT license does not replace the snapshot's CC0 terms.

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

## Publisher software and artwork

Store and launcher names, trademarks, installers, and artwork belong to their respective publishers. Caskrun references official download locations and fetches artwork at runtime; downloaded installers and artwork caches are not included here. Publisher software and pictures are not relicensed by Caskrun. Remote picture provenance and the absence of a blanket reuse license are documented in [the browser catalog](catalog/BROWSER_GAMES.md); publishing these links does not grant permission to redistribute the pictures or games.

No CrossOver code, runtime, recipes, or artwork is included in this project. User-directed bottle import copies user application data locally and leaves the source unchanged.
