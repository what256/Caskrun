# Native editing and close-to-menu-bar behavior

The development Wine package adds a macOS **Edit** menu with Cut, Copy,
Paste and Select All, including Command-X/C/V/A. These actions send balanced
Windows editing keys to the focused Windows window. The native menu does not
read clipboard contents, field values, login credentials or browser DOM data.

Steam's web helper also gets a native text context menu when the pointer is
an I-beam over its active, visible content. Mouse capture, cursor clipping,
dragging, closing windows and other Windows processes keep their existing
right-click behavior. The original balanced click targets the field without
changing its selection. A bounded asynchronous query verifies the original
Windows menu's window, root, GUI thread and owner, requests menu cancellation,
then waits for tracking and capture to end before showing the native menu.
The two menus are never deliberately displayed together. Unknown custom
popups retain their original menu; **Edit → Paste** and **Command-V** remain
available.

A new key, click, scroll, loss of focus or closing window cancels the pending
replacement. Queries expire after 450 milliseconds. Four outstanding query
slots are retained until their final queued owner releases them; at capacity,
the original click passes through unchanged. Failed clock readings refuse the
replacement before any targeting input. Delayed editing actions also refuse
pending mouse input, changed focus and Windows menu or move mode.

## Caskrun window lifecycle

The red close button and **Close Window** hide the main Caskrun window while
retaining its SwiftUI scene and app state. Its menu-bar logo remains available;
**Open Caskrun** restores the same window. These actions never request owned
session shutdown. A forwarding window delegate preserves the framework's
other callbacks and is reattached if the framework replaces it.

**Quit Caskrun** remains a separate operation: it stops Caskrun-owned Windows
and experimental PS5 sessions and waits for verified completion. Browser tabs
remain managed by the browser.

## Steam sign-in window

Steam draws its sign-in X inside its Windows client surface. For the recognized
English **Sign in to Steam** window, the development driver replaces the empty
top strip and custom X with a 30-point native header. It uses AppKit's standard
close and minimize controls and native window dragging. Close hides the Steam
UI process; it does not send a Windows close request or stop its bottle.
The Dock can restore the same window. Minimize uses the existing Wine request
and acknowledgment path; zoom is disabled for this fixed-size window.

The policy requires the Steam web-helper process, the exact title and fixed
700-by-440 client size, and an eligible top-level window. Its Win32 geometry,
Steam logo and input fields are retained. Changed titles, sizes, parents,
resizable windows and other Windows processes retain their original surface.
Drawing is bounded at the view, layer and dirty-rectangle levels so the native
header cannot paint over Wine's client image.
Explicit **Quit** remains unchanged. **Hide Steam** (Command-H) is also
available. Logged-in Steam windows and active-game retention require separate
user-led testing; this policy does not change their close behavior.

## Independent runtime build

Run `python3 -B scripts/build-wine-native-text-dev.py --check`, then run the
same command without `--check`. Both commands require the existing reviewed,
independently built seed and switchable development package under `.build`.
The check reconstructs every Wine source file from its exact upstream archive
and reviewed patch sequence before accepting the seed.

The builder makes two isolated clones and rebuilds the complete macOS driver
and both Windows halves for each graphics view. The DXMT driver retains its
exact ten exports; the default driver retains its two Unix-call table exports.
Only six driver files, their component receipt and corresponding patch/source
metadata change. Translators, TLS libraries, loader identities, dependency
notices and other engine files remain intact. Only changed Mach-O drivers are
repaired and signed. App packaging checks the entire runtime identity after
outer app signing and refuses a changed engine.

The new package has exact content identity
`sha256:744741740221a92dc10849620661801dcbf3361fdda5fbbc3bf4bca6a6f252a1`.
Known older runtime records have explicit, directly reviewed migration edges.
The native and switchable predecessors use record-only migration; older
TLS/Steam predecessors retain their Steam recipe and PE validation gates.
Migration occurs under the bottle lock only after all relevant owners are
verified idle. It replaces the runtime record with an exclusive backup, verifies
the resulting binding and restores that record on failure. Preservation tests
cover app files, profiles, saves, drive mappings, registry settings and graphics
preferences. The existing idle checks can refresh managed Desktop configuration
and prune stale process records; this is not a claim that every idle probe is
write-free. Unrecognized engines and records remain refused.

The intermediate locally deployed package
`sha256:f3c98358ec68997c3c7f5ed10e406a20e34022bbd8f17fd0827ce21086b16886`
had the confirmed header-painting regression. Its exact identity remains a
historical migration source and renderer-protocol approval, which does not
claim that its Steam UI was usable. The corrected package can replace its
record through the same locked, idle-only path while retaining earlier backups.

## Validation boundary

- Real AppKit lifecycle tests cover repeated close, retained window/session,
  delegate forwarding, framework replacement, unrelated windows and explicit
  application termination.
- The native sign-in header passes 72 headless and 110 actual AppKit checks
  using the extracted production methods. Its standard close control hides
  the window while preserving its client geometry and an independent owned
  process. Unhide restores it; minimize follows the modeled Wine acknowledgment.
  Paired input, foreign windows, eligibility changes and explicit Quit are
  covered. Both changed production objects compile with Wine's build flags.
- An exact WineContentView composition regression checks the client image and
  rendered body pixels with the header attached, repainted and removed, including
  attaching it before the first client image. An intermediate local candidate
  left its header layer unclipped and painted over Steam's content. Explicit
  layer clipping restored the body in the same fixture; drawing is also bounded
  by the view and the intersection of the dirty rectangle with its bounds.
- The exact production editing handler and Wine server key normalization are
  exercised across all modifier combinations, both menu paths and refusal
  states. Cocoa tests cover menu scope, balanced targeting clicks, retained
  target validation and non-consuming pending-input inspection. The refined
  menu tests model a real Windows popup, delayed cancellation unwind,
  unsupported popups, clock failure, input cancellation and late queued owners
  retaining the four-slot limit. A reused HWND is refused unless it still maps
  to the captured Cocoa window.
- The original Windows probe uses two fields and a fixed synthetic marker;
  it never reads or writes the real clipboard. Its source is
  `scripts/probes/native-text-edit.c`.
- A later live probe with a real Windows `TrackPopupMenu` verified that menu
  tracking ended before the native menu appeared, selected text survived, and
  one Paste reached the intended field. A delayed original menu retained its
  own UI, and selective key-only polling produced neither an extra menu nor
  misplaced Paste. That probe uses `scripts/probes/native-text-menu.c`.
- The corrected packaged runtime passed a live two-field probe: Command-V
  and Edit → Paste reached the focused field; native right-click Paste reached
  the second field after its matching right-button down/up pair. In selective
  key-only polling, native Paste was refused until the targeting pair was
  processed. A later explicit Edit action still worked normally.
- The user subsequently confirmed that the real Steam login window was usable
  and its native Paste menu worked after the local cache repair. Account
  sign-in, game compatibility and game save handling remain unvalidated.
- After installing the bounded-header package, the user confirmed that the
  real login window rendered again. Its right-click menu remained the single,
  usable Windows fallback; that case still needs verification of the native
  replacement. A separate live main-window close check found Caskrun's window
  off screen while its process and Steam's sign-in window remained running.
  This does not establish retention of a logged-in client or an active game.

An earlier real imported Steam launch exited with Steam's own 15-second main-loop
stall assertion, without a native-menu exception. The red-close check initially
retained the Caskrun, Steam and helper processes; that is not evidence of
sustained gameplay. An isolated comparison then kept Steam and its helpers
running for 120 seconds on both the previous and updated engines. Six critical
client binaries matched after anonymous updates, and the updated engine reached
a visible sign-in window. No account files, registry or games were copied into
those tests; all disposable prefixes were removed after scoped shutdown and
zero-owner checks. These tests did not identify the cause of the intermittent
real-client exit. A local, idle-only browser-cache reset retained
the old cache directories in a private backup; that real imported client then
remained running for over six minutes without the watchdog assertion. The user
then confirmed the usable login window and native Paste menu. This repair is a
separate reversible diagnostic action, not an automatic behavior of this
runtime update.

The package is a local development build, not an approved public binary
release. Corresponding Wine source, patches and license notices remain inside
the runtime package.
