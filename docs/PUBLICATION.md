# Private GitHub source demo

Caskrun is in the demo phase and remains privately hosted while development continues. More features and improvements are coming. Its original source is MIT-licensed; Wine patches keep their LGPL-2.1-or-later license.

## Initial publication scope

Only reviewed source, synthetic tests, original logo pixels, catalog data, build scripts, runtime patches, and sanitized documentation are uploaded. Build products, local experiments, runtime binaries, installers, bottles, logs, screenshots, and the local DMG are excluded. A binary release requires its own privacy and runtime-license review.

## Privacy review

The initial review covered 58 previous local commits and 282 distinct historical file blobs. It found a local home path in one test fixture, one real bottle identifier in smoke documentation, identifying Git author/committer metadata, and nonpixel provenance metadata in the original logo files. Those references were removed from the upload; logo pixel data was preserved exactly. Synthetic home fixtures were changed to non-user paths. Upstream license credits, official references, and synthetic URL-validation fixtures were retained.

GitHub receives a fresh root commit with neutral `Caskrun Contributors` authorship, `contributors@caskrun.invalid` email, and a UTC timestamp. The earlier local history is archived only in ignored local build storage and is not uploaded. Private match terms stay in the local Git directory.

The tracked privacy checker and commit/push hooks inspect staged files, effective commit identity, and every commit reachable from outgoing tips. They reject detected private values, credential patterns, unsafe artifacts, personal host paths, nonneutral author metadata, and unsafe logo metadata. The exclusions also keep local build and user data out of ordinary staging.

These checks reduce accidental disclosure; every future upload still needs review. GitHub repository ownership remains associated with the account that owns it. This source cleanup does not change that account's profile, GitHub's account records, or material previously uploaded to unrelated repositories.
