# Next manual checks

The 26 October engine pass leaves store and game launches to the user. Source and
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
4. In a separate DirectX 9 test bottle, check Automatic's explanation at an
   idle launch. Complete unmixed imports may choose verified hybrid DXVK;
   unknown/mixed evidence should keep WineD3D. Also test an explicit DXVK choice
   and reopening the inspector. Keep D3D8/12 titles outside this DXVK check.
5. With all bottle apps stopped, enable MSync on a supported engine. Confirm it
   stays saved after closing/reopening details and restarting Caskrun; changing
   the renderer or DXMT tuning must preserve it. A running bottle must refuse
   a mode change without stopping the game. Repeat with MSync off. Compare the
   same game/scene before drawing any performance conclusion; off is default.
6. Play, exit, close/reopen the app window and relaunch. Check image quality,
   sound, microphone if used, controller input and saves. Frame pacing and
   spatial scaling do not guarantee higher FPS; compare in the same scene.
   High Resolution is independent and can affect DPI-aware apps differently.

## Steam and other stores

- Steam: observe Starting/Updating feedback, reach its main window, close that
  window and reopen from the Dock. Check one correct Dock identity and native
  copy/paste. Red-close of Caskrun must preserve the session; full Quit must
  stop only its owned apps. Save work before testing full Quit.
- Repeat Steam's window/Dock/Quit cycle with each renderer being tested, and
  with MSync off/on only after a full bottle stop. DXMT reopen must retain its
  selected renderer and saved tuning; a submitted show request alone does not
  prove the window became usable.
- EA: after a normal client update, Open should retain Starting/Waiting
  feedback even when its client moved into a numbered folder. An installed
  game's Play request must stay in the selected bottle. Review any agreements
  and sign in yourself.
- Epic, GOG, Ubisoft, Battle.net, itch.io and Rockstar: check installer
  completion, usable client, sign-in, game download, play, window reopen and
  quit separately. A saved EXE or a submitted request does not prove success.
  Record the first failing stage and any exact error.

## Engine limits

DXVK and MSync controls require their exact reviewed engine; source or probe
results alone do not make them available in an older installed app. D3DMetal
remains unavailable in Wine after its private factory probe failed, despite
the separate native framework control passing. An isolated DXMT D3D12
path is now in the 0.2.0 app's V16 engine under manual DXMT. Its actual Core
shader/presentation/migration tests pass; test games in a separate bottle and
observe the FL11_1/SM5_1/no-DXIL limits. Automatic does not select D3D12.
Temporal scaling and frame generation remain unimplemented. See the
[D3D12 experiment](D3D12_EXPERIMENT.md).
See [backend status](GRAPHICS_BACKEND_STATUS.md). MSync's unsupported mixed wait
objects and Vulkan feature limits can affect individual games; retain a known
working setting and record the first failing stage when testing alternatives.

Earlier API results and the narrow R.E.P.O. user result remain recorded in
[launcher validation](LAUNCHER_VALIDATION.md). This checklist does not claim
new game compatibility or a completed public binary release.
