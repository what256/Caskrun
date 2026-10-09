# Final manual demo checks

Use a disposable bottle first. Keep credentials inside publisher apps. Do not
publish raw logs or bottle backups; the structured diagnostic report excludes
personal paths and app names. Record observed stages separately.

## App and bottle controls

- [ ] Collection shows games/apps directly. A click opens the details panel;
  double-click opens that item. Closing details or Escape keeps the flat grid.
- [ ] Its single filter menu switches All/Windows/PS5/Browser and Games/Apps.
  Search and filter changes clear details for hidden items. + → Manage bottles
  remains reachable with no Windows bottles or when another platform is selected.
- [ ] Closing the main window leaves the Caskrun menu-bar item available and
  removes its Dock item; Open Caskrun returns the window.
- [ ] Full Quit closes Caskrun-owned Windows apps and PS5 sessions and leaves
  independent Wine/engine apps alone. Browser tabs remain in the browser. Save work before testing Quit or Simulate Reboot.
- [ ] App Options rename, saved arguments and temporary arguments work across
  reopen. Check an argument containing spaces. When two saved commands use the
  same EXE, select each in Run an EXE and check its arguments/working folder.
- [ ] Installed Programs lists registered products with names/versions and
  refreshes after an installer or uninstaller finishes.
- [ ] Rebuild App Icons refuses a running bottle; after closing its apps, Collection
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
  filter row. Confirm Collection and Discover are the only main navigation entries.
- Search Collection’s saved apps and installed Steam/Epic/GOG games. Confirm
  Steam opens through its saved client, while Epic/GOG cards say Open store.
  Bottle controls must stay in details without replacing the flat grid.
- Request a Steam game install in an existing Steam bottle, then in a new one.
  Steam must handle ownership, sign-in, agreements and download confirmation.
- Add a browser game from Discover or a public HTTPS link. Select the saved game and double-click or use Play in browser to open
  one default-browser tab; removing the shortcut preserves browser game data.
- Check game pictures and neutral loading/error states at narrow and wide window sizes.
  A saved known game should reuse its picture on Collection; a custom link must not
  cause an artwork request. Check own-file badges, collection Browse games and
  unverified Visit website labels. Website reachability is not a gameplay test.
- In an idle DXMT bottle, try the frame target and optional MetalFX spatial
  scales with a real game. Compare performance, image quality and UI scaling.
  Reset to application controlled / Off afterwards; existing defaults remain
  Auto/WineD3D and optional high-resolution mode.
- Check reduced motion, narrow window layout, keyboard focus, menu-bar reopen
  and full Quit with the game running. Full Quit must stop only Caskrun-owned
  Windows processes. Closing the main window keeps the menu-bar app available.

## Optional experimental PS5 adapter

- Import a game folder with `eboot.bin`, ELF or ZAR file that you own. Confirm
  its original files stay in place, local artwork appears when available and
  a missing engine prevents Open. Import from a Windows/Apps filter too.
- Explicitly select a trusted complete KytyPS5 Mac folder or app containing its
  engine and matching local MoltenVK library. An x86-64 engine needs Rosetta 2. Record the engine
  version yourself; attaching a file does not prove compatibility.
- With a supported user-owned title, test visible rendering, audio, controllers,
  saves, relaunch, performance and native Dock/window focus separately.
- Verify repeated Open does not add another engine session. Stop and full Quit
  should stop only the engine Caskrun started and leave an independent session
  untouched. Engines that refuse to exit should leave Caskrun open with an error.
- Remove an import after stopping it: only the Collection reference is removed.
  Original game files and engine files must survive. Local PS5 metadata/logs
  are separate from bottle backups and should not be published.

No real PS5 engine or game was executed by the automated fixture checks. AnyPS5
conversion, managed engine downloads and broad compatibility remain unfinished.
