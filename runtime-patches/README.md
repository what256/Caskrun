# Wine, DXMT and experimental AnyPS5 patches

`wine-*.patch` and `dxmt-*.patch` are Caskrun-authored changes to independently
downloaded upstream Wine and DXMT. They are offered under their respective
projects' LGPL-2.1-or-later license; the complete [LGPL 2.1 text](licenses/COPYING.LIB)
is included unchanged from Wine 11.0. Preserve the copyright and license
notices in the upstream source when applying or redistributing these changes.

The [experimental AnyPS5 subdirectory](anyps5/README.md) is separately
GPL-2.0-only and includes its own COPYING file. Original MIT-licensed helpers
and plist templates retain Caskrun's repository license. These source patches
do not include third-party runtimes, publisher artwork or game data.

## Wine patches

The Wine source patches apply to **Wine 11.0 only**. When Caskrun opts a verified
Steam launch into `CASKRUN_STEAM_MODE=1`, `wine-11.0-steam-cef-commandline.patch`
appends `--in-process-gpu --disable-gpu` to `steamwebhelper.exe` children
started by `steam.exe`. It does not alter Steam's binaries or disable the CEF
sandbox. `wine-11.0-steam-dock.patch` selects a Caskrun-built native loader
named `Steam` for Steam and its web helper, keeps Steam's background process
out of the Dock, and presents the helper's visible sign-in as the one Steam
Dock item. The signed alias embeds the original `steam_info.plist.in`
template. Its icon is loaded from Caskrun's extraction of the user's own
Steam.exe icon through `CASKRUN_STEAM_ICON`; no icon asset is copied here.

The isolated tests are recorded in ignored `.build/steam-cef-flags-probe/`
and `.build/steam-dock-probe/`. A combined disposable Steam prototype rendered
a usable sign-in and a single Steam Dock item whose click brought that window
forward. The final tracked Dock patch has one fewer environment guard and must
be retested in the complete source-built runtime. This is a focused development
validation, not a general release or game-compatibility claim.

Run `./scripts/build-wine-steam-dev.sh --check` to verify the source archive
and patch context. The full command builds into a new ignored `.build/`
directory and refuses to replace any existing output. The stage carries the
exact Wine 11.0 source archive, both patches, plist template, Wine license,
and a source manifest. A later runtime packager must preserve these and the
GnuTLS/FreeType/Mono/Gecko notices. Add further changes only after independent
validation, by updating the script's tracked patch allowlist, pinned digests,
output name, and source manifest. Do not use CrossOver code or runtime files.

## Per-program native identity

`wine-11.0-native-app-identity.patch` applies after both Steam patches. It
preserves their Steam-specific CEF flags and Dock selection. Other opted-in
Windows images receive their own generated native bundle through a bounded,
no-shell `prepare-native-app <bottle-id> <resolved-unix-image>` core callback.
Each child refreshes its image from Wine's resolved process path. The callback
replaces `CASKRUN_HOME` with canonical `CASKRUN_NATIVE_DATA`, excludes inherited
Wine loader/socket variables and file descriptors, and accepts only one owned,
canonical native executable below that data home. Failure or a two-second
callback deadline falls back to the neutral `WindowsApp` loader.

The launch environment must supply `CASKRUN_NATIVE_CORE`,
`CASKRUN_NATIVE_BOTTLE`, `CASKRUN_NATIVE_DATA`, `CASKRUN_NATIVE_IMAGE`, and
`CASKRUN_NATIVE_NTDLL` from the independently verified runtime and owned bottle.
These variables are a trusted launcher opt-in, not a sandbox for malicious
Windows programs. The core callback must not acquire the installer operation's
bottle lock. The generated bundle uses `Contents/MacOS/WindowsApp`; its own
Info.plist supplies its display name and `Resources/app.icns` (or app.ico).
`native_app_info.plist.in` is the neutral embedded loader metadata.

The driver keeps known Wine background services and explicit Chromium helper
subprocess roles accessory, including same-name Electron processes. Both
`--type=value` and `--type value` are recognized; a browser or unknown final
role remains eligible for the foreground. It leaves ordinary Windows programs, games, Wine
configuration tools, and browser/main helper processes eligible for their own
Dock items. Four changed Wine sources compile with Wine 11.0's build flags.
Fourteen isolated callback regressions pass, including timeout reaping,
non-shell argv, environment/descriptor isolation, invalid identifiers,
malformed responses, and linked or out-of-home loader paths. A separate
Foundation probe confirms the generated bundle's name overrides neutral
embedded metadata. These checks do not establish game or launcher compatibility.

`native_icon.swift` is an original MIT-licensed offscreen ARM64 helper. The current
app packages it as a signed `caskrun-native-icon` sibling of the core and shares
its composition code with the UI. A guarded app-helper selection keeps the
included Windows runtime byte-identical, so icon polish alone does not migrate
existing bottles. Older engine packages retain their recorded renderer source
and notices. Wine patches retain their upstream licences.
It accepts an original ICO or Caskrun's own fallback PNG and writes a new
1024-pixel PNG plus ICNS with native and Retina PNG representations. It retains
publisher colours and aspect ratio through adaptive alpha/background composition;
finished tiles and circles retain their own shape, while sparse low-contrast
marks can receive a restrained contrasting plate. See [icon policy](../docs/ICON_COMPOSITION.md).
ImageIO validates at most 64 representations, each at most 4096 pixels per
side, before decoding only a 1024-pixel thumbnail. Input and output are bounded
to 16 MiB; anchored no-follow directory/file operations reject symbolic links
and preserve pre-existing outputs. No native application or window is created.
Twenty-three real CLI synthetic-image cases pass for composition, native/Retina
representations, decode limits and safe failures. The slowest successful
conversion measured 0.365 seconds against the 1.1-second callback deadline;
this is a sample, not a universal timing guarantee.
Eighty-five isolated Cocoa role checks confirm ordinary apps/tools remain
eligible for foreground use while the narrowly identified background roles
are accessory. Full packaged-runtime and vendor-launcher checks remain separate.

## Native editing and Steam sign-in controls

`wine-11.0-native-text-editing.patch` follows the identity patch. It forwards
native Edit commands and Command shortcuts as balanced Windows editing keys.
Within the unchanged active Steam/Wine Cocoa I-beam scope, bounded asynchronous
queries retire a verified standard Windows menu or one directly owned captured
`Chrome_WidgetWin_1` popup. The latter requires the exact class, popup shape,
the tool-window and no-activate extended styles, absence of the transparent
style, original GUI thread/root/focus, current capture and request serial.
Chromium toggles the layered style while its menu is shown, so that bit is
observed but not required; requiring it previously rejected Steam's own menu. Longer class-name prefixes, changed ownership, stale input and
unknown popups are refused. After a final identity/visibility recheck, it posts
one `WM_CANCELMODE` to the captured popup without a synchronous application
callback. Success only requests cancellation: later queries carry that exact
popup and original focus, then require it hidden or destroyed with capture
released before showing the native menu. No clipboard or field values are read.
Foreign Win32 cursor handles do not establish the actual Cocoa cursor shape;
the source retains its original driver-local I-beam guard. In the real client
the first right-click, and repeated dismiss/reopen, each produced one native
editing menu with no Chromium popup behind it. Dock hide/reopen cycles and a
full clipboard round-trip ending in Paste remain unvalidated.

`wine-11.0-steam-native-window.patch` follows the editing patch. Its closed
process/title/size policy adds native AppKit controls over the empty top strip
of Steam's fixed English sign-in window. Close hides that UI without a Windows
close request; minimize follows the existing Wine path. Other windows and
explicit Quit retain their behavior. The client geometry is unchanged.

`wine-11.0-steam-native-bundle.patch` follows the window patch. Steam's visible
browser helper reuses the per-bottle native app bundle Core prepares before launch;
the primary process remains accessory and games keep their own identities.
The helper's normal drive-mapping path is resolved and both forms must remain
inside its exact private prefix. The bundle, Core and selected native loader
retain their canonical ownership checks. A refused binding uses the existing
raw Steam loader without a child Core callback.

Five explicit Chromium background roles retain the raw loader and an accessory
policy before Dock promotion. Type-switch parsing matches the Cocoa policy,
including both forms, overrides and the positional separator. The visible
browser retains its independent bundle identity.

`scripts/build-wine-native-text-dev.py` pins all three patches and rebuilds
the base and DXMT macOS drivers, their x86/x64 Windows halves and both native
NTDLL child loaders in isolated clones. Export sets stay unchanged. The
packaged source records carry the corresponding patches. See
[native editing validation](../docs/NATIVE_TEXT_EDITING.md) for the exact runtime
identity, tests and limits; these changes do not establish gameplay support.

The complete recognized-popup package is pinned to
`sha256:fb91add5bbcdddef39738259595c2da592bed8afb863fccd6384ae6b1337a5ee`.
Ten direct predecessor edges, including the captured-popup package
`sha256:46998fc3cf95d882e14c3b5978dba2ee086c5b92e826400fda528517f3808082`,
retain all 36 historical transitions and backup names. TLS/Steam sources keep recipe/PE gates; later reviewed native packages
use locked, idle-only record migration and retain rollback. Query-state layout
is checked for Unix64/PE32/PE64 and existing event/query ordinals stay fixed.

Earlier audio-device probes stalled, but normal audio-enabled Steam startup
later succeeded after unlock and inspection of already-enabled permissions.
A repeated capture probe completed in about 6 ms after an earlier 5-second
limit. This is a transient or context-dependent observation, not proof of a
persistent microphone failure. No audio bypass, permission change or saved
audio setting is included in these patches.

## DXMT development candidate — separate from the included runtime

Four patches now build an isolated, signed runtime candidate with both Windows
architectures. Hardware Direct3D device, shader, readback and HWND Present gates
pass in fresh Caskrun bottles. The current app runtime and existing bottles are
unchanged. See [DXMT smoke](../docs/DXMT_SMOKE.md) for reproducible commands,
acceptance boundaries and remaining game/app-integration work.

### `wine-11.0-winemac-metal-exports.patch`

Wine builds its Unix libraries with hidden visibility. This project's Wine 11.0
build exports exactly two symbols from `winemac.so`, both unix-call tables, out
of 1905 symbol entries.

Audited against DXMT revision `e94c312f5c054263acf261cfa109edf13e757587`, its
unix layer first looks for a `macdrv_functions` structure, which is a CrossOver
export absent from upstream Wine, and otherwise resolves exactly **five**
functions by name: `get_win_data`, `release_win_data`,
`macdrv_view_create_metal_view`, `macdrv_view_get_metal_layer` and
`macdrv_view_release_metal_view`. Other names in that structure, including
`macdrv_create_metal_device` and `macdrv_get_cocoa_window`, are never looked up
on the upstream path and are deliberately left local.

The patch annotates those five declarations with default visibility. It widens
linkage only. Rebuilding `winemac.so` from a clone of this project's configured
Wine 11.0 build tree raised its exports from 2 to 7, the five plus the two
existing tables, and left the three unused names local. It also applies cleanly
to a pristine Wine 11.0 tree. The disposable clone was removed afterwards.

### `dxmt-e94c312f-wine-11-win-data.patch`

DXMT declares its own copy of `struct macdrv_win_data` with four pointer fields
and reads the client view from offset 24. Wine 11.0 has no separate
`cocoa_view`: its client view is `client_view` at offset 16, and offset 24 begins
a `struct window_rects`. Reading the older layout against this driver therefore
passes window coordinates to `macdrv_view_create_metal_view` as a view pointer.

The patch declares only the fields that file reads, with Wine 11.0's names and
order. It targets a build against upstream Wine 11.0 and does not preserve the
older layout the `macdrv_functions` path expects, which upstream Wine never
exports.

### Managed Wine 11 client surfaces

`wine-11.0-dxmt-client-surface.patch` follows the export patch. It adds versioned
opaque create/prepare/release APIs using Wine's managed client surfaces, with
safe argument/allocation failures and the existing ownership/destruction path.
It exports three more functions; the candidate driver has ten exports total.

`dxmt-e94c312f-wine-11-client-surface.patch` follows the layout patch and
supersedes private-structure access entirely. DXMT retains only an opaque
surface handle and its borrowed Metal layer. Appended Unix call 146 preserves
all existing x64/WoW64 ordinals. Preparation follows committed presentation
encoding order; release follows DXMT's existing GPU-idle lifecycle.

The build and packager pin all modified DXMT files and the ordered patches.
No code or runtime is extracted from an installed vendor app. All four patches
are included in the separate candidate's corresponding-source records.

### Provenance and licence of the pinned revision

DXMT is published at `3Shain/dxmt` under LGPL-2.1-or-later. The pinned revision
is `e94c312f5c054263acf261cfa109edf13e757587`, committed 6 October 2026, tree
`b3cc8aaef14ec492d1bcf62acf29196efde90024`; the separately reviewed tagged release is v0.80
of 23 April 2026. Its `LICENSE` file at that revision is 782 bytes, git blob
`927beb2e391ca7188b15fa7fbd9599e83e84bd7a`, SHA-256
`b87c35aef7b2cf14de854118ca55ce5c4b284c85b5f002421fb8d46d868c2d17`, and reads
"Copyright (c) 2023-2026 Feifan He for CodeWeavers". That file is a grant notice
only: it does not contain the LGPL text, so the local candidate includes a full
LGPL-2.1 copy alongside it, corresponding-source archives and these patches.

Building from that public, separately licensed repository is distinct from
copying anything out of an installed CrossOver, which remains forbidden. This is
an engineering and documentation judgement, not legal advice.
