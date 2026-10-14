# Native editing and close-to-menu-bar behavior

The development Wine package adds a macOS **Edit** menu with Cut, Copy,
Paste and Select All, including Command-X/C/V/A. These actions send balanced
Windows editing keys to the focused Windows window. The native menu does not
read clipboard contents, field values, login credentials or browser DOM data.

Steam's web helper also gets a native text context menu when the pointer is
an I-beam over its active, visible content. Mouse capture, cursor clipping,
dragging, closing windows and other Windows processes keep their existing
right-click behavior. The click is forwarded as a balanced pair before the
native menu. If targeting clicks are still pending, focus changed, or a
Windows menu or window move is active, the delayed action is refused. A
focused field can still use **Edit → Paste** or **Command-V**.

## Caskrun window lifecycle

The red close button and **Close Window** hide the main Caskrun window while
retaining its SwiftUI scene and app state. Its menu-bar logo remains available;
**Open Caskrun** restores the same window. These actions never request owned
session shutdown. A forwarding window delegate preserves the framework's
other callbacks and is reattached if the framework replaces it.

**Quit Caskrun** remains a separate operation: it stops Caskrun-owned Windows
and experimental PS5 sessions and waits for verified completion. Browser tabs
remain managed by the browser. Closing a Windows app's own window follows
that app's behavior; this change does not turn every Windows close into Hide.

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
`sha256:4f736f73c87adeb030507e2f2fcb74989fbd3460ddf750e3b5d47f8f2d6f3039`.
Known older runtime records have explicit, directly reviewed migration edges.
The native and switchable predecessors use record-only migration; older
TLS/Steam predecessors retain their Steam recipe and PE validation gates.
Migration occurs under the bottle lock only after all relevant owners are
verified idle. It changes the runtime record with an exclusive backup, verifies
the resulting binding and rolls back on failure. App files, profiles, saves,
drive mappings, registry settings and graphics preferences are preserved.
Unrecognized engines and records remain refused.

## Validation boundary

- Real AppKit lifecycle tests cover repeated close, retained window/session,
  delegate forwarding, framework replacement, unrelated windows and explicit
  application termination.
- The exact production editing handler and Wine server key normalization are
  exercised across all modifier combinations, both menu paths and refusal
  states. Cocoa tests cover menu scope, balanced targeting clicks, retained
  target validation and non-consuming pending-input inspection.
- The original Windows probe uses two fields and a fixed synthetic marker;
  it never reads or writes the real clipboard. Its source is
  `scripts/probes/native-text-edit.c`.
- The corrected packaged runtime passed a live two-field probe: Command-V
  and Edit → Paste reached the focused field; native right-click Paste reached
  the second field after its matching right-button down/up pair. In selective
  key-only polling, native Paste was refused until the targeting pair was
  processed. A later explicit Edit action still worked normally.
- The user subsequently confirmed that the real Steam login window was usable
  and its native Paste menu worked after the local cache repair. Account
  sign-in, game compatibility and game save handling remain unvalidated.

A real imported Steam launch later exited with Steam's own 15-second main-loop
stall assertion, without a native-menu exception. The red-close check initially
retained the Caskrun, Steam and helper processes; that is not evidence of
sustained gameplay. An isolated comparison then kept Steam and its helpers
running for 120 seconds on both the previous and updated engines. Six critical
client binaries matched after anonymous updates, and the updated engine reached
a visible sign-in window. No account files, registry or games were copied into
those tests; all disposable prefixes were removed after scoped shutdown and
zero-owner checks. This points to imported-bottle state or preferences, without
identifying the exact cause. A local, idle-only browser-cache reset retained
the old cache directories in a private backup; that real imported client then
remained running for over six minutes without the watchdog assertion. The user
then confirmed the usable login window and native Paste menu. This repair is a
separate reversible diagnostic action, not an automatic behavior of this
runtime update.

The package is a local development build, not an approved public binary
release. Corresponding Wine source, patches and license notices remain inside
the runtime package.
