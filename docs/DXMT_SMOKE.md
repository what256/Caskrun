# Isolated DXMT development candidate

_8 October 2026. This is an original development experiment, separate from the
included app runtime and existing user bottles. Gameplay and visible screen
composition are not validated._

## Defect and change

The first independently built candidate loaded the expected DXMT DLLs and
passed hardware-device creation, clear readback and an original shader triangle
at feature level 10_0. Creating its HWND swap chain then aborted. Exporting
Wine's five existing translator entry points and correcting the private window
structure offset were insufficient: Wine 11 creates its client view lazily
when a managed graphics surface is attached. DXMT had not created that surface.

The paired original patches now use a versioned opaque Wine client-surface
API. Wine creates and owns the surface and Metal view, handles window updates
and destruction, and releases them through the existing reference-counted
lifecycle. DXMT no longer reads Wine's private window structure. It prepares
the surface immediately before queued presentation; an appended Unix call
preserves the existing x64 and WoW64 dispatch ordinals.

Patch order:

1. Wine: `wine-11.0-winemac-metal-exports.patch`, then
   `wine-11.0-dxmt-client-surface.patch`.
2. DXMT: `dxmt-e94c312f-wine-11-win-data.patch`, then
   `dxmt-e94c312f-wine-11-client-surface.patch`. The second supersedes the
   private-layout integration while retaining a reproducible patch sequence.

The Wine driver exports ten symbols: two existing Unix-call tables, the five
existing translator functions and three versioned surface functions. The
ordinary native-v2 package remains byte-identical.

## Reproduce with staged development inputs

All paths below are relative to the repository. Build products, source
checkouts, toolchains, caches and raw diagnostics stay under ignored `.build/`.
The resume script does not download tools, install Apple components or rebuild
LLVM. Stage the pinned upstream sources and the independent Wine 11 build first,
following the existing Wine build scripts and DXMT's upstream development guide.

Required staged inputs:

- DXMT revision `e94c312f5c054263acf261cfa109edf13e757587` with the two patches.
- LLVM 15.0.7 x86_64 headers/static libraries and its pinned source checkout.
- Meson 1.11.2, Ninja 1.13.2, both MinGW Windows cross-compilers, Xcode and its
  working Metal compiler component.
- The independently built native-v2 runtime and a separate patched Wine 11
  build. The resume script checks exact source/patch pins, compiler flags,
  architectures and exports before building.

```sh
scripts/build-dxmt-dev.sh --check
scripts/build-dxmt-dev.sh
python3 scripts/package-wine-runtime-dxmt-dev.py --check \
  --output .build/dxmt-wine-runtime-dev-v3
python3 scripts/package-wine-runtime-dxmt-dev.py \
  --output .build/dxmt-wine-runtime-dev-v3
```

The packager refuses existing outputs, unreviewed source changes, unexpected
architectures, external links and dependencies. It clones the exact approved
base, adds both Windows DLL architectures and the x86_64 Unix translator,
normalizes dependencies, signs and audits native images, preserves pinned
source archives/notices and records component hashes in `components.json`.
The receipt describes built components; it deliberately makes no game claim.
This local candidate is not approved for binary distribution.

Compile the original probe for both Windows architectures:

```sh
mkdir -p .build/dxmt-smoke-probe
x86_64-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror \
  scripts/probes/dxmt-smoke.c -o .build/dxmt-smoke-probe/dxmt-smoke-x64.exe \
  -ladvapi32 -ldxguid -luser32
i686-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror \
  scripts/probes/dxmt-smoke.c -o .build/dxmt-smoke-probe/dxmt-smoke-x86.exe \
  -ladvapi32 -ldxguid -luser32
```

Use a current project-local core under `.build/`. The example core is the
separate demo candidate, whose core source matches this experiment's baseline:

```sh
python3 -B scripts/test-dxmt-runtime.py run \
  --core .build/demo-candidate/Caskrun.app/Contents/MacOS/caskrun-core \
  --runtime .build/dxmt-wine-runtime-dev-v3 \
  --probe-x64 .build/dxmt-smoke-probe/dxmt-smoke-x64.exe \
  --probe-x86 .build/dxmt-smoke-probe/dxmt-smoke-x86.exe
```

Repeat that command with `--lifecycle` for the window-lifecycle gate. Each run
uses two separate fresh Caskrun win64 prefixes, including the x86 WoW64 path,
and verifies their core runtime pins. Explicit builtin DLL overrides select
the package's translator; loaded DLL hashes must match its receipt. All HOME,
XDG, temporary and shader-cache paths are owned by the disposable session.

The runner requires complete stage records and recomputes pixel checks. A
reported pass without the required device, shader, readback and presentation
evidence is refused. A process-only watchdog and bounded core wait limit hangs.
Success removes the fixture after prefix-scoped shutdown and independent
process/open-file checks. Failure preserves local evidence, reports whether shutdown was verified and
prints an explicit owned-session cleanup command. Repeated cleanup is safe: an idle
Wine server returns 1 for `-k`, so idleness is checked with `-w` before signalling.

## Recorded outcomes

The tested runtime tree digest is
`sha256:e9c93ae63f8c28929f65d29f4d31b89eeafb00d977e1ac096c4f460008c38daa`;
its component-receipt SHA-256 is
`87636b0889e85087c4d98f0e28c361100ce71abb317c15a9f979691c6e13af36`.
These identify local development artifacts, not user bottles.

The final separate candidate passed the lifecycle gate for x64 and x86 WoW64,
using hardware feature levels 10_0 and 11_0 in each architecture:

| Check | Result |
| --- | --- |
| Candidate DLL identity | Loaded `d3d11.dll` and `dxgi.dll` hashes match the package receipt |
| Two HWNDs and alternating rendering | Passed with seven checked Present calls per feature level |
| Resize and readback | Passed at 192×96; expected clear and triangle pixels match |
| Same-window swap-chain recreation | Passed |
| Destroy window before releasing swap chain | Passed |
| Surviving second window | Continued rendering and presentation passed |
| Cleanup | Both owned prefixes stopped; independent zero-owner checks passed; fixture removed |

The preceding candidate also passed the basic offscreen/HWND gate in both
architectures. Its old cleanup code incorrectly treated an already-idle
`wineserver -k` exit status as a shutdown failure. Prefix-only `-w` checks proved
idleness; the corrected idempotent cleanup then removed those fixtures. The
final candidate also passed the same basic gate separately, with both fixtures
removed after verified shutdown.

Source checks passed: 272 Rust unit tests and two integration tests (two other
Rust tests remain ignored), plus 74 Python checks including the new evidence,
component-packaging and cleanup cases. Both original Windows probes compile
with strict warnings. Package audits verify native architectures, relative
linkage and signatures; the approved base tree digest stays unchanged.

## Acceptance boundaries

The basic gate requires hardware feature levels 10_0 and 11_0, offscreen and
HWND clear/triangle readback, and four `S_OK` Present calls per level. The
lifecycle gate adds two simultaneous windows, alternating presentation,
192×96 resize/readback, swap-chain recreation on the same HWND, window
destruction before swap-chain release and continued rendering in the survivor.

Readback and successful Present calls establish graphics/API interoperability.
They do not independently measure visible screen pixels, game performance,
controllers, media playback or publisher-client compatibility. D3D12, NVAPI
and NVNGX remain disabled. The app still uses its current WineD3D runtime;
there is no user-bottle migration or selectable DXMT control in this phase.
High Resolution remains optional and off by default.

Before promotion: verify visible output and a real game; add exact runtime
capability reporting and an idle-bottle migration/rollback path; then integrate
a supported selection in the app. Keep source and binary publication reviews
separate from local development validation.
