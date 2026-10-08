# Original Wine patches

The patches in this directory are Caskrun-authored changes to independently
downloaded upstream Wine. They are offered under Wine's LGPL-2.1-or-later
license so the complete modified Wine source can be redistributed under the
same terms. The rest of Caskrun keeps its own repository license.

The source patches apply to **Wine 11.0 only**. When Caskrun opts a verified
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

## `wine-11.0-winemac-metal-exports.patch` — prepared, not packaged

Wine builds its Unix libraries with hidden visibility. Measured against this
project's own Wine 11.0 build, `winemac.so` exports exactly two symbols, both
unix-call tables, out of 1905 symbol entries. The Metal and window-data entry
points an independently sourced Direct3D-to-Metal translator needs are present
in that library but have local binding, so a by-name lookup cannot find them.

The patch annotates eight declarations in `dlls/winemac.drv/macdrv.h` and
`macdrv_cocoa.h` with default visibility: `get_win_data`, `release_win_data`,
`macdrv_get_cocoa_window`, `macdrv_create_metal_device`,
`macdrv_release_metal_device`, `macdrv_view_create_metal_view`,
`macdrv_view_get_metal_layer` and `macdrv_view_release_metal_view`. It widens
linkage only; nothing is added, removed or renamed.

Rebuilding `winemac.so` from a clone of this project's configured Wine 11.0 build
tree with the patch applied raised its exported symbols from 2 to 10, exactly the
eight annotated declarations. The patch also applies cleanly to a pristine
Wine 11.0 source tree. The disposable clone was removed afterwards.

This patch is **not in any packaged engine**. Including it means rebuilding and
repackaging the whole runtime, which changes the runtime-tree digest existing
bottles are bound to, so it needs the general idle-bottle migration path the
graphics plan already requires. No translator has been built or loaded, and no
Direct3D call has been made through it. See the [graphics plan](../docs/GRAPHICS_PLAN.md).
