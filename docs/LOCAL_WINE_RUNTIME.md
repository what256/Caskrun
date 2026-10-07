# Local Wine runtime package (development only)

Caskrun's native app is ARM64. This separate experiment packages an **x86_64 Wine 11.0** build, which runs through Rosetta on Apple Silicon and is configured for i386 and x86_64 Windows PE programs. It does not make this Wine build suitable for Steam or general Windows apps.

## Build the local package

The inputs are the independently built upstream Wine and FreeType trees described in [WINE_BUILD_EXPERIMENT.md](WINE_BUILD_EXPERIMENT.md), plus the pinned Wine Mono/Gecko MSIs and source-license notices below. From the repository root:

```sh
./scripts/package-wine-runtime-dev.sh
```

The command creates `.build/caskrun-wine-runtime-dev/` and prints the `bin/wine` path. It refuses to overwrite an existing package. It reads only the project-local Wine/FreeType build inputs, `.build/mono-gecko-probe/downloads/`, and `.build/mono-gecko-probe/licenses/`. Nothing is installed globally or placed in the Caskrun app/DMG.

The package layout is:

```text
bin/wine, bin/wineboot, bin/wineserver  -> project-authored dispatch script
runtime/                               -> staged upstream Wine build
runtime/share/wine/mono/*.msi          -> matching Wine Mono package
runtime/share/wine/gecko/*.msi         -> matching 32/64-bit Wine Gecko packages
lib/libfreetype.6.dylib                -> project-built upstream FreeType
licenses/                              -> Wine, FreeType, Mono, Gecko notices/licenses
SOURCES.txt                            -> pinned upstream source locations/hashes
```

The dispatch script finds the package root from its own path. It sets `WINEDLLPATH`, `WINEDATADIR`, `WINELOADER`, and `WINESERVER` to the packaged Wine paths, and sets `DYLD_FALLBACK_LIBRARY_PATH` to the packaged FreeType directory. Wine calls `dlopen("libfreetype.6.dylib")`; the package therefore supplies that path **internally**, without asking the caller to set `DYLD_LIBRARY_PATH`. The copied FreeType dylib's build-directory install name is changed to `@rpath/libfreetype.6.dylib`, then ad-hoc re-signed. Wine's staged Mach-O signatures are preserved and verified. All package symlinks are relative.

The packaging script checks all staged Mach-O files in Wine's executable and Unix module directories for x86_64 architecture, valid local signatures, and direct dependencies limited to macOS system libraries or Wine `@rpath`/relative modules. It also rejects absolute symlinks. This does not enumerate every library that Wine may load later with `dlopen`.

This wrapper is a development workaround, not a finished runtime policy. The Wine loader still contains the old build prefix as a fallback string, and the package has not been Developer ID signed or notarized. A final app bundle must verify that macOS permits the required loader environment under its actual signing and launch context, then include all required dependency source and license compliance material.

## Matching Wine Mono and Gecko inputs

Wine 11.0's [addon source](https://github.com/wine-mirror/wine/blob/wine-11.0/dlls/appwiz.cpl/addons.c) pins Mono **10.4.1** and Gecko **2.47.4**. Download these exact upstream MSIs over HTTPS into `.build/mono-gecko-probe/downloads/` before packaging. The packaging script verifies each file against the SHA-256 embedded in Wine 11.0; Wine does not hash its locally discovered MSI before installing it.

| File | Official WineHQ download | SHA-256 |
| --- | --- | --- |
| `wine-mono-10.4.1-x86.msi` | [Mono 10.4.1](https://dl.winehq.org/wine/wine-mono/10.4.1/wine-mono-10.4.1-x86.msi) | `071f4b2887e1c97a11d791ff3d65be9429eed6dec4c2708888bfd546ba358e23` |
| `wine-gecko-2.47.4-x86.msi` | [Gecko x86](https://dl.winehq.org/wine/wine-gecko/2.47.4/wine-gecko-2.47.4-x86.msi) | `26cecc47706b091908f7f814bddb074c61beb8063318e9efc5a7f789857793d6` |
| `wine-gecko-2.47.4-x86_64.msi` | [Gecko x86_64](https://dl.winehq.org/wine/wine-gecko/2.47.4/wine-gecko-2.47.4-x86_64.msi) | `e590b7d988a32d6aa4cf1d8aa3aa3d33766fdd4cf4c89c2dcc2095ecb28d066f` |

The package script also requires these version-tagged notices in `.build/mono-gecko-probe/licenses/`: `Mono-COPYING.txt` from [Wine Mono 10.4.1](https://raw.githubusercontent.com/wine-mono/wine-mono/wine-mono-10.4.1/COPYING), and `Gecko-LICENSE.txt` plus `Gecko-license.html` from [Wine Gecko 2.47.4](https://gitlab.winehq.org/wine/wine-gecko/-/tree/wine-gecko-2.47.4). Their SHA-256 values are pinned in the script. Wine Mono's notice describes mixed MIT, LGPL, GPL-with-exception, and other component terms; Gecko's notice points to MPL-2.0 and third-party terms. The WineHQ MSI directories did not expose detached signatures during this check, so the hardcoded Wine 11.0 hashes are the integrity gate. Release distribution still needs full third-party license and source-availability review.

The wrapper's `WINEDATADIR` points to `runtime/share/wine`. Wine's [local addon lookup](https://github.com/wine-mirror/wine/blob/wine-11.0/dlls/appwiz.cpl/addons.c) searches its `mono/` or `gecko/` subdirectory for the exact MSI filename before offering a download. These packages increase the development runtime from about 1.5 GB to **1.7 GB**; installing them into a fresh bottle increased that test prefix to about **2.0 GB**. A shared extracted-payload layout may reduce per-bottle duplication later.

## Local relocation smoke on 23 September 2026

A cloned package was placed at `.build/runtime-relocation-smoke/Caskrun Wine/` to test a different path containing a space. The caller explicitly removed `DYLD_LIBRARY_PATH`, `DYLD_FALLBACK_LIBRARY_PATH`, `WINEDLLPATH`, `WINEDATADIR`, `WINELOADER`, and `WINESERVER` before starting each wrapper. Results:

| Check | Result |
| --- | --- |
| Relocated `bin/wine --version` | Exit 0, `wine-11.0` |
| Fresh relocated `bin/wineboot --init` | Exit 0; `system.reg` and `drive_c/windows` appeared; zero missing-FreeType messages |
| Fresh relocated `wineboot` with `WINEDEBUG=+font` | Exit 0; Wine traced `FreeType version is 2.14.3`; zero missing-FreeType messages |
| Self-authored x86_64 console PE through relocated `bin/wine` | Exit 37 as programmed; expected `CASKRUN-WINE-SMOKE:x86_64:64` output |
| Relocated Mach-O signatures | `codesign --verify --strict` passed for Wine and FreeType |
| Relocated FreeType identity | `otool -D` reported `@rpath/libfreetype.6.dylib` |
| Isolated relocation check | Original staged Wine directory and original FreeType dylib were temporarily moved aside; relocated Wine still reported its version and initialized another fresh prefix, tracing FreeType 2.14.3 13 times and reporting zero missing-FreeType messages. The original paths were restored in a `finally` block and verified present. |
| Caskrun core with relocated wrapper | The ARM64 core's `probe-runtime` reported `wine-11.0` and found sibling `wineboot`; it created and initialized a new Caskrun bottle in `.build/runtime-relocation-smoke/core-home/` in 13 seconds, with `system.reg` present. No caller DYLD path was set. |

These earlier relocation tests used `WINEDLLOVERRIDES='mscoree,mshtml='`; the later addon test below removed it. The cloned package's Wine startup still logged missing Vulkan support, RPC service startup errors, and Bluetooth/device errors. The isolated test demonstrates that **this startup path** did not need the original staged Wine or FreeType paths; it does not prove every Wine feature or Windows program uses only packaged files.

## Fresh prefix without diagnostic overrides

Before adding the local MSIs, `wineboot --init` in a fresh prefix **timed out after 60 seconds** without `WINEDLLOVERRIDES`. Its `+appwizcpl` trace looked for `wine-mono-10.4.1-x86.msi` in Wine's cache, did not find it, and prepared Wine's Mono download dialog. The prefix-specific wineserver was stopped after the timeout. This was a concrete missing-addon blocker, not a FreeType failure.

With the three SHA-verified MSIs in the development package, another fresh `wineboot --init` ran with **no caller DYLD variables and no `WINEDLLOVERRIDES`**. It exited 0 in about 10 seconds, created `system.reg`, and registered Wine Mono **10.4.1** from the package's local `mono/` directory. Two self-authored PE probes then invoked Wine's `mshtml.dll` installer entrypoint: the x86_64 probe exited 0 in 2.3 seconds, and the i686 probe exited 0 in 1.4 seconds. The registry records Wine Gecko **2.47.4** for both architectures with `InstallSource` pointing to the package's local `gecko/` directory; both installed trees contain `xul.dll`. The probe's zero result alone is insufficient because Wine's entrypoint can return success without loading Gecko, so the registry and installed files are the stronger evidence. HTML rendering itself was not tested.

Caskrun's ARM64 core then selected the rebuilt wrapper in a **new** `.build/mono-gecko-probe/core-no-override-home/` data directory, with no caller DYLD variables and no `WINEDLLOVERRIDES`. `probe-runtime` reported `wine-11.0`; `initialize` completed in 26 seconds. `launch-wait` installed the self-authored one-file MSI into that bottle, reported direct Wine exit code **0**, and the installed payload's SHA-256 was `2487bf13d3fe0ac390b814d8170ce0d5a0cea61652124a9d17fbe23808a31161`, matching the source. This is a narrow quiet-MSI smoke, not validation of downloaded installers or of the SwiftUI installer flow.

The no-override tests still logged missing Vulkan support and RPC/Bluetooth/device errors. GnuTLS, SDL2, Vulkan/MoltenVK, FFmpeg, GStreamer and other feature dependencies are absent from this build. Keep the package as a project-local artifact until real-app, GUI, network/TLS, controller, graphics and media tests; dependency completion; a source/notice audit; and release signing are done.

A later [Steam smoke test](STEAM_SMOKE.md) used a clone of this package: Valve's installer completed in a disposable bottle, the updater ran, and Steam/CEF login processes started. Steam connection-manager certificate checks failed, so sign-in and game compatibility remain unverified.

Wine is LGPL-2.1-or-later; FreeType is offered under the FreeType License or GPL-2.0. The package includes upstream Wine, FreeType, Mono, and Gecko license texts and source references, but does **not** yet include a release-ready source-availability process. Do not redistribute this development package as a supported Caskrun runtime.

## Separate GnuTLS-enabled development runtime

The original package above remains intact. A second, isolated Wine 11.0 build adds the x86_64 GnuTLS crypto backend used by Wine's bcrypt signature checks and Schannel. Its project-local package is `.build/tls-wine-runtime-dev/` (about **1.7 GB logical size**). It contains the same Wine Mono 10.4.1 and Wine Gecko 2.47.4 MSIs, FreeType 2.14.3, license notices, and source metadata as the original package, plus GnuTLS 3.8.13 with statically linked GMP 6.3.0 and Nettle 3.10.2. It is still a development artifact, not part of a signed release.

The tracked reproduction path on an Apple Silicon Mac with Rosetta and the earlier Wine/FreeType build inputs is:

```sh
./scripts/package-wine-runtime-dev.sh       # only if the original package is absent
./scripts/build-wine-crypto-deps-dev.sh
./scripts/build-wine-tls-dev.sh
./scripts/package-wine-runtime-tls-dev.sh
```

The first script supplies the independently built FreeType and SHA-checked Mono/Gecko inputs to the TLS variant. The crypto-dependency script downloads pinned upstream sources, checks their SHA-256 hashes, builds x86_64 static GMP and Nettle, runs their test suites, and builds a shared x86_64 GnuTLS with bundled libtasn1/unistring. The second Wine script configures the original Wine 11.0 source with `--with-gnutls`, refuses a missing GnuTLS backend, builds both i386 and x86_64 Windows PE targets, and stages Wine separately. The final script verifies source/addon hashes, copies into a separate package, changes GnuTLS's absolute build-tree install name to `@rpath/libgnutls.30.dylib`, checks architecture/dependencies, and ad-hoc signs the copied Mach-O files. The build scripts encode the completed source builds but have not been rerun end to end as a single sequence. The tracked packager **was** rerun into `.build/tls-wine-runtime-repro/`; its version and P-256 checks passed, its GnuTLS dylib SHA-256 matched the first package, and its notices/source metadata compared identically.

| Source | SHA-256 | Local result |
| --- | --- | --- |
| [GMP 6.3.0](https://ftp.gnu.org/gnu/gmp/gmp-6.3.0.tar.xz) | `a3c2b80201b89e68616f4ad30bc66aee4927c3ce50e33929ca819d5c43538898` | Portable-C x86_64 static library; `make check` passed. |
| [Nettle 3.10.2](https://ftp.gnu.org/gnu/nettle/nettle-3.10.2.tar.gz) | `fe9ff51cb1f2abb5e65a6b8c10a92da0ab5ab6eaf26e7fc2b675c45f1fb519b5` | Portable-C x86_64 static libraries; `make check` passed. |
| [GnuTLS 3.8.13](https://www.gnupg.org/ftp/gcrypt/gnutls/v3.8/gnutls-3.8.13.tar.xz) | `ffed8ec1bf09c2426d4f14aae377de4753b53e537d685e604e99a8b16ca9c97e` | x86_64 shared library; a Rosetta `gnutls_global_init()` smoke passed. Full GnuTLS tests were disabled for this diagnostic build. |

Detached source signatures verified mathematically. GMP and Nettle were signed by Niels Möller's fingerprint `343C2FF0FBEE5EC2EDBEF399F3599FF828C67298`; GnuPG reports that key expired now. GnuTLS's archive had valid signatures from Zoltan Fridrich `5D46CB0F763405A7053556F47A75A648B3F9220C` and Alexander Sosedkin's subkey `A6AB53A01D237A94F9EEC4D0412748A40AFCC2FB`, using the [official GnuTLS release keyring](https://gnutls.org/gnutls-release-keyring.gpg). GnuPG did not certify the latter keys' identities through a trusted web-of-trust path. The package includes the Wine, FreeType, Mono, Gecko, GnuTLS, GMP, and Nettle notices and their source references; a release still needs a complete source-availability and third-party license audit.

The built package's GnuTLS dylib advertises `@rpath/libgnutls.30.dylib` and lists only Apple system libraries as other direct dependencies. Its `bin/wine --version` returned `wine-11.0`. A clone at `.build/tls-validation/Relocated TLS Runtime/` also returned that version and verified a valid P-256 test certificate with caller DYLD/Wine path variables and `WINEDLLOVERRIDES` removed. A separate fresh-prefix `bin/wineboot --init` with the same clean caller environment exited 0 in **16.9 seconds**, created `system.reg`, and installed local Wine Mono files. These checks do not prove every library loaded by a real app is relocatable.

The self-authored x86_64 certificate probe in ignored `.build/tls-validation/` calls `CryptVerifyCertificateSignatureEx` and `CertGetCertificateChain` on a locally generated, valid, self-signed P-256 certificate. On the original Wine build, direct verification failed and its chain status was `0x28`: `0x20` for the intentionally untrusted test root plus `0x08` for invalid signature. With the GnuTLS-enabled package, verification returned success and chain status became `0x20` only. A self-signed RSA-2048 control already verified with the original build. Wine defines `0x08` as `CERT_TRUST_IS_NOT_SIGNATURE_VALID` in [wincrypt.h](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/include/wincrypt.h); Wine's [chain builder](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/dlls/crypt32/chain.c) sets it after a signature failure. No certificate validation was disabled.

A 120-second Steam run in a clone of the earlier Steam bottle, with this runtime and no caller DYLD or Wine overrides, produced **zero** repeats of the earlier `Crypto API failed certificate check` message. Steam's connection log recorded a WebSocket `ConnectionCompleted()` and later a `[Connected]` state. It then received `'Try another CM' / 'Failure'` from the remote connection manager, disconnected, and connected to a second manager before the bounded test ended. It still logged `LogonFailure No Connection`; this is a narrower crypto/connectivity improvement, **not Steam sign-in or game compatibility**. No credentials were entered. The original runtime, Steam bottle, and installed CrossOver were unchanged.

GnuTLS was built without a default trust-store path or PKCS#11, and Wine's macOS CryptoAPI trust handling remains a separate issue. Vulkan, media, controllers, GUI interaction, full network behavior, signing/notarization, and release source obligations remain unvalidated. The TLS package is a candidate for a local self-contained development app, not a supported public runtime.

## Separate Steam Wine build

A focused disposable Steam prototype using [original Caskrun Wine patches](../runtime-patches/README.md) produced a usable sign-in window, one Steam Dock item, and Dock click returning focus to that window. The final tracked Dock patch removes one redundant prototype guard, so the exact full source rebuild still needs its own Steam smoke and game-child identity check. Both tracked patches target WineHQ Wine 11.0 only. One adds CEF GPU flags to Steam's web-helper Windows command line; the other selects a Steam-named native loader for Steam processes and keeps the background Steam process out of the Dock. Caskrun extracts the icon from the user's own Steam executable.

The independent full-rebuild path verifies WineHQ's pinned Wine 11.0 SHA-512, the two patch SHA-256 values, and a reviewed Steam loader plist template. It extracts a **fresh** source tree and leaves the existing app/runtime alone:

```sh
./scripts/build-wine-steam-dev.sh --check
./scripts/build-wine-steam-dev.sh
./scripts/package-wine-runtime-steam-dev.sh
```

The build uses the same project-local x86_64 GnuTLS, FreeType, and Bison inputs described above. The package target is uniquely `.build/steam-wine-runtime-dev/`; both build and package scripts refuse to overwrite existing outputs. The package contains the exact corresponding Wine 11.0 source archive, both patches, the loader plist template, Wine LGPL notices, and existing dependency notices/source references. The `Steam` Mach-O loader alias is ad-hoc signed, and the packager signs/verifies every staged x86_64 Unix Mach-O and checks direct dependencies. This is still a local development package, without Developer ID signing, notarization, or broad game validation. The app/DMG remains on its existing runtime until the new package is tested and deliberately integrated.

## Bundle relocation check

Caskrun's core records a SHA-256 identity of the complete packaged Wine tree when it initializes a bottle. In a disposable real-runtime test, an initialized bottle accepted an identical copy at a path containing a space, then rejected that copy after `SOURCES.txt` was changed. Custom Wine paths remain tied to their original location; an older bottle record without a package digest can migrate only while its original engine is still available. This test did not move or modify the installed CrossOver app.

Hashing the 1.7 GB development engine currently adds about **3 seconds** to a repeated bottle initialization check on this Mac (the measured repeat was 3.04 seconds; the identical relocated copy took 3.82 seconds). That same check runs before some bottle operations and launches, so it needs a safe performance improvement before a public release. The first fresh bottle initialization took 21.39 seconds including Wine startup and identity hashing.
