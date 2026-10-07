# Contributing to the Caskrun demo

Caskrun is an early demo for Apple Silicon Macs. More features are coming, and app/game compatibility remains experimental. Keep feature claims tied to actual tests.

## Original work and licenses

Use original code and separately licensed upstream components. Do not copy CrossOver code, recipes, artwork, or runtimes. Keep existing source attributions and license notices. Caskrun's original source is MIT-licensed; Wine patches are LGPL-2.1-or-later.

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

For app changes, build on Apple Silicon with `./scripts/build.sh` and inspect the affected native flow. Record compatibility tests and their limits. Keep GitHub private until the project owner explicitly requests public visibility. Binary release readiness is separate from this source demo.
