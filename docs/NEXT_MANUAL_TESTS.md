# Next manual checks

The completion pass leaves store and game launches to the user. Source and
fixture checks do not replace these checks. Keep raw logs and bottle backups
private; use the allowlisted diagnostic report for sharing.

## Graphics and saved choices

1. Quit the bottle's apps using its bottle control. Closing a window can leave
   them running. Keep any existing working manual renderer unless testing a
   separate bottle.
2. In a disposable bottle with a supported x64 DirectX 10/11 game, select
   Automatic and open its app. After its next idle launch, inspect the displayed
   renderer and explanation. Unknown/mixed requirements should keep WineD3D.
3. When Automatic chose DXMT, quit the bottle's apps. Change Frame target and
   MetalFX spatial, reopen details and confirm both saved. Automatic must stay
   selected. Reset tuning; it must still stay Automatic. Repeat with manual
   DXMT. A running bottle must refuse changes without stopping the game.
4. Play, exit, close/reopen the app window and relaunch. Check image quality,
   sound, microphone if used, controller input and saves. Frame pacing and
   spatial scaling do not guarantee higher FPS; compare in the same scene.
   High Resolution is independent and can affect DPI-aware apps differently.

## Steam and other stores

- Steam: observe Starting/Updating feedback, reach its main window, close that
  window and reopen from the Dock. Check one correct Dock identity and native
  copy/paste. Red-close of Caskrun must preserve the session; full Quit must
  stop only its owned apps. Save work before testing full Quit.
- EA: after a normal client update, Open should retain Starting/Waiting
  feedback even when its client moved into a numbered folder. An installed
  game's Play request must stay in the selected bottle. Review any agreements
  and sign in yourself.
- Epic, GOG, Ubisoft, Battle.net, itch.io and Rockstar: check installer
  completion, usable client, sign-in, game download, play, window reopen and
  quit separately. A saved EXE or a submitted request does not prove success.
  Record the first failing stage and any exact error.

## Engine limits

DXVK, D3DMetal, MSync, temporal scaling and frame generation are not enabled by
this build. Their engine work is listed in [backend status](GRAPHICS_BACKEND_STATUS.md).
Do not expect those names in the informational list to act as switches.

Earlier API results and the narrow R.E.P.O. user result remain recorded in
[launcher validation](LAUNCHER_VALIDATION.md). This checklist does not claim
new game compatibility or a completed public binary release.
