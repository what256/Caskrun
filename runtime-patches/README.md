# Original Wine patches

The patches in this directory are Caskrun-authored changes to independently
downloaded upstream Wine. They are offered under Wine's LGPL-2.1-or-later
license so the complete modified Wine source can be redistributed under the
same terms. The rest of Caskrun keeps its own repository license.

Both source patches apply to **Wine 11.0 only**. When Caskrun opts a verified
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
