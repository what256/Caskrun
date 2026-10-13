# Public source demo

Caskrun is in the demo phase. Its reviewed source became public on
13 October 2026. More features and improvements are coming. Its original source
is MIT-licensed; Wine and DXMT patches keep their LGPL-2.1-or-later license,
AnyPS5 patches are GPL-2.0-only, and the reviewed Highball data snapshot is CC0.
See [third-party notices](../THIRD_PARTY_NOTICES.md).

## Initial publication scope

Only reviewed source, synthetic tests, original logo pixels, catalog data, build scripts, runtime patches, and sanitized documentation are uploaded. Build products, local experiments, runtime binaries, installers, bottles, logs, screenshots, and the local DMG are excluded. A binary release requires its own privacy and runtime-license review.

## Privacy review

The initial review covered 58 previous local commits and 282 distinct historical file blobs. It found a local home path in one test fixture, one real bottle identifier in smoke documentation, identifying Git author/committer metadata, and nonpixel provenance metadata in the original logo files. Those references were removed from the upload; logo pixel data was preserved exactly. Synthetic home fixtures were changed to non-user paths. Upstream license credits, official references, and synthetic URL-validation fixtures were retained.

The initial GitHub upload used a fresh root commit with neutral `Caskrun
Contributors` authorship, `contributors@caskrun.invalid` email, and a UTC
timestamp. The earlier local history is archived only in ignored local build
storage and is not uploaded. Private match terms stay in the local Git directory.

The tracked privacy checker and commit/push hooks inspect staged files, effective commit identity, and every commit reachable from outgoing tips. They reject detected private values, credential patterns, unsafe artifacts, personal host paths, nonneutral author metadata, and unsafe logo metadata. The exclusions also keep local build and user data out of ordinary staging.

These checks reduce accidental disclosure; every future upload still needs review. GitHub repository ownership remains associated with the account that owns it. This source cleanup does not change that account's profile, GitHub's account records, or material previously uploaded to unrelated repositories.

## Public source publication review

The visibility-change review checks all published branch histories and closed
pull-request heads, author/committer metadata, source and documentation, and
GitHub-hosted releases, issue/PR discussion, Actions artifacts and deployment
surfaces. Source licensing, dependency notices and the demo's stated validation
limits are reviewed separately. This is a publication audit, not a claim that
the entire application has passed a vulnerability audit or a binary release gate.

No local build or development DMG is part of this source publication. Test-only
game/runtime packages, captures, logs, private configuration and pre-publication
history remain excluded. The commit and push hooks continue to check later
changes; contributors should review their own data before submitting it.

## Publication result — 13 October 2026

Before changing visibility, the review covered all 35 commits reachable from
the published source, both remote branches and both closed pull-request heads.
The source-history scan checked 387 historical file entries. No personal-data
or credential findings remained in the reviewed publication scope. The signed-in
repository page also confirmed that no packages were published.

Anonymous access to the public repository and its reviewed README was verified.
GitHub secret scanning, secret push protection and private vulnerability reporting
were enabled. Actions retains read-only default token permissions and cannot
approve pull requests. These settings supplement the local privacy checks;
secret scanning does not detect every kind of personal information.

This publication contains source only. No application binary, DMG, game dump,
runtime package or test capture was released.
