# MaPC / MacadeKit adaptation record

Upstream: https://github.com/abduzeedo/mapc-engine

Reviewed revision: `37bb046423dbb85c5cb98fee742868dccb96002d`.

Copyright 2026 Steale LLC. Apache License 2.0. The exact upstream LICENSE and
NOTICE are retained here and included in local app packages. Individual
upstream components keep their own licenses; no third-party runtime is imported
by this adaptation.

## Referenced source

- `Sources/MacadeKit/Library/GraphicsProbe.swift`
- `Sources/MacadeKit/Wine/PEImage.swift`
- `Sources/MacadeKit/Wine/GraphicsBackend.swift`
- `Sources/MacadeKit/Stores/Launchers/LauncherStore.swift`

## Caskrun changes

The Rust scanner parses bounded PE import tables and local imported libraries
through no-follow file descriptors. It avoids guessing from DLL filenames or
scanning whole binary payloads. It recognizes Direct3D 10/11 evidence for the
existing reviewed DXMT package; unknown and mixed evidence remains conservative.
The local bottle decision is refreshed only before an idle launch. Explicit
manual preferences are retained, and neither a backup nor an active session
changes the selected engine view.

EA client lookup handles numeric version directories when the usual junction
is absent. Checked EA clients receive fixed browser rendering switches. EA
installer XML and Ubisoft machine installation records supply game identities;
fixed publisher URI requests require a current manifest and same-bottle client.
These changes use Caskrun's existing private storage and process ownership rules.

No MaPC UI, artwork, name/logo, compiled engine, store credentials or account
files are included. No CrossOver-derived engine route or Apple toolkit files
were imported. The upstream Witcher 3 proxy named in NOTICE was not copied.

Static imports cannot identify every API loaded dynamically. Source and fixture
checks do not establish publisher sign-in, game compatibility or performance.
