# Native editing and close-to-menu-bar behavior

The development Wine package adds a macOS **Edit** menu with Cut, Copy,
Paste and Select All, including Command-X/C/V/A. These actions send balanced
Windows editing keys to the focused Windows window. The native menu does not
read clipboard contents, field values, login credentials or browser DOM data.

Steam's web helper also has a native text context path when Wine's own
Cocoa application cursor is an I-beam over its active, visible content. Mouse
capture before targeting, cursor clipping, dragging, closing windows and other
Windows processes retain their existing right-click behavior. The original
balanced right-button pair targets the Windows control without changing its
selection. A bounded asynchronous query verifies the original window, root,
GUI thread and owner before requesting cancellation of a standard Windows menu.

The development driver also recognizes one captured Chromium popup:
exact class `Chrome_WidgetWin_1`, a visible, enabled top-level popup directly
owned by the original root on its GUI thread. It must have tool-window and
no-activate styles and must not have the transparent style. Chromium toggles
the layered style while the menu is shown, so that bit is observed but not
required. Longer class names sharing that prefix are refused. The existing Steam/I-beam scope
is unchanged; an unknown captured window does not become an editable control.
The query rechecks popup identity, class, styles, visibility, ownership, active
root, focus and request serial immediately before posting one `WM_CANCELMODE`
to that exact popup. It invokes no synchronous application callback.

Success means cancellation was requested, not that the old menu is gone.
Every subsequent query carries the original GUI focus and exact popup. The
popup must be hidden or destroyed, capture must be released, and the same
root and focus must remain before a native menu may appear. A changed popup,
new capture or changed focus refuses the replacement. GUI focus identifies a
Windows control; it does not inspect Chromium's individual DOM fields. The
two menus are never deliberately displayed together. Unrecognized popups
retain their original menu; **Edit → Paste** and **Command-V** remain available.
The user and Claude verified a single native menu on the first and repeated
right-clicks in the foreground Steam sign-in window in build 48. The v12
package retains that exact editing patch; its new changes concern Quit.

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
**Hide Steam** (Command-H) is also available. Explicit **Quit Steam** uses
the separate graceful client request described below. Logged-in Steam windows and active-game retention require separate
user-led testing; this policy does not change their close behavior.

## Explicit Quit Steam

An explicit macOS **Quit Steam** from the prepared visible Steam browser asks
Core to run the saved Windows Steam client with only `-shutdown`. It does not
terminate just the supervised web helper, send a bottle-wide kill, or use a
saved game's arguments. Closing the sign-in window continues to hide it.
Steam decides whether its own apps can finish closing; retained games, save
prompts and logged-in-client behavior are not yet validated.

The native callback verifies its canonical Core, private data/bottle paths and
exact `WINEPREFIX` binding. Core rechecks the selected recorded runtime, one
plain saved client and its Windows headers, then requires the live client's
mapped loader, NTDLL and image vnodes, current kernel execution identity and
bottle scope. Stale identities, ambiguous distinct clients, a different engine
or a sibling bottle cannot select the request. There is no explicit Core
initialization, migration, icon generation or bottle-wide shutdown in this path.

Only one callback can be pending. Its child starts outside the bottle with
standard streams redirected and no inherited Wine descriptors or stale loader
state. AppKit remains responsive while that exact child is waited for and
reaped. Failure keeps the Steam UI process alive and reports an error rather
than killing only the helper and provoking a replacement. A successful request
means the control child completed successfully, not that Steam or every game
has exited.
System logout, restart and shutdown retain Wine's existing session handling.

## Bottle sidebar controls

A Windows bottle's sidebar places **Quit Apps** next to Open while current
file-owner inspection reports activity. The control is hidden when idle;
inspection errors remain unknown and offer a retry, never an idle result.
A normal request uses the selected initialized bottle's existing Wine server
and exact `wineboot -e -s` arguments. It performs no explicit Core initialization,
migration or mapping repair, and applications can show save prompts or decline.
The prerequisite is checked again before launch. The last server can still exit
between that check and startup, and Wine's loader can perform its own bootstrap;
this is not a promise that no Wine startup can ever occur.

While the request is pending, the sidebar shows **Quitting…**. After four
seconds, or a reported failure, **Force Quit…** becomes available with an
unsaved-work warning and explicit confirmation. There is no automatic
escalation. Force first cancels and reaps the polite request's own control child,
then uses the existing `quit-bottle` path with the exact selected runtime and
current ownership checks. Other bottles stay outside that action. Open and
maintenance actions for the stopping bottle are disabled until it finishes.
Full **Quit Caskrun** drains these operations before its own session shutdown;
an interrupted or failed full Quit leaves completed canceled operations retryable.

## Steam's macOS app identity

Core prepares Steam's per-bottle app bundle before starting the client and
passes its validated executable to Wine. The visible `steamwebhelper.exe`
browser reuses it;
Steam's main process retains its accessory role and games keep their own
native identities. This gives the visible helper a real bundle identifier,
friendly app name and existing artwork without a child Core callback.

Explicit renderer, GPU, utility, zygote and crash-handler roles use the raw
Steam loader and remain accessory before any Dock promotion. Both forms of
the Chromium type switch are recognized, the last switch wins, and parsing
stops at the positional separator. Unknown or missing roles retain the
browser path. This keeps background helpers out of the visible app identity.

New aliases are shallow ad-hoc signed in a private staging bundle. Core
compares their executable code with the selected package's loader while
allowing only structurally checked code-signature bookkeeping to differ.
It separately verifies the complete bundle seal and generated identifier
through macOS Security.framework, without enabling network access. Cache
lookups do not spawn signing tools. The new cache schema preserves existing
aliases, and shutdown retains the byte-exact proof for already-running older
aliases. Cached or active bundles are never resigned.

Wine resolves the helper's normal Windows drive-mapping path and requires
both its original and resolved forms to stay in the exact private prefix.
The resolved file must remain the helper. The bundle executable, its cache
ancestors, Core and the selected NTDLL retain their canonical ownership
checks. Missing or refused bindings retain the raw Steam loader.

## Independent runtime build

Run `python3 -B scripts/build-wine-native-text-dev.py --check`, then run the
same command without `--check`. Both commands require the existing reviewed,
independently built seed and switchable development package under `.build`.
The check reconstructs every Wine source file from its exact upstream archive
and reviewed patch sequence before accepting the seed.

The builder pins three base, four UI and two DXMT Wine patches, applying the
reviewed sequence to each appropriate graphics view. It makes two isolated
clones and rebuilds the complete macOS driver,
both Windows halves and the native child loader for each graphics view. The
DXMT driver retains its
exact ten exports; the default driver retains its two Unix-call table exports.
Both NTDLL views retain their exact native exports. Only six driver files,
two native child loaders, their component receipt and corresponding patch/source
metadata change. Translators, TLS libraries, dependency notices and other
engine files remain intact. Only changed Mach-O files are
repaired and signed. App packaging checks the entire runtime identity after
outer app signing and refuses a changed engine.

The v12 package has independently attested exact content identity
`sha256:cae84f3978b2e0820bbdfc95c1e9dd5fa6c79c8d1f1aca825ac86bd2afb77c29`.
Eleven known predecessor packages, including build 48's recognized-popup package
`sha256:fb91add5bbcdddef39738259595c2da592bed8afb863fccd6384ae6b1337a5ee`,
have explicit direct migration edges to it. All 55 historical policy fields and
separate backup names remain unchanged, for 66 total transitions. No intermediate
engine is adopted during a skipped release.
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

The bounded-header predecessor
`sha256:744741740221a92dc10849620661801dcbf3361fdda5fbbc3bf4bca6a6f252a1`
also has an explicit idle-only transition to the bundled-helper package.

The locally deployed build-45 predecessor
`sha256:d8bd4eb824d7ad1b340cbd32ae70f04cdeafb7a1d6016ee4754e472699e61ec8`
has an explicit idle-only transition to the package with one visible helper
identity. Its historical approval and recovery backups remain intact.

The build-46 predecessor
`sha256:aae295ed019afb077ebb6e58d90445d7cbaf0b3a77fe69dc28f42683622715b9`
also has a direct, idle-only record transition to the captured-popup candidate.
Its exact renderer approval and all earlier recovery backups remain intact.
The new package changes native query state within the existing union; event
and query ordinals remain fixed and all driver views are rebuilt together.

## Validation boundary

- Native identity/signing tests exercise a compiled macOS executable, complete
  ad-hoc bundle signing, byte equality after Apple's signature removal,
  metadata/resource tampering and a valid seal around unrelated code. Cache
  reuse performs no signing or rendering, and older byte-exact aliases retain
  their shutdown proof. The ARM release Core also generated a fresh alias
  from the actual packaged loader: its complete seal passed, signature-stripped
  code matched the source, and a second preparation preserved every file and
  modification time. GUI context-menu checks remain separate.
- Extracted production bundle routing passes 58 real-filesystem checks,
  including Wine's normal drive link and refusal of a host-mapped helper.
  The Cocoa loader/process scope passes 44 checks, and 41 foreground-policy
  checks prevent either raw or bundled background helpers reaching Dock
  promotion. Steam's primary process
  and refused helper bindings make no child Core callback; games retain
  their existing callback path. Isolated Steam helpers also registered the
  expected bundle identity and emitted the helper/mode startup canary.
  Those routing probes alone did not establish a usable sign-in window or
  native context menu; the later build-48 foreground check below did verify
  the menu in the real sign-in client.
- Real AppKit lifecycle tests cover repeated close, retained window/session,
  delegate forwarding, framework replacement, unrelated windows and explicit
  application termination.
- The earlier native sign-in header passed 72 headless and 110 actual AppKit checks
  using the extracted production methods. Its standard close control hides
  the window while preserving its client geometry and an independent owned
  process. Unhide restores it; minimize follows the modeled Wine acknowledgment.
  Paired input, foreign windows and eligibility changes are covered. The v12
  headless header fixture passes 83 checks plus 16 composition checks; a separate
  three-group extracted Quit harness covers the new dispatch and callback
  lifecycle. These fixtures do not establish actual Steam shutdown or game retention.
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
  to the captured Cocoa window. The driver passes seven extracted
  editing test groups, including captured-popup outcomes for delayed unwind,
  replacement capture, same-root focus changes, late ownership/style changes,
  truncated class-name prefixes and failed cancellation. Cocoa tests carry the
  exact popup and focus across successor queries. Query-state layout checks
  cover Unix64, PE32 and PE64. These fixtures alone do not establish the real
  Steam first-click result; build 48 supplied the separate foreground evidence.
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
  usable Windows fallback at that point. Build 48 later verified the native
  replacement in a genuinely foreground session. A separate live main-window
  close check found Caskrun's window off screen while its process and Steam's
  sign-in window remained running.
  This does not establish retention of a logged-in client or an active game.

The build-48 combined source passed 507 Rust unit tests and seven integration
tests, with two existing ignored tests. The v12 focused Core checks pass 77
migration tests and 20 graphics tests. They cover all eleven direct predecessors,
all 55 historical destinations, seven earlier backup records, lock waiting,
owner refusal, package-change rollback and exact renderer approval. Twenty-six
Foundation-only checks cover the sidebar's running/unknown/idle states, stale
operation tokens, slow-Quit escalation and recovery after an interrupted full
Quit. Full Swift typechecking passed. Live checks with original owned Windows
processes for the new sidebar controls and explicit Steam Quit are pending;
these source/fixture results do not establish Steam shutdown, game retention or
save behavior.

The first bundled-helper package was installed locally as build 45 after normal
Caskrun Quit and a zero-owner check on the previous app. The imported bottle's
runtime record migrated to this package and its generated Steam bundle passed
strict signature verification. The subsequent real Steam launch exited with
the same 15-second main-loop assertion seen in older launches, before showing
sign-in. A code-only isolated comparison reached browser helpers on both that
package and the bounded-header predecessor, but neither reached sign-in in
the bounded test. The isolated client downloaded an official update, and the
desktop locked during testing; these are material comparison limits. No
first-right-click test was performed and no cause is attributed to the bundle
routing. A subsequent unlocked real launch reached the rendered sign-in
window and stayed running. Its browser, renderer and crash handler shared
the same bundle identifier while computer-use attachment timed out. The
successor routing separates those background roles. The native replacement
and Dock interaction remain live test items.

Build 46 was then installed after normal Caskrun Quit and a zero-owner check.
Two real starts stalled before sign-in. Bounded native stack samples found
Steam's engine IPC thread waiting in Wine's CoreAudio format probe while both
AppKit event loops remained responsive. The blocking operation was assigning
the audio unit's current device; the native sample did not establish input or
output direction. A separate native capture-device probe blocked at that
operation for its five-second limit, while output-device assignment succeeded
in both ARM64 and x86-64 processes.

A temporary, process-only audio-driver exclusion allowed a diagnostic sign-in
window to render. Computer use attached to that actual window and reproduced
the poorly rendered white context menu. A read-only structural probe identified
a captured, directly owned `Chrome_WidgetWin_1` popup with no Windows menu-tracking
mode. It was not a `#32768` standard Windows menu, so relaxing the standard
menu-owner check would not address this different path.

The earlier claim that this probe established a custom or non-I-beam cursor
was incorrect. External `GetCursorInfo` observations and failed inspection of
foreign cursor handles through `GetIconInfo` cannot establish the cursor shape
used by Wine's Cocoa driver. The actual eligibility guard reads that driver's
own `WineApplicationController` cursor and still requires its standard I-beam
or vertical I-beam. The captured-popup implementation does not relax that guard.
At that point its first-right-click replacement and repeated Dock hide/reopen
cycles were pending. Build 48 later verified the first and repeated foreground
menu; Dock hide/reopen cycles remain a separate live check.

After unlocking and inspecting the existing permissions, a normal launch with
audio enabled reached a usable real sign-in window. The permissions were
already enabled and were left unchanged. A repeat native capture-device probe
completed in about six milliseconds instead of reaching the earlier five-second
limit. The audio stall is therefore transient or context-dependent in the
observed tests; a persistent microphone blocker or its root cause was not
established. No audio bypass, registry change or permission change is part of
the product fix. The temporary exclusion was never saved to bottle or macOS
settings.

Exactly one Steam process held the visible native app identity during the
observed diagnostic session, with the main process and background helpers
outside it. That observation does not validate every relaunch or Dock cycle.
The diagnostic session was stopped through the selected bottle's normal
shutdown command, with zero remaining Steam applications confirmed. No account
sign-in, clipboard inspection, audio recording or gameplay occurred; raw
screenshots and native samples were removed, with only sanitized local
receipts retained outside source control.

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

### Build 47 stopping point

The complete v10 runtime was packaged in local build 47. Nested and outer
signatures, exact engine identity, ARM64 app/Core architecture and development
DMG verification passed. The full serial Core suite passed 505 unit and seven
integration tests, with two unit tests intentionally ignored.

A first-click automation attempt still displayed the original white Chromium
popup. A process-only cursor trace requested the native I-beam on the same GUI
thread before that click. This establishes the requested cursor route, not the
Cocoa cursor or key-window state at the event. The Caskrun Steam app was not
observed as the frontmost macOS application, so this attempt cannot establish
whether the guarded replacement works in a genuine foreground session. The
key-window and focus guards must remain in place; background input must not
force activation or replace an unrelated application's menu.

Work stopped at the user's request. Only the Caskrun-owned test bottle was
stopped; the separate compatibility application was left alone. Temporary
keep-awake and cursor tracing ended. Actual foreground menu behavior remained
unverified at that stopping point. No claim of a completed Steam menu repair
follows from build 47; build 48 below supplied the later foreground evidence.

### Build 48: the captured-popup recognizer was too strict

A second opt-in, boolean-only diagnostic engine extended the existing trace into
`macdrv_native_text_context`. In a genuinely frontmost session every Cocoa gate
passed and the request was accepted; the Windows query then refused at the
capture branch on the first poll in which Steam's popup held capture, without
ever recording that popup, so `WM_CANCELMODE` was never posted.

Per-bit observations named the cause exactly. The captured window is directly
owned by the original root on its GUI thread, visible, holds capture, and its
class is exactly `Chrome_WidgetWin_1`. It has the tool-window and no-activate
styles and not the transparent style. It does **not** have `WS_EX_LAYERED`,
which the recognizer required. Chromium toggles that style while its menu is
shown, so requiring it rejected Steam's own menu as an unrecognized window.

The repair removes `WS_EX_LAYERED` from the required set. The exact class and
length, popup shape, child/disabled refusal, transparent-style refusal, direct
owner, GUI thread, visibility, capture holder, active root, unchanged focus and
request serial, and the existing 450ms/four-slot bounds are unchanged. The
earlier structural probe had observed the style in a transient state; the test
suite had encoded the same assumption, asserting that a non-layered popup must
be refused. That case is now inverted and a positive case was added.

The complete runtime was rebuilt as
`sha256:fb91add5bbcdddef39738259595c2da592bed8afb863fccd6384ae6b1337a5ee`
and packaged as local build 48, with verified nested and outer signatures,
exact engine identity, ARM64 architecture and DMG checksum. The full serial
Core suite passed 507 unit and seven integration tests, with two intentionally
ignored. Focused native editing passed seven groups and the packager six.
Ten direct predecessor edges, including build 47's engine, retained all 45
earlier transitions and their backups, for 55 at that release. Build 47 and its DMG are retained in
an ignored recovery directory.

The real imported Steam client migrated to this engine under the bottle lock and
reached its sign-in window. Exactly one correctly named Steam application held
the visible identity. The **first** right-click inside the account field
produced a single native macOS editing menu with Cut, Copy, Paste and Select All
and their Command shortcuts, with no Chromium popup behind it and a responsive
window; AppKit adds its own AutoFill item. Repeated dismiss and reopen produced
the same single native menu. Menu-driven Select All and Copy acted on the
focused field.

Limits of this check: right-clicking past the end of the text collapses the
selection first, which is ordinary Chromium behavior and not a property of this
replacement; a right-click inside the selection retains it. Synthetic typing
through this path did not reproduce the intended characters, so the field marker
was the repeated character the injected keys produced; a full clipboard
round-trip ending in Paste was not completed before testing stopped at the
user's request. No account sign-in occurred, no clipboard contents were read,
and the right-click path while the application is not frontmost deliberately
still retains Steam's own menu.

### V12: graceful Steam Quit and separate bottle controls

The final independently signed and fully attested package is
`sha256:cae84f3978b2e0820bbdfc95c1e9dd5fa6c79c8d1f1aca825ac86bd2afb77c29`.
Its existing native editing patch is byte-identical to build 48. The additional
Wine patch routes explicit native Steam Quit through the verified Core request;
red-close and menu guards remain intact. Both complete source trees match a
fresh nine-patch replay, default/DXMT exports and architectures passed checks,
and translator files are unchanged. Native fixture checks passed. Original-process bottle checks also passed as
recorded below; explicit native Steam shutdown remains a separate live check. No native-game
compatibility or retention claim is added.


### V12 bottle shutdown live checks

The signed ARM64 candidate passed its package signature, exact engine identity
and development-DMG verification. The complete Core suite passed **530 unit
and 7 integration tests**, with two existing unit tests ignored, plus 26 Swift
state checks and full SwiftUI typechecking. Native Quit extracted-production
fixtures passed 3,815 guard, spawn, environment, descriptor and lifecycle checks;
these remain distinct from an actual Steam-client shutdown test.

Two original Win32 apps ran in separate disposable bottles using the candidate's
exact engine location. One accepted `WM_QUERYENDSESSION`, received
`WM_ENDSESSION` and exited cleanly. The other refused the request and remained
running, with no automatic force. The sidebar offered Force Quit with its
unsaved-work confirmation; confirming it stopped only that bottle, while the
first still had nine current owners. The clean app then exited through the
polite Core path. Both bottles reached zero current owners after Wine's normal
background shutdown. The UI hid the quit control and kept Open available.
The test UI completed full Quit, and both prefixes and the isolated UI copy
were removed only after independent zero-owner checks.

The first live attempt was invalidated by the test runner ending while its
Wine descendants still needed its process group. A bounded supervisor fixed
the test lifetime; no product change was needed for that attempt. No publisher
client, account or real game was used, and no existing user bottle was run.
The Steam text-menu patch remains unchanged. Explicit native Steam Quit,
logged-in Steam windows, games and full-Quit cancellation during an active
real app remain separate user-led checks.

### V13: Steam Dock recovery in both graphics views

The independently built package is
`sha256:17504e50264389aa7986828fa6e4ff3578ce97d4fc096b6dabdf12c60c4de1fb`.
The additional original LGPL patch is
`runtime-patches/wine-11.0-steam-reopen.patch`, SHA-256
`0fb1e8c4a481bcf6a5c635e34f54ac85a878e7d07dbeb65cd0149cda9ff0734d`.
The previous native editing, header, identity, and Quit patches retain their
exact bytes. Only both Unix Mac drivers, their component receipt, source
notice and new corresponding patch change. Windows drivers, translators, TLS,
fonts, native text protocol and all other package files remain unchanged.
The Python packager and an independent probe using Core's Rust attestation
agreed on the complete new tree identity.

Wine's upstream Dock delegate only restores a minimized window. A logged-in
Steam client can instead hide or destroy its own main window while continuing
to run. The prepared visible Steam browser now checks actual Wine windows,
content and Wine's saved hidden-app state. If nothing can be restored, it asks
Core to send exactly `steam://open/main` to the same freshly verified running
Steam client. Core reuses the checked Quit control path, runtime and client
identity proofs, filtered environment, scoped tracking and final revalidation.
It does not adopt saved shortcut arguments, alter Steam files or signal apps.
No Cocoa call forces a Windows-hidden window into view. Other publishers and
background processes keep Wine's ordinary restore behavior.

There is one pending Show. An explicit Quit waits behind an existing Show,
then runs once; it suppresses new Shows while its own child remains in flight.
After successful submission, a generation-checked five-second hold prevents
immediate reopen and then permits recovery if Steam did not exit. A failed Quit
child clears the hold and shows the existing failure notice. No timer cancels a child. If Show
stalls, its queued native Quit also waits; Caskrun's separate bottle Force Quit
remains the escape. Submission is not proof that Steam actually exited, and
the five-second hold is not a guarantee for an indefinitely slow shutdown.

Validation before packaged acceptance: exact extracted production helpers
passed 16,140 headless and 16,997 interactive reopen checks. Existing Quit
fixtures passed 3,815 checks; the native header and composition fixtures passed
83 and 16 respectively. This includes hidden retained versus ordered-out,
empty or closing windows, minimized clients, duplicate requests, stale timers,
queued Quit and generic-process negatives. A live Core Show restored a rendered
logged-in Steam Store window twice after Windows Close, with DXMT selected and
effective and one managed Steam application identity. That verifies the client
protocol in DXMT, separately from final Dock-event delivery. It does not verify
gameplay, quitting with a running game, or other publishers' tray protocols.

`python3 -B scripts/build-wine-steam-reopen-dev.py` requires the exact reviewed
V12 package as its base. It verifies the corresponding Wine archive and all
patch pins, configures a small independent Mac-driver build and replaces only
the two Unix drivers. It does not need the deleted full Wine/TLS compiler
trees. The validated local run reused a separately checked minimal configure
and cached build dependencies with `--resume`; a complete fresh-script run
remains distinct from that evidence. Twelve exact predecessor edges preserve
the 66 historical policies and their backup suffixes, for 78 total; upgrades
still require the existing idle, ownership and rollback checks.

### V14: require prior client presentation before Dock recovery

The current package is
`sha256:398d08ea874ad53c8c8f6519e291a990f9d598dde0dfebc88fb149fe6d6c1c54`,
with revised reopen patch SHA-256
`83e4f375644906163239d37581d1f64409cd4d88a21ec3aec82ef63c56fcacbe`.
It retains the earlier control serialization and adds a per-helper history
guard: recovery is armed only when Wine orders out previously drawn, visible
client-sized content. A first-start AppKit reopen with no client window sends
no protocol request. This is a correctness guard, not an established repair for
the independently observed audio-device stall.

A complete clean builder run verified the pinned upstream source, Bison,
configure, both drivers, corresponding patch and complete package identity.
Only the same five package paths change from V12; all Windows drivers, TLS,
fonts and other engine binaries remain unchanged. Exact extracted helpers
passed 16,160 headless and 17,017 interactive reopen checks, with the existing
3,815 Quit, 83 header and 16 composition checks intact. Initial zero-window
reopen, undrawn content and background-process negatives are included.

Build 52's live first starts stalled with both DXMT and WineD3D. One normal
launch log and no extra Show control were observed per attempt. A native sample
placed Steam's engine thread in Wine CoreAudio format checking while assigning
an audio unit's device, matching the earlier intermittent stall. Microphone
access was already enabled and was left unchanged. An idle-only browser-cache
comparison did not repair this attempt, and the original cache was restored.
These observations do not establish that the new Dock delegate caused the
stall. Final packaged Dock and normal-audio startup acceptance remain distinct
from the native fixture and direct client-protocol checks above.

The later macOS permission-service logs tied each attempted startup to a
rejected former Caskrun code requirement and a pending microphone permission
request. The local app is ad-hoc signed, so a rebuild changes its code hash;
the old enabled entry in System Settings did not prove authorization for the
new code. The microphone entry was reset through System Settings and returned to its
enabled state for the replacement build, without changing other apps' grants,
recording audio or disabling the Wine audio driver. Foreground permission
completion was then performed by the user for installed build 53. Normal Steam
startup completed with DXMT selected and effective, rendering the existing
logged-in Store session under one managed native Steam application identity.
Caskrun displayed its starting state and completed the main-window handoff.

After Windows Close, reopening the existing native Steam application through
Finder restored the rendered Store window. A bounded process observation
captured the native helper invoking Core's `request-steam-show` and its child
using the fixed `steam://open/main` command. The original client and native
helper generations survived, DXMT remained selected and effective, and no Quit
control was observed during this cycle. An earlier close/observation cycle
also returned the window, but did not capture the control chain and is not
additional callback evidence. A physical Dock mouse click was not observed;
the verified path is macOS reopening the same running native application.
These checks do not establish fresh authentication, gameplay, actual audio
output or recording, native Quit, or other publishers'
tray behavior. Steam was left open after acceptance.
