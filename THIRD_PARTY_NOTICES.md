# Source licenses and third-party components

Caskrun is an independent project. Its original app, Rust core, catalog data, build tooling, and Caskrun artwork are covered by the [MIT license](LICENSE), except where a file or directory states otherwise.

## Wine patches

The original changes in `runtime-patches/*.patch` are offered under **LGPL-2.1-or-later**, matching upstream Wine. See [the patch directory notice](runtime-patches/README.md). Caskrun's MIT license does not replace this license or the license of the Wine source to which these patches apply.

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

Store and launcher names, trademarks, installers, and artwork belong to their respective publishers. Caskrun references official download locations and fetches artwork at runtime; downloaded installers and artwork caches are not included here. Publisher software is not relicensed by Caskrun.

No CrossOver code, runtime, recipes, or artwork is included in this project. User-directed bottle import copies user application data locally and leaves the source unchanged.
