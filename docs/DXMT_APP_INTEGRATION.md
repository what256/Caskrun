# Optional DXMT in the development app

_Current source behavior updated 26 October 2026. The package and historical
validation sections retain the 8 October snapshot. Later user game results are
recorded in [launcher validation](LAUNCHER_VALIDATION.md)._

## Behavior

Collection’s bottle inspector → Display & Graphics offers **Automatic**,
**WineD3D**, **DXMT · Experimental**, and a package-gated **DXVK · Experimental**.
Automatic uses bounded static Direct3D 10/11 evidence to select the verified
DXMT view at an idle launch; complete unmixed x86/x64 Direct3D 9 evidence can
select the verified hybrid DXVK view. Mixed, unsupported or incomplete evidence
keeps WineD3D. The existing inspector layout is retained. See
[automatic graphics and stores](AUTOMATIC_GRAPHICS_AND_STORES.md) for the
selection rules and limits. Ordinary Discover installation still chooses the
included engine without a Wine picker. High Resolution remains separate,
optional and off by default.

The new candidate also supplies opt-in MSync, off by default, after coordinated
client/server API checks. Its version-3 graphics record preserves synchronization
alongside renderer and tuning. This does not change High Resolution defaults or
prove game performance. Current hybrid DXVK and MSync evidence, and the failed
private D3DMetal factory gate, are recorded in
[backend status](GRAPHICS_BACKEND_STATUS.md).

DXMT is selectable only with the exact reviewed package. A saved DXMT preference
on an unavailable engine blocks launch with a clear error rather than silently
using another renderer. Unknown preference versions, malformed files and unsafe
links are refused. The small versioned `graphics.json` preference is carried by
duplicate and Caskrun backup/restore, without carrying a runtime path.

Selection changes require the bottle lock, no tracked apps, an idle prefix's
Wine server and an independent open-file owner check. A refused switch preserves
the previous preference and leaves running apps alone. Atomic writes are read
back; a failed verification restores only Caskrun's own unchanged write.

Normal DXMT launches and read-only Steam reopen/Quit requests share the same
managed graphics-only DLL overrides and saved tuning. Control requests do not
repair prefixes or create shader-cache/log folders. Unrelated overrides and
the exact saved preference remain untouched. Diagnostics recognizes every
reviewed switchable successor rather than only the original DXMT package.

## Current candidate loader boundary

The coordinated candidate adds the fixed `components/dxvk/runtime/` view. Its
eight hybrid PE files are kept outside prefixes, and the dispatcher and direct
native aliases use the same selected view and fixed ICD. Secondary views' server
entries are relative links to the canonical common wineserver; an environment
variable alone is insufficient because Wine checks its adjacent server first.
Exact final package identity and app-build acceptance are separate from the
source/API evidence in this document.

## Historical package and loader — 8 October 2026

Build with the pinned inputs described in [DXMT smoke](DXMT_SMOKE.md):

```sh
python3 -B scripts/package-wine-runtime-dxmt-dev.py --switchable --check \
  --output .build/dxmt-wine-runtime-dev-v4
python3 -B scripts/package-wine-runtime-dxmt-dev.py --switchable \
  --output .build/dxmt-wine-runtime-dev-v4
CASKRUN_PACKAGE_RUNTIME_VARIANT=native-apps \
CASKRUN_PACKAGE_NATIVE_RUNTIME_NAME=dxmt-wine-runtime-dev-v4 \
CASKRUN_PACKAGE_OUTPUT_DIR="$PWD/.build/dxmt-app-candidate" \
  scripts/package-dmg.sh
```

Wine 11 searches its own builtin directory before `WINEDLLPATH`. Merely adding
a sidecar search path cannot reliably replace existing builtins. The package
therefore has two real runtime views:

| Choice | Inner runtime view |
| --- | --- |
| Automatic / WineD3D | `runtime/` |
| DXMT | `components/dxmt/runtime/` |

All 3,741 pre-existing entries in the default native-v2 runtime view are
unchanged. Dormant WineMetal bootstrap files are added; the DXMT view contains
the patched driver and both PE architectures. The common server bytes and
server-control path stay on the base view. The dispatcher accepts only these
fixed views and normalizes the native application's NTDLL path accordingly.
Native names, artwork, builtin tool containment and shutdown inode proofs use
the bottle's saved selection, never an inherited environment choice.

The complete package identity is
`sha256:4d82b50b96eb5995a78d848fb67166979577cd98e36d7db72fbb08636f8f44c5`.
The signed development app retains the same runtime identity. Its component
receipt records both views and twenty artifact hashes; source archives, LGPL
text and original patches accompany the ignored local package. Binary
distribution still requires its own publication and licensing review.

All Wine client paths receive the selected renderer: setup, ordinary/custom
apps, children, initialization, Windows tools, registry changes and reboot.
DXMT's graphics DLL overrides apply only to that launch environment; unrelated
overrides, application-local DLLs and registry entries are preserved. Its shader
cache and diagnostics stay in the bottle. Old prefixes receive only missing
`winemetal.dll` bootstrap placeholders while idle. Existing files are never
replaced; unsafe paths are refused, and a partial placeholder-copy failure
removes only the newly created files.

## Historical migration evidence and rollback boundary

Only the exact approved native-v2 package can advance to this exact successor,
while idle. This general transition does not require a Steam recipe. Custom
engines, legacy path-only records and unknown packages retain strict matching;
the older reviewed Steam transitions retain their original gates.

The old prefix record is backed up exclusively, the new record is installed
atomically and the ordinary runtime verifier checks it again. Failure at that
step restores the exact old record. Pre-Wine operation failures also restore
the record. This is **runtime-record rollback**, not a snapshot or rollback of
an application's files after Wine has run. A successful upgrade changes no
registry, profile, launcher, application DLL or user data. Imported prefixes
without a record still initialize normally.

## Historical DXMT evidence and remaining scope

The original [DXMT API/lifecycle experiment](DXMT_SMOKE.md) established hardware
feature levels 10_0/11_0, shader readback, HWND presentation, resizing,
recreation and destruction in x64 and x86 WoW64. The switchable package must
add a separate loader proof: Wine's logical module filename can still name the
prefix's baseline DLL even when the mapped file comes from the selected view.
The held-process probe therefore verifies exact live mapped files and their
receipt hashes on the same owned Windows process, alongside logical-file
diagnostics and the rendering gates.

The switchable v4 package passed this complete sequence in separate fresh x64
and x86 WoW64 win64 prefixes:

| Check | Result |
| --- | --- |
| Automatic → DXMT → WineD3D → Automatic | Exact receipt-backed mapped files at every step |
| DXMT feature levels 10_0 and 11_0 | Hardware devices, original clear/triangle readbacks and seven Present calls per level |
| Window lifecycle | Two windows, resize to 192×96, recreation on the same HWND, release after destruction and surviving-window rendering |
| Switch with the probe running | Refused; exact preference and runtime record unchanged; probe stayed alive |
| Source runtime preservation | All original default runtime entries and twenty receipt hashes verified |
| Cleanup | Prefix-scoped stop, independent zero owners, disposable storage removed |

WoW64 can detach its Windows child from the invoking Wine process. The test
discovers that child through an exact mapped owned fixture, checks its numeric
UID and process birth, then proves the same child's graphics mappings. A
merely opened DLL or stale recorded PID cannot satisfy this check. The Windows
logical DLL hashes remained the baseline hashes even during DXMT; they are
retained as diagnostics, not presented as selected-renderer evidence.

A separate live generic-bottle migration preserved all 1,956 prefix entries'
content, mode, inode and modification time. The old runtime record backup was
byte-identical; neutral app settings, an app-local custom D3D DLL and the
graphics preference stayed unchanged. Repeating initialize made no new
upgrade and retained the original backup. This test used no Steam recipe.

A second real migrated prefix initially lacked both WineMetal placeholders.
Selecting DXMT added only those missing files, then passed the original x64
hardware/device, offscreen/HWND readback and four-Present basic gates at both
feature levels. Existing prefix graphics DLLs, the app-local DLL and settings
were preserved. Both migration sessions stopped and passed independent
zero-owner checks before removal.

Source verification passed 294 Rust unit tests and two integration tests (two
existing tests remain ignored), plus 97 Python checks. Both original probe
architectures compile with strict warnings; Swift typecheck, Rust formatting,
package architecture/linkage audits, app signatures and DMG verification pass.

The full switching/lifecycle sequence also passed again with the final signed
app's actual core, bundled runtime and native artwork helper in both
architectures. Both packaged test prefixes stopped, independent zero-owner
checks passed and their disposable storage was removed.

Game play, visible screen-pixel composition, game performance, authentication,
other launchers, D3D12, NVAPI, DLSS, DXVK, D3DMetal and MSync were outside that
historical DXMT validation. The 26 October hybrid DXVK/MSync probes add only their
explicit API scope; no new game was launched. Later user-led game checks are
recorded in [launcher validation](LAUNCHER_VALIDATION.md).
Source publication is authorized; binary distribution still requires its separate privacy and runtime-license review.
