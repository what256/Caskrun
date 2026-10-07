# Source licenses and third-party components

Caskrun is an independent project. Its original app, Rust core, catalog data, build tooling, and Caskrun artwork are covered by the [MIT license](LICENSE), except where a file or directory states otherwise.

## Wine patches

The original changes in `runtime-patches/*.patch` are offered under **LGPL-2.1-or-later**, matching upstream Wine. See [the patch directory notice](runtime-patches/README.md). Caskrun's MIT license does not replace this license or the license of the Wine source to which these patches apply.

## External runtimes and dependencies

Wine, Wine Mono, Wine Gecko, FreeType, GnuTLS, and their dependencies retain their respective licenses. They are downloaded or built separately; their binaries and source archives are excluded from this Git repository. Rust dependencies retain their own licenses, with versions recorded in `core/Cargo.lock`.

A future binary release requires a separate audit of corresponding source, license texts, notices, redistribution obligations, and embedded private build paths. The current local development DMG is not a GitHub release.

## Publisher software and artwork

Store and launcher names, trademarks, installers, and artwork belong to their respective publishers. Caskrun references official download locations and fetches artwork at runtime; downloaded installers and artwork caches are not included here. Publisher software is not relicensed by Caskrun.

No CrossOver code, runtime, recipes, or artwork is included in this project. User-directed bottle import copies user application data locally and leaves the source unchanged.
