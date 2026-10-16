# Caskrun project rules

- This repository is an original, open-source Windows-app launcher project.
- Do not copy code, installer recipes, images, icons, fonts, or bundled runtimes from CrossOver or its app bundle.
- Treat public product documentation as functional requirements. Use separately licensed upstream Wine components for compatibility.
- Never edit, replace, launch, or remove `/Applications/CrossOver.app` or the existing `~/Desktop/CrossOver-Lab` while working here.
- Keep builds, tests, and downloaded runtimes inside this repository or Caskrun's own user-data directory.
- Target Apple Silicon Macs. Windows x86/x64 app support remains necessary, but Intel Mac hardware support is out of scope.
- Record features honestly as planned, implemented, or validated. Do not claim CrossOver parity until each feature is tested.

## Local build retention

- After completed work, retain one current reviewed runtime package in `.build/` and one current installer with its checksum in `dist/`. Remove superseded runtime copies and compiler caches after checking that no process uses them.
- Keep automated test source; remove disposable test bottles, downloaded installers, VM experiments and generated test files after ownership checks. Preserve unique fixes, corresponding source and license notices before removing experimental trees.
- Apply reviewed worktree fixes to the main checkout before pruning worktrees. Keep compact patch handoffs and reproduction notes instead of duplicate source and build trees.

## GitHub publication and privacy

- The user has authorized public source publication. Describe Caskrun as an open-source launcher in active development. Keep compatibility claims tied to verified results and name unresolved issues. Publish reviewed source only; binary releases require a separate privacy and runtime-license review.
- Never commit personal information, credentials, local home paths, device or real bottle identifiers, user data, logs, screenshots, downloaded binaries, or build products.
- Use the neutral local identity configured by `scripts/setup-publication-privacy.py`. Run commits with `TZ=UTC` and retain the commit/push hooks in `.githooks/`.
- Review `scripts/check-publication-privacy.py --staged` before a commit and `--revision HEAD` before uploading. Private terms are stored only in Git's local directory, never source files.
- The pre-publication Git history is archived only in ignored `.build/`; do not upload it or attach local development DMGs without a separate privacy and runtime-license review.
