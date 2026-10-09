# Final manual demo checks

Use a disposable bottle first. Keep credentials inside publisher apps. Do not
publish raw logs or bottle backups; the structured diagnostic report excludes
personal paths and app names. Record observed stages separately.

## App and bottle controls

- [ ] Home folder single-click opens its contents and inspector; double-click
  opens the selected app. Closing the inspector or Escape returns to Home.
- [ ] Closing the main window leaves the Caskrun menu-bar item available and
  removes its Dock item; Open Caskrun returns the window.
- [ ] Full Quit closes Caskrun-owned Windows apps and leaves other Wine apps
  alone. Save work before testing Quit or Simulate Reboot.
- [ ] App Options rename, saved arguments and temporary arguments work across
  reopen. Check an argument containing spaces. When two saved commands use the
  same EXE, select each in Run an EXE and check its arguments/working folder.
- [ ] Installed Programs lists registered products with names/versions and
  refreshes after an installer or uninstaller finishes.
- [ ] Rebuild App Icons refuses a running bottle; after closing its apps, Home
  artwork and the next native Dock icon recover under the correct app name.
- [ ] Diagnostics opens, refreshes, copies and saves. Save Text exports only
  the displayed log text; it can contain private information.
- [ ] Duplicate, backup/restore and import preserve settings in a disposable
  bottle. Test a compressed `.caskrun-backup` and an older folder backup. A
  changed/corrupt archive must fail without adding a bottle.
- [ ] Bottle Notes saves across reopen and restore; Cancel warns about unsaved
  text. Windows version changes require stopped apps; Default restores the
  prior machine default while per-app overrides remain separate.
- [ ] CrossOver import is a separate copy workflow; it does not establish
  that imported apps or dependencies run.

## Publishers

For each publisher, record installer completion, usable client window,
sign-in, relaunch, correct Dock identity/focus and complete Quit separately.
Review agreements yourself and sign in inside the publisher's own client.

| Publisher | Starting evidence; what still needs checking |
| --- | --- |
| Steam | Usable sign-in and Dock focus were observed; some relaunches were black. Check repeated startup, sign-in, download and one game. |
| Epic Games Store | Its real MSI failed. Setup and a usable client remain unresolved. |
| GOG Galaxy | Setup completed; service/components and blank client remain unresolved. |
| EA app | The earlier agreement was closed without acceptance; setup/client acceptance remains pending. |
| itch.io | The bootstrap argument separator is fixed; usable versioned client remains unresolved. |
| Battle.net | Setup completed; usable client remains unresolved. |
| Ubisoft Connect | Setup completed; usable client remains unresolved. |
| Rockstar Games Launcher | Setup started but no completed-client result was obtained. |

See [launcher evidence](LAUNCHER_VALIDATION.md) for the detailed history. A
setup exit code or saved EXE alone is not a passing client result.

## One real game

- [ ] Note the title/version, store, macOS version, Windows architecture,
  renderer and display mode. Avoid account/device identifiers in shared notes.
- [ ] Install, reach visible play, test input and sound, exit, then relaunch
  from Caskrun. Record crashes, visual faults and performance separately.
- [ ] For a Direct3D 10/11 title, close the bottle's apps before selecting the
  experimental DXMT renderer. Other APIs need their own compatible backend.
- [ ] Compare High Resolution off/on only if useful for this title, and note
  readability and performance. It remains optional and off by default.

## Remaining implementation and external gates

This build is a private demo. It has no validated game profiles or broad
publisher compatibility. DXVK, D3DMetal, Direct3D 12, MSync, DLSS/MetalFX and
media support need separate runtime work and evidence. Dependency recipes and
repair, persistent Finder shortcuts,
file associations, storage migration and update delivery remain unfinished.
Developer ID signing/notarization, a separate-Mac clean-install check and the
public binary licence/distribution review are release gates.


## Discover, browser games and optional DXMT controls

- Switch between Stores and Browser games using Discover's compact content
  filter row. Confirm Home and Discover are the only main navigation entries.
- Open a bottle on Home, search its apps and installed Steam games, and confirm
  a game opens through that bottle's saved Steam client.
- Request a Steam game install in an existing Steam bottle, then in a new one.
  Steam must handle ownership, sign-in, agreements and download confirmation.
- Add a browser game from Discover or a public HTTPS link. Confirm Home opens
  one default-browser tab; removing the shortcut preserves browser game data.
- Check scene previews and title fallbacks at narrow and wide window sizes.
  A saved known game should reuse its picture on Home; a custom link must not
  cause an artwork request. Check own-file badges, collection Browse games and
  unverified Visit website labels. Website reachability is not a gameplay test.
- In an idle DXMT bottle, try the frame target and optional MetalFX spatial
  scales with a real game. Compare performance, image quality and UI scaling.
  Reset to application controlled / Off afterwards; existing defaults remain
  Auto/WineD3D and optional high-resolution mode.
- Check reduced motion, narrow window layout, keyboard focus, menu-bar reopen
  and full Quit with the game running. Full Quit must stop only Caskrun-owned
  Windows processes. Closing the main window keeps the menu-bar app available.
