# Contributing to the Caskrun demo

Caskrun is an early demo for Apple Silicon Macs. More features are coming, and app/game compatibility remains experimental. Keep feature claims tied to actual tests.

## Issues and pull requests

Use GitHub issues for bugs, reproducible compatibility results and focused feature proposals. Before reporting a result, check [known gaps](docs/FEATURE_GAPS.md) and the [launcher validation record](docs/LAUNCHER_VALIDATION.md). Include the Caskrun source revision, macOS version, runtime version, application version, steps, expected behavior and the stage actually reached: installation, sign-in, download, launch or play. Do not include account credentials, full game files or raw personal logs.

Keep pull requests focused. Explain the problem, resulting behavior and relevant checks; preserve existing ownership and isolation safeguards. Use original or synthetic fixtures. A source test, visible window or successful installer exit is not a gameplay result. Report untested behavior explicitly. Use [SECURITY.md](SECURITY.md) for private vulnerability reports rather than a public issue.

The [feature inventory](docs/FEATURES.md) and [feature gaps](docs/FEATURE_GAPS.md) serve as the demo roadmap. There is no promised release schedule or broad game-support commitment.

## Original work and licenses

Use original code and separately licensed upstream components. Do not copy CrossOver code, recipes, artwork, or runtimes. Keep existing source attributions and license notices. Caskrun's original source is MIT-licensed; Wine and DXMT patches are LGPL-2.1-or-later, AnyPS5 patches are GPL-2.0-only, and the Highball compatibility snapshot is CC0-1.0. See [third-party notices](THIRD_PARTY_NOTICES.md) and each patch directory's terms.

## Protect personal information

Never commit credentials, account details, names or emails from local Git settings, private home paths, device identifiers, real bottle identifiers, user bottles, logs, screenshots, downloaded installers, runtimes, or built apps. Use synthetic fixtures and repository-relative paths. Build and smoke-test data belong in ignored `.build/` or Caskrun's own local user storage.

Before a first commit, run:

```sh
python3 scripts/setup-publication-privacy.py
TZ=UTC python3 scripts/check-publication-privacy.py --staged
```

Setup stores private match terms in Git's local configuration directory, sets a neutral project author, and installs the tracked commit/push hooks. The private match file must never be tracked. Add any additional private identifiers to its local `terms` array. Commit in UTC:

```sh
TZ=UTC git commit -m "Describe the change"
python3 scripts/check-publication-privacy.py --revision HEAD
```

The commit hook checks staged source and identity; the push hook checks the complete reachable history of each outgoing tip. Do not bypass them. Pattern checks help prevent mistakes and do not replace reviewing the files and history being uploaded.

## Validation

Use disposable test homes, never real user bottles, for automated tests:

```sh
(cd core && cargo test -- --test-threads=1)
python3 -m unittest discover -s catalog -p 'test_*.py'
python3 scripts/test_publication_privacy.py
```

For app changes, build on Apple Silicon with `./scripts/build.sh` and inspect the affected native flow. Use `CASKRUN_BUILD_OUTPUT_DIR` inside `.build/` to prepare a separate candidate while an existing app or its bundled Wine process remains active. Record compatibility tests and their limits. Compiler and fixture checks do not replace a live check of the changed flow.

Before attaching anything to a public issue or pull request, inspect it yourself. Structured Caskrun diagnostic reports omit identifying fields; raw Wine logs, screenshots and bottle backups can contain paths, account details or private app data. Do not upload them without removing private material. The app does not upload diagnostics automatically.

Public source publication is separate from binary release readiness. Do not attach local development DMGs, downloaded runtimes or game files to a contribution.
