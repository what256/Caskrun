# Independent Wine build experiment (Apple Silicon)

_Evidence captured 23 September 2026. This is a project-local Wine build and basic console-program experiment, not a validated app-compatibility runtime. Project-local Bison and x86_64 FreeType were built; Wine 11.0 was configured, compiled, staged, ad-hoc signed, and smoke-tested under Rosetta. CrossOver was untouched._

The sections below record the original staged-build experiments in order. For the later relocatable package and Caskrun MSI result, see [the follow-up](#follow-up-relocated-development-package-and-core-msi-test) and [local runtime notes](LOCAL_WINE_RUNTIME.md).

## Scope and decision

Caskrun targets **Apple Silicon Macs**. The first practical compatibility experiment is an **x86_64 macOS Wine process under Rosetta**, built from WineHQ source inside this repository. Caskrun's Swift/Rust app stays native ARM64. Wine 11.0's `--enable-archs=i386,x86_64` option is the candidate for both 32- and 64-bit Windows PE code; it is a configuration option, **not proof that either class of Windows app works**. There is no requirement to build or test Caskrun on Intel Mac hardware.

This is a bridge. [Apple says](https://developer.apple.com/news/?id=w5ngl9k2) macOS 27 is the final general-purpose Rosetta release. An eventual ARM-native Wine route still needs a separately usable x86/x64 translator for Steam and other Intel Windows apps. The [current upstream FEX build](https://github.com/FEX-Emu/FEX/blob/main/CMakeLists.txt) explicitly supports Linux and Windows, not macOS; an upstream, independently buildable macOS solution has not been verified.

## Evidence from this Mac

| Check | Observed result | Meaning |
| --- | --- | --- |
| `uname -m`, `sw_vers`, `xcodebuild -version` | arm64; macOS 26.5.2; Xcode 26.6 | Current build host is Apple Silicon with an Apple toolchain. |
| Rosetta receipt and `/usr/bin/arch -x86_64 /usr/bin/uname -m` | Rosetta receipt present; command prints `x86_64` | Intel processes can start on this Mac now. |
| Small **self-authored** C program built with `xcrun clang -target x86_64-apple-macos13`, ad-hoc signed, verified, then run with `arch -x86_64` | `file` reports Mach-O x86_64; signed program exits 0 | Native Apple compiler can produce a locally signed Intel process that Rosetta executes. This says nothing about Wine's build or dependencies. |
| `/usr/bin/arch -x86_64 /usr/bin/clang -print-target-triple` | Printed `arm64-apple-darwin25.5.0` | **Explicit `-target x86_64-apple-macos13` is required**; running the compiler through `arch` alone did not change its default target. |
| `file` on `/opt/homebrew/lib/libfreetype.dylib`, `libgnutls.dylib`, `libSDL2.dylib` | All ARM64-only | These installed libraries cannot be linked into an x86_64 Wine process. A separate x86_64 FreeType was subsequently built below. |
| PE cross-compilers | `i686-w64-mingw32-gcc` and `x86_64-w64-mingw32-gcc` in `/opt/homebrew/bin` | Wine configure found both and reported their basic compiler tests passing. The diagnostic build subsequently completed. |

The test source, ad-hoc signed executables, local Bison and FreeType, staged Wine, disposable prefix, and all build logs are confined to ignored `.build/wine-build-probe/`. No system-wide packages were installed. The installed CrossOver was not accessed.

## Pinned upstream source

Downloaded [WineHQ's Wine 11.0 source archive](https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz) and [WineHQ's signed SHA-512 list](https://dl.winehq.org/wine/source/11.0/sha512sums.asc) into ignored `.build/wine-build-probe/`. The archive's computed SHA-512 **equals** the `wine-11.0.tar.xz` entry in that list:

```text
a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b
```

The list's OpenPGP signature and the [archive's detached signature](https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz.sign) both verified mathematically with fingerprint `DA23579A74D4AD9AF9D3F945CEFAC8EAAF17519D`, imported into a project-local GnuPG directory from [keys.openpgp.org](https://keys.openpgp.org/vks/v1/by-fingerprint/DA23579A74D4AD9AF9D3F945CEFAC8EAAF17519D). As a separate official WineHQ reference, a shallow clone of the [WineHQ GitLab `wine-11.0` tag](https://gitlab.winehq.org/api/v4/projects/wine%2Fwine/repository/tags/wine-11.0) was fetched into `.build/wine-build-probe/wine-git-verify/`; `git tag -v wine-11.0` verified with the **same fingerprint** and names commit `db11d0fe6a169c457e23d007e20404643d067aa8`. The [Wine 11.0 release announcement](https://list.winehq.org/hyperkitty/list/wine-announce@list.winehq.org/thread/MXULWEMXQ5C24UEJ2STHPIUHNOZ7AZPQ/) links the source and names Alexandre Julliard. This provides two official WineHQ surfaces using the same signer key, sufficient for this local startup experiment. GnuPG still reports the UID `Alexandre Julliard <julliard@winehq.org>` **with unknown web-of-trust certification**; release provenance needs a documented trusted-key policy.

The tarball contains the [Wine 11.0 README](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/README.md), `configure`, and `configure.ac`. Its README lists macOS/Xcode and MinGW requirements. The [tagged configure source](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/configure.ac) confirms `--enable-archs=i386,x86_64` and makes a missing requested PE compiler an error. Its `configure` script was run three times as described below, followed by one diagnostic build and local startup test. The signer-key trust caveat above is still a release-provenance gate.

## First configure attempt: exact build gate

The following configure command was run on 23 September 2026 with all outputs in `.build/wine-build-probe/`. Before repeating it for a release workflow, recheck the source SHA-512 and signatures and apply a documented trusted-key policy.

```sh
repo="$(git rev-parse --show-toplevel)"
probe="$repo/.build/wine-build-probe"
mkdir -p "$probe/src" "$probe/build" "$probe/stage" "$probe/deps-x86_64/lib/pkgconfig"
tar -xJf "$probe/wine-11.0.tar.xz" -C "$probe/src"
cd "$probe/build"
env -u PKG_CONFIG_PATH \
  PKG_CONFIG_LIBDIR="$probe/deps-x86_64/lib/pkgconfig" \
  CC='/usr/bin/clang -target x86_64-apple-macos13' \
  CXX='/usr/bin/clang++ -target x86_64-apple-macos13' \
  /usr/bin/arch -x86_64 /bin/sh "$probe/src/wine-11.0/configure" \
    --build=x86_64-apple-darwin \
    --enable-archs=i386,x86_64 \
    --disable-tests \
    --without-x \
    --without-gstreamer \
    --prefix="$probe/stage" \
    > "$probe/build/configure.log" 2>&1
```

**Observed:** configure exited 1. It reported build and host `x86_64-apple-darwin`, accepted the C and C++ compiler, and found a recent-enough flex. It then stopped at the first dependency check: `/usr/bin/bison` is version **2.3**, while Wine 11.0 requires **Bison 3.0 or newer**. The exact message in `.build/wine-build-probe/build/configure-bison23.log` is `configure: error: Your bison version is too old. Please install bison version 3.0 or newer.` No Makefile was generated on this first attempt.

`--without-x` tests the macOS display path without X11. `--without-gstreamer` deliberately excludes media for this **toolchain diagnostic** and is unacceptable for a feature-complete release. An empty project-local `PKG_CONFIG_LIBDIR` prevents ARM-only Homebrew `.pc` files from being accepted as Intel dependencies. Do not drop `i386` merely to get a passing configure and then claim Steam compatibility.

## Project-local Bison and second configure attempt

Downloaded [GNU Bison 3.8.2 source](https://ftp.gnu.org/gnu/bison/bison-3.8.2.tar.xz), its [detached signature](https://ftp.gnu.org/gnu/bison/bison-3.8.2.tar.xz.sig), and the [GNU keyring](https://ftp.gnu.org/gnu/gnu-keyring.gpg) into ignored `.build/wine-build-probe/tool-sources/`. The source SHA-256 is `9bba0214ccf7f1079c5d59210045227bcf619519840ebfa80cd3849cff5a5bf2`; `gpgv` with that GNU keyring reported a good signature from key `7DF84374B1EE1F9764BBE25D0DDCAA3278D5264E` (Akim Demaille). This verifies the signature against the downloaded GNU keyring; it does not independently certify the keyring's provenance.

Bison configured with Apple clang and `--disable-nls`, built with `make -j8`, and installed with `make install` to `.build/wine-build-probe/toolchain/`, with no `sudo`. The staged `bin/bison` is ARM64, ad-hoc signed, passes `codesign --verify --strict`, reports version **3.8.2**, and links only Apple's `/usr/lib/libiconv.2.dylib` and `/usr/lib/libSystem.B.dylib`. Configure, build, and install logs remain in `.build/wine-build-probe/bison-build/`.

Wine's second configure run used the same flags and isolated paths shown above, adding `BISON="$probe/toolchain/bin/bison"`. It passed Bison and found both PE compilers: `i686-w64-mingw32-gcc` and `x86_64-w64-mingw32-gcc` each reported `works... yes`; the latter also reported SEH directive support. The next hard stop in `.build/wine-build-probe/build/configure.log` is:

```text
checking for ft2build.h... no
configure: error: FreeType 64-bit development files not found. Fonts will not be built.
Use the --without-freetype option if you really want this.
```

No Wine Makefile was generated on this second attempt. The existing `/opt/homebrew/lib/libfreetype.dylib` is ARM64-only and cannot satisfy an x86_64 Wine build. FreeType stayed enabled; disabling fonts would not meet the Caskrun runtime target.

## Project-local x86_64 FreeType and successful Wine configure

The [FreeType project download page](https://freetype.org/download) points to Savannah and SourceForge as its release mirrors. Downloaded [FreeType 2.14.3 source](https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz) and its [detached signature](https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz.sig) into ignored `.build/wine-build-probe/tool-sources/`. The source SHA-256 is `36bc4f1cc413335368ee656c42afca65c5a3987e8768cc28cf11ba775e785a5f`, matching the [release directory's published checksum](https://sourceforge.net/projects/freetype/files/freetype2/2.14.3/). The OpenPGP signature verifies mathematically with fingerprint `E30674707856409FF1948010BE6C3AAC63AD8E3F` (Werner Lemberg), obtained from Ubuntu's public key server. The release directory names its final 16 hex digits as the signing key. GnuPG still reports **unknown trust** for the key's identity; an independent binding remains a release-provenance gate.

FreeType configured with `CC='/usr/bin/clang -target x86_64-apple-macos13'`, `--build=x86_64-apple-darwin`, and `--prefix` inside `.build/wine-build-probe/deps-x86_64/`; `make -j8` and `make install` both completed without `sudo`. Its optional bzip2, libpng, HarfBuzz, and Brotli integrations were disabled for this diagnostic; it uses the macOS system zlib. The staged `libfreetype.6.dylib` and static archive contain **x86_64 only**. The dylib was ad-hoc signed and passed `codesign --verify --strict`; `otool -L` lists only its project-local install name, `/usr/lib/libz.1.dylib`, and `/usr/lib/libSystem.B.dylib`. The absolute install name inside `.build/` is not relocatable release packaging. A small self-authored, ad-hoc signed x86_64 program linked to this dylib and ran under Rosetta, reporting FreeType `2.14.3`. The test source and logs remain in the ignored build area.

Wine configure was run a third time with the same flags as above, `BISON="$probe/toolchain/bin/bison"`, and `PKG_CONFIG_LIBDIR="$probe/deps-x86_64/lib/pkgconfig"`. It **exited 0 and generated a Makefile**. The log confirms `ft2build.h... yes` and `-lfreetype... libfreetype.6.dylib`. The generated Makefile records `HOST_ARCH = x86_64`, `PE_ARCHS = i386 x86_64`, an explicit x86_64 clang target, FreeType include/library paths under `.build/`, and a `winemac.drv` build target. Neither the Makefile nor configure log contains `/opt/homebrew/lib` or `/opt/homebrew/include` paths.

Configure success leaves major feature gaps. Its final notices report **no GnuTLS/schannel**, **no SDL2**, **no Vulkan/MoltenVK**, and **no FFmpeg**, among other optional libraries. GStreamer was intentionally disabled in the command. These omissions are **release gaps to close**, not acceptable final omissions. They may affect Steam login, controllers, games, and media; the basic console smoke tests below do not establish their behavior. Pin and build required x86_64 runtime dependencies under the same project-local prefix, then repeat the build and tests. Do not publish this configuration as CrossOver parity.

## Diagnostic Wine build result

Ran `/usr/bin/make -j8` in `.build/wine-build-probe/build/`, redirecting all output to `make-diagnostic.log`. It **exited 0** after the log reported `Wine build complete.` The build output is about **3.2 GB** and remains ignored inside `.build/`. Make executed its own compile helpers (such as `winebuild`); Wine runtime programs were not launched during this compilation step.

Non-executing inspection found the build-tree `wine` loader and `server/wineserver` as x86_64 Mach-O files, `programs/wineboot/x86_64-windows/wineboot.exe` as x86-64 PE, both i386 and x86_64 PE `winemac.drv` modules, and an x86_64 Mach-O `winemac.so` linked to Apple's macOS frameworks. The loader's `otool -L` output lists only `/usr/lib/libSystem.B.dylib`; the build log contains no `/opt/homebrew/lib` or `/opt/homebrew/include` path. The build-tree loader was unsigned. The staged build and startup test below extend this evidence, but not to Windows-app compatibility.

## Staged install and startup smoke

Before installation, `/usr/bin/make -n install` completed and wrote `build/install-dry-run.log`. The generated Makefile's `prefix` was `.build/wine-build-probe/build/../stage`. All `tools/install` destinations inspected in the dry run, including `-t` directory targets, direct file targets, and symlink targets, were under that stage directory; no destination used `/Applications`, `/usr/local`, `/opt/homebrew`, `/opt/local`, or the user's Library. The stage was empty before this check. `/usr/bin/make install` then **exited 0**, with output in `build/install-local.log`; the staged tree is about **1.5 GB**. This was a project-local install, not a macOS system install.

`stage/bin/wine` and `stage/bin/wineserver` are x86_64 Mach-O executables. `stage/bin/wineboot` is Wine's symlink to `wine`; the actual Windows `wineboot.exe` is staged in Wine's PE tree. All **39** regular Mach-O files in `stage/bin` and `stage/lib/wine/x86_64-unix` were x86_64, ad-hoc signed locally, and passed `codesign --verify --strict`; the per-file results are in `build/stage-codesign.log`. Inspection with `otool -L` found only Apple system libraries/frameworks and Wine's `@rpath` modules among their direct Mach-O dependencies, with no Homebrew or external absolute dependency. This check does **not** cover libraries loaded later with `dlopen`. The local FreeType dylib was separately ad-hoc signed and checked. The staged `wine` entrypoint had no `com.apple.quarantine` attribute; no Gatekeeper setting or quarantine flag was changed. Ad-hoc signatures and these local paths are **not** a notarized, relocatable release.

Only these runtime probes were performed, via Python `subprocess.run` with 30- and 90-second timeouts respectively:

```sh
probe="$(git rev-parse --show-toplevel)/.build/wine-build-probe"
WINEPREFIX="$probe/smoke-prefix" "$probe/stage/bin/wine" --version
WINEPREFIX="$probe/smoke-prefix" \
  WINEDLLOVERRIDES='mscoree,mshtml=' \
  DYLD_LIBRARY_PATH="$probe/deps-x86_64/lib" \
  "$probe/stage/bin/wineboot" --init
```

`wine --version` **exited 0** and printed `wine-11.0` (`wine-version.stdout.log`, `wine-version.stderr.log`). The first fresh-prefix `wineboot --init` also exited 0 and created `system.reg`, `user.reg`, and `drive_c/windows`, but printed **13** `Wine cannot find the FreeType font library` messages because FreeType is loaded by name at runtime and was not on `dyld`'s search path. That disposable prefix was deleted after retaining its logs. A second fresh-prefix `wineboot --init` with only the project-local FreeType library in `DYLD_LIBRARY_PATH` **exited 0**, created the same prefix structure, and printed **zero** FreeType-missing messages. Its logs are `wineboot-init-local-freetype.stdout.log` and `.stderr.log`; the retained disposable prefix is `.build/wine-build-probe/smoke-prefix/`. `WINEDLLOVERRIDES` disabled Wine Mono/Gecko components for this diagnostic; no Windows installer was run.

This is a **startup smoke result**, not a clean or generally usable Windows-app result. The second `wineboot` stderr still reports Wine built without Vulkan, failure to open/start `RpcSs`, and Bluetooth/device/service startup errors. Their effects are not yet isolated. FreeType must be bundled and resolved without an absolute build path or developer-only `DYLD_LIBRARY_PATH`. The diagnostic build also lacks the GnuTLS, SDL2, Vulkan/MoltenVK, FFmpeg, and GStreamer capabilities noted above. The next sections test tiny self-authored Windows console programs and a one-file MSI; no Steam, game, controller, or graphics workload was tested. The next gate is to close these runtime and feature gaps, establish release-signing and dependency/licensing policies, produce a relocatable signed bundle, and then run broader app tests.

## 64-bit and 32-bit Windows console smoke

Wrote a small **self-authored** C console program at `.build/wine-build-probe/smoke-exe.c` (SHA-256 `8e11ae940ff226134325761c7733d897981d3a318bc2ddc70dafabcfc0f18cbe`). It prints an architecture-specific marker and returns deliberate exit code **37**; it returns 99 if the pointer width is wrong. Existing MinGW GCC 16.1.0 cross-compilers produced these local files with `-Os -Wall -Wextra -Werror`:

| Compiler | File | `file` result |
| --- | --- | --- |
| `x86_64-w64-mingw32-gcc` | `.build/wine-build-probe/smoke-exe-x86_64.exe` | `PE32+ executable (console) x86-64, for MS Windows` |
| `i686-w64-mingw32-gcc` | `.build/wine-build-probe/smoke-exe-i686.exe` | `PE32 executable (console) Intel 80386, for MS Windows` |

Ran them **sequentially** with staged `stage/bin/wine` inside the already initialized disposable `.build/wine-build-probe/smoke-prefix/`, setting `WINEARCH=win64`, the local FreeType `DYLD_LIBRARY_PATH`, and `WINEDLLOVERRIDES='mscoree,mshtml='`. Each Python `subprocess.run` had a **45-second timeout**. Results:

| Program | Exit | Captured stdout | Captured stderr |
| --- | ---: | --- | --- |
| x86_64 | 37 | `CASKRUN-WINE-SMOKE:x86_64:64` | Vulkan-not-built and Bluetooth service errors, keyboard fixmes |
| i686 | 37 | `CASKRUN-WINE-SMOKE:i686:32` | Vulkan-not-built and Bluetooth service errors, keyboard fixmes |

The exact stdout, stderr, and exit-code logs are `.build/wine-build-probe/smoke-exe-{x86_64,i686}.{stdout.log,stderr.log,exitcode}`. Both architecture-specific markers and the deliberate exit code reached the host process. This establishes **basic 64-bit and 32-bit console PE execution** in this Wine configuration under Rosetta. It does not establish GUI, Steam, networking/TLS, audio/video, controller, graphics, or real-world app compatibility. No downloaded installer or CrossOver artifact was run.

### Caskrun core integration smoke

The current core's `is_usable_wine` check accepts an executable `wine` with an executable sibling `wineboot`, both outside CrossOver paths. `stage/bin/wine` passes those file checks; `stage/bin/wineboot` is an executable symlink back to it, and `stage/bin/wineserver` is present. The core's version probe only runs `wine --version`, which succeeded without extra environment variables. Thus the staged path meets **selection and version-probe requirements** without a wrapper.

The core's bottle initializer sets `WINEPREFIX` and `WINEARCH`, but does not supply a FreeType search path or manage Wine Mono/Gecko. A first disposable Caskrun CLI initialization without Mono/Gecko overrides stalled past 80 seconds; its top-level Wine process was terminated and its prefix-specific wineserver was stopped. The cause was not conclusively isolated. With the diagnostic `DYLD_LIBRARY_PATH` and `WINEDLLOVERRIDES='mscoree,mshtml='`, a subsequent `wineboot --init` returned before `system.reg` appeared, exposing a race in Caskrun's immediate registry check. The core now bounds `wineboot` to 120 seconds, requests a prefix-specific wineserver shutdown on timeout, and waits up to 10 seconds for the registry after a successful exit.

With those developer-only environment variables and a **new** `.build/core-wine-smoke-final/` home, the rebuilt Caskrun core created a bottle, initialized its prefix in about 13 seconds, and found both `system.reg` and `drive_c/windows`. It then launched both self-authored console EXEs in that bottle and captured `CASKRUN-WINE-SMOKE:x86_64:64` and `CASKRUN-WINE-SMOKE:i686:32` in its own per-launch logs. The core reports a PID and log path but does not yet monitor the Windows process's final exit code; the direct Wine tests above establish exit code 37. The explicit `probe-runtime` CLI reported `wine-11.0` and an x86_64 Mach-O executable. No normal Caskrun UI install flow was exercised with this Wine build.

This proves a narrow end-to-end CLI path, **not** a self-contained app runtime. FreeType needs relocatable bundling, Mono/Gecko handling cannot rely on a diagnostic override, startup still logs service/graphics errors, and process completion must be reported before user installers are trustworthy. The Caskrun CLI/UI MSI flow was not exercised in this integration smoke; a direct self-authored MSI test follows. No Steam, downloaded installer, game, or CrossOver artifact was run.

## One-file MSI installation smoke

Using installed `wixl 0.106`, wrote `.build/wine-build-probe/msi-smoke/caskrun-msi-smoke.wxs` and a **self-authored** 37-byte `caskrun-msi-smoke.txt` payload entirely inside the ignored build area. The WiX-like XML declares one product, one feature, one component, and one file under `ProgramFilesFolder/CaskrunMSISmoke`; the payload is only `CASKRUN-MSI-SMOKE:project-local-only`. `wixl -v -o caskrun-msi-smoke.msi caskrun-msi-smoke.wxs` produced the local MSI. `msiinfo export` showed exactly one `File` row and an empty `CustomAction` table. `msiextract` reproduced the payload, and its SHA-256 matched the source: `2487bf13d3fe0ac390b814d8170ce0d5a0cea61652124a9d17fbe23808a31161`. The MSI's SHA-256 was `e09758a99f4b32ffb644d5594ecae45563de127078daf91577c9eaa8af805428`. No downloaded installer or third-party application payload was used.

A **fresh** disposable prefix `.build/wine-build-probe/msi-smoke-prefix/` was initialized with staged `wineboot --init`, `WINEARCH=win64`, the project-local FreeType `DYLD_LIBRARY_PATH`, and `WINEDLLOVERRIDES='mscoree,mshtml='`. It exited 0 in about 15 seconds and created `system.reg`. Then the staged Wine loader ran `msiexec /i <absolute-path-to-local-MSI> /qn /L*v C:\caskrun-msi-smoke-install.log` with the same environment and a **90-second timeout**. `msiexec` exited **0** in about 5 seconds. The installer log inside `drive_c/caskrun-msi-smoke-install.log` ends `Action ended ... INSTALL. Return value 1.` (MSI action success). Wine stderr still included Vulkan-not-built and Bluetooth-service messages.

The installed file exists at `.build/wine-build-probe/msi-smoke-prefix/drive_c/Program Files (x86)/CaskrunMSISmoke/caskrun-msi-smoke.txt`. Its SHA-256 equals the source payload's, and its resolved path remains inside the disposable prefix. Build, extraction-check, Wine stdout/stderr, exit code, and wineboot logs are under `.build/wine-build-probe/msi-smoke/`. This demonstrates a **single quiet MSI file install** in the isolated Wine prefix, not general installer compatibility, GUI setup, custom actions, uninstall, upgrade, or Steam support. No macOS application or global package was installed.

## Pass criteria for this milestone

1. Wine 11.0 configure completes with x86_64 macOS host and both requested PE architectures; logs show no ARM64-only linked libraries. **Met for this diagnostic configuration.**
2. A local build and install under `.build/` completes without touching `/Applications`, CrossOver, or global package prefixes. **Met for this diagnostic configuration.**
3. Locally signed Wine binaries pass macOS signature verification; `file`/`otool -L` show the intended x86_64 architecture and project-local or system direct dependencies. **Met for the staged Mach-O files; runtime-loaded FreeType still needs an explicit local search path.**
4. With `WINEPREFIX` under `.build/`, `wine --version` and `wineboot --init` exit successfully, followed by small x86_64 EXE, 32-bit EXE, and MSI tests. **Version, prefix creation, and self-authored x86_64/i686 console EXEs ran directly and through the Caskrun CLI with developer-only environment settings; a self-authored one-file MSI installed directly into a separate prefix. Startup still logged service errors, and the Caskrun UI installer flow has not been tested.** Record each separately before offering this runtime through the normal app setup.

Passing a compiler probe, configure, build, tiny console smoke, or one-file MSI is **not** a usable general Wine runtime. Steam and games remain unvalidated until actual installation, launch, update, and graphics/input tests pass on Apple Silicon.

## Follow-up: relocated development package and core MSI test

The original staged Wine and FreeType were copied into the [development runtime package](LOCAL_WINE_RUNTIME.md). Its `bin/wine` wrapper supplies a package-local FreeType path. A copy at another path still initialized a fresh prefix while the original stage and FreeType paths were temporarily unavailable. This removes the caller-set `DYLD_LIBRARY_PATH` requirement for these narrow tests, but not the diagnostic `WINEDLLOVERRIDES='mscoree,mshtml='` used to avoid Wine Mono/Gecko prompts.

With a fresh `CASKRUN_HOME`, the rebuilt ARM64 Caskrun core ran `launch-wait <bottle-id> <self-authored.msi> --quiet-msi --timeout-seconds 120` through that package. The direct Wine process exited **0** without timing out, and the installed file in the new bottle had SHA-256 `2487bf13d3fe0ac390b814d8170ce0d5a0cea61652124a9d17fbe23808a31161`, matching the original payload. The SwiftUI installer picker and real software have not been validated. `launch-wait` observes the direct Wine process only; a real installer may continue in child processes. This is a successful controlled MSI smoke test, not proof of general installation compatibility.

The later package added Wine 11.0-pinned Mono and Gecko, allowing fresh Caskrun prefixes and the same controlled MSI install **without caller DYLD or Mono/Gecko overrides**; see [local runtime notes](LOCAL_WINE_RUNTIME.md). A separate [Steam smoke test](STEAM_SMOKE.md) then completed installer and updater stages but found connection-manager certificate failures. Those results do not change the limits of the original staged-build checks above.

## Follow-up: isolated GnuTLS build and certificate test

The original configure log says `libgnutls 64-bit development files not found, no schannel support`; its `include/config.h` also leaves `HAVE_GNUTLS_CIPHER_INIT` undefined. Wine 11.0's [configure source](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/configure.ac) checks GnuTLS for both Schannel and bcrypt. The local Wine [crypt32 signature path](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/dlls/crypt32/cert.c) calls bcrypt for CNG certificate algorithms, so missing GnuTLS was a concrete candidate for Steam's `0x00000008` certificate status. Steam's closed-source logging field was not independently identified, so its numeric flag remains an inference from the Windows CryptoAPI status definition.

Official GMP 6.3.0, Nettle 3.10.2, and GnuTLS 3.8.13 sources were hash-checked and built as x86_64 libraries under ignored `.build/tls-probe/`. GMP and Nettle use portable C because GMP's x86 assembly produced illegal text relocations with this macOS linker. Their `make check` suites passed; GnuTLS's full suite was disabled, but an x86_64 Rosetta initialization smoke passed. A separate Wine 11.0 build in `.build/tls-wine-build/` configured with `--with-gnutls`, the project-local `.pc` files, and the earlier x86_64 FreeType. Its `config.h` defines both `HAVE_GNUTLS_CIPHER_INIT 1` and `SONAME_LIBGNUTLS "libgnutls.30.dylib"`; build and install finished under `.build/tls-wine-stage/`. This stage was packaged separately at `.build/tls-wine-runtime-dev/`, leaving the original Wine stage and package intact. [The package notes](LOCAL_WINE_RUNTIME.md#separate-gnutls-enabled-development-runtime) contain exact source hashes, signature observations, scripts, licenses, and checks.

A self-authored, standard named-curve P-256 certificate returned direct signature failure and chain status `0x28` in the original Wine package; the new package returned direct signature success and chain status `0x20`, with only the deliberately untrusted self-signed root remaining. A valid RSA-2048 control verified before the rebuild. In a cloned Steam bottle, the new package produced no earlier `Crypto API failed certificate check` lines and reached a WebSocket connection-manager connection, but Steam subsequently received a remote failure and still reported `LogonFailure No Connection`. These results establish a specific cryptographic improvement, not sign-in or general Steam compatibility.

As a read-only comparison requested during diagnosis, the installed CrossOver app's metadata lists x86_64 `libgnutls.30.dylib` and `libgmp.10.dylib` under its `lib64/`, and its x86_64 `bcrypt.so` has an rpath toward that directory. No CrossOver code, assets, or runtime files were copied or used in the independent build. The comparison supports the choice to package an x86_64 crypto backend but does not prove identical behavior or configuration.
