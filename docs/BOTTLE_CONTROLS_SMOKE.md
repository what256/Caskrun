# Generic bottle controls: isolated runtime smoke

_23 September 2026. Apple Silicon Mac, final Caskrun development app and its bundled independent Wine 11.0 engine._

Two fresh bottles were created under an ignored `.build/generic-controls.*` test home, separate from Caskrun's user data. The packaged ARM64 `caskrun-core` was given only that test home and its own bundled Wine path. Neither the running Steam bottle nor its engine was selected. GUI control-panel commands were excluded so this test would not open extra Wine windows.

| Check | Observed result |
| --- | --- |
| Create and initialize A and B | Both returned `initialized: true` and distinct prefix paths. |
| Read display state | Both began with High Resolution Mode off at 96 DPI. |
| Change A on, then off | A reported 192 DPI with Retina on, then 96 DPI with Retina off. B remained at 96 DPI throughout. |
| Quit Windows apps | Both bottle-specific quit calls reported `server_stopped: true`; the idle calls correctly reported no active server. |
| Simulate Reboot in A | Wine reported the session restarted in A and wrote a control log. A subsequent quit stopped its server. |
| Inspect logs | A listed Wine initialization, both resolution changes, reboot, and quit logs. |

The CLI sequence completed successfully. A read-only open-file check found no process using either test prefix after the final quit. The 3.5 GB disposable prefixes were removed; 44 KB of logs remain under ignored `.build/research/generic-controls-20260923/`. Steam's existing process stayed running throughout.

This validates the real Wine command paths and bottle isolation for these controls. It does **not** validate the visual quality of High Resolution Mode in Steam or another app, rebooting a bottle with unsaved work, or the GUI behavior of Wine Configuration, Game Controllers, Task Manager, Internet Settings, Registry Editor, Command Prompt, or the uninstaller.

## App-visible DPI check

A separate disposable bottle ran a small, self-authored Windows program through the packaged engine. It queried Windows `GetDpiForSystem`, `GetDeviceCaps(LOGPIXELSX/Y)`, and screen dimensions before and after `SetProcessDPIAware`. Both x64 and x86 builds exited successfully. The test program and Wine logs are retained under ignored `.build/research/dpi-effect-20260923/`.

| Mode | Program | Default process | DPI-aware process |
| --- | --- | --- | --- |
| Off | x64 | 96 DPI, 1512×982 | 96 DPI, 1512×982 |
| On | x64 | 96 DPI, 1512×982 | 192 DPI, 3024×1964 |
| On | x86 | 96 DPI, 1512×982 | 192 DPI, 3024×1964 |
| Off again | x86 | 96 DPI, 1512×982 | 96 DPI, 1512×982 |

The mode changes app-visible DPI and physical screen dimensions for a DPI-aware process in both architectures. The probe's default, DPI-unaware process remains virtualized at 96 DPI. The installed Steam executable is x64, so the x64 result covers its architecture, but this still does **not** establish Steam's own DPI-awareness or prove that its window looks sharper. The test bottle was returned to 96 DPI, its server stopped, and its 1.7 GB disposable prefix was removed after preserving the source and logs. The live Steam bottle and running process were not changed.
