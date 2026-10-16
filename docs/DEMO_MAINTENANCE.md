# Maintenance workflows

The development app uses Collection/Discover navigation with named Windows bottle folders.
Select a Windows app/game, then open **Files & Maintenance** in its details.
Empty or hidden bottles remain reachable through + → Manage bottles or Command-3.
They do not establish publisher-client or game compatibility.

## Recently Deleted

**+ → Recently Deleted…** opens deleted bottles at the bottom of the inspector.
**Restore** returns a bottle to Collection. Its trash button opens a named
**Delete Permanently** confirmation; accepting it erases that deleted bottle’s
apps, games and files without a recovery option.

`purge-deleted <bottle-id>` refuses active or unknown bottles and requires the
bottle lock, verified idle runtime/process state and no open file holders.
The core pins real directories and removes entries relative to their file
descriptors; payload symlinks are removed as links, never followed.
Unsafe layouts, mounts, unverified ownership or entry limits stop removal.

## Original game icons

Installed Windows apps and games use compact original icons. Steam game icons
come from exact bottle-local icon cache names or a Steam shortcut associating
its AppID with that client’s original icon. A verified public icon hash can
fetch the missing ICO from Steam’s official CDN; no account or bottle paths
are sent. ICOs use their largest available frame through the shared native
icon renderer. Publisher-provided resolutions vary. Browser games keep their
wide picture cards. Store cover images are never used as a replacement icon.

## Installed Programs

`installed-programs <bottle-id>` reads the machine and current-user uninstall
registrations directly from Wine's on-disk registry. The sheet lists names,
versions and publishers, offers search, and opens the existing Windows
installed-programs panel for the user to manage a product.

The reader never starts Wine or initializes a bottle. Files, rows, display
fields and output counts are bounded. Changed, malformed, missing or unsafe
registry evidence is reported as incomplete; symlinks and nonregular inputs
are refused. Registry views identify the registration source, not an inspected
executable's architecture. Portable apps may have no registration, and disk
contents may lag a running Wine server. This is not a dependency inventory or
an automatic component repair system.

## Rebuild App Icons

`rebuild-icons <bottle-id>` holds the bottle lock and requires both tracked
apps and independent file-owner inspection to be idle. It validates recognized,
owned generated native bundles and derived artwork before deleting cache data.
Unknown files, links, shared files and readonly layouts are preserved.

Publisher executables, installed programs, preferences, launch options and
engine records are unchanged. Collection artwork reloads automatically; native Dock
aliases regenerate on their next ordinary launch. A per-bottle UI generation
rejects stale asynchronous icon results after a repair.

Recoverable intermediate cache claims stay under `icon-repair-cache` with
bounded receipts. A retry accepts only its original recognized claim contents;
foreign changes are refused. This disposable directory is excluded from
duplicate/export and refused as reserved metadata during import, like native
app aliases. Ordinary artwork backup behavior is unchanged.

## Diagnostics and log export

`diagnostics <bottle-id>` returns a closed, versioned JSON projection:
app/core version, initialization state, selected engine and record agreement,
renderer status, saved-app counts, known setup stage, bounded log metadata and
fixed issue codes. It never runs Wine, migrates a record or reads log contents.
Selected-engine health is checked separately from an old bottle record.

Only reviewed static engine identities can enter the report. Program names,
bottle/session identifiers, local paths, launch arguments, arbitrary errors,
raw logs and credentials are excluded by construction. Save Report and Copy
Report operate locally; nothing is uploaded automatically.

The existing log viewer separately offers **Save Text**. It saves exactly the
displayed text, including the existing latest-64-KiB limit. This raw output may
contain personal paths or account data; it is not the structured report and is
not redacted. The native save panel describes that distinction.

## Launcher editing and one-time commands

App Options → Rename App changes only saved launcher metadata. Identity,
executable, visibility, creation time and arguments are preserved. The new
`rename-app <bottle-id> <app-id> <name>` command uses the bottle lock and the
same printable-name policy as bottle names.

Run an EXE now offers a temporary argument sheet. If the selected executable
already has a saved launcher, it routes through that launcher to retain the
current recipe and saved options. Multiple saved commands for one executable
have an explicit selector. Otherwise `launch <bottle-id> <path> -- ...`
starts it once without adding a Collection icon. Each line is one literal argument;
leading/trailing spaces are preserved, and no shell is involved. Saved and
temporary editors share bounds matching the core. Null characters are refused
before launch.

Shortcut suggestions now carry a checked working directory as well as the
executable and literal arguments. Only existing, contained C: directories are
accepted, stored relative to the prefix and rechecked before launch and copy.
An unsafe or unsupported requested directory suppresses that suggestion
instead of silently changing its command. A held directory descriptor pins
the launched command's working folder. Ordinary EXE launches retain the EXE's
parent directory. Backup, restore, duplicate and imported suggestions preserve
this field. Original x64 and x86 Windows probes verified both paths by reading
relative files from their actual working directories.

## Bottle notes and Windows version

Bottle Notes saves bounded local text in the bottle, with explicit Save and
unsaved-change confirmation. Notes travel with duplicate and backup; they are
excluded from structured diagnostics. Unsupported notes formats are refused
before copy or restore.

Windows Tools offers an optional compatibility-version control for an
initialized, stopped bottle: Default, Windows 11, 10, 8.1, 8, 7, Vista and XP.
Reads inspect Wine's on-disk registry without starting or initializing Wine.
Writes use Wine's actual global HKCU version override, require full attestation
of the reviewed switchable Wine 11 package and idle ownership checks, verify the result and attempt
rollback on failure. Default removes that override; machine metadata and
per-app overrides are preserved. Unknown custom settings are refused rather
than normalized. This control does not install another Windows version or
prove a publisher app supports it.

## Compressed backups

Export Bottle writes an original `.caskrun-backup` file: a versioned gzip/tar
envelope containing the existing portable folder backup and a closed SHA-256
integrity manifest. The old `.caskrun-bottle` folder importer remains supported.
Checksums detect corruption, not authenticity or harmless software.

The reader checks the complete ordered entry set, types, modes, file hashes,
gzip CRC and end of stream before passing its private staging copy to the
existing importer. It bounds entries, expanded bytes, paths and metadata;
rejects traversal, hard links, devices, sparse/PAX metadata and appended
members; and creates links only after verification. Existing profile and
standard-folder policies still apply. Neither format copies mapped host-folder
contents. Export refuses a running bottle and publishes a new file without
overwriting an existing destination. Copy and archive streaming check
cancellation between chunks, including the folder snapshot/restore stages.
Failures clean owned staging and copied read-only subtrees; any unverifiable
cleanup is reported. Backups contain private app
data and should not be committed or shared without inspection.

## Copying a CrossOver bottle

Import from CrossOver copies an idle win64 bottle into separate Caskrun storage.
The original remains available for comparison. The importer preserves ordinary
profile data even when the Windows account is named `crossover`, and keeps app
registry values that refer to that Windows profile. Runtime/vendor configuration
keys and host paths remain excluded. Before initialization it connects this Mac
account to the copied profile with a bottle-relative link when safe. Existing
profiles are preserved and conflicts are reported instead of silently replacing
settings or saves.

The copy does not include the source Windows tree, engine, builtin programs,
symlinks or external drive mappings. System dependencies and externally stored
game libraries may need to be installed or attached again. A conventional,
checked x86/x64 Steam client receives Caskrun's Steam rendering and native focus
behavior without creating a catalog installation receipt.

On 14 October 2026, a real local copy passed byte-for-byte verification of every
copied app/data file, with the original source unchanged. The copied games were
listed in Collection. This does not establish gameplay, sound or voice-chat
compatibility. Keep the original until the apps and games you need are tested.

## Sound and microphone access

Ordinary sound playback requires no microphone consent. The native macOS
Caskrun menu offers **Microphone Access…** for voice chat: it checks or requests
macOS audio-input authorization only on that explicit action, never records
audio and does not request unrelated access. Denied or restricted permission is
reported with the normal System Settings path.

The app, generated Windows-app bundles and source loader templates include
microphone purpose text. The current independent runtime remains unchanged;
its embedded loader metadata predates those source-template changes. Windows
apps may therefore need their own consent or a separately reviewed runtime
update if macOS attributes capture to an independent helper. An app permission
grant is not evidence that all clients or games can capture audio successfully.

## Verification boundary

Source tests exercise privacy projection, real Wine registry syntax, safe
cache ownership/refusals/recovery, CLI wiring, unchanged program/settings
records, chunked-copy cancellation/cleanup and literal argument boundaries. An isolated smoke creates a native
alias from an original inert PE without starting Windows, verifies refusal
while another process owns the generated loader, repairs and regenerates the
alias, then checks zero owners before removing its fixture. A separate real
reviewed-runtime profile passed compressed export, restore and wineboot
reinitialization with settings, standard-folder mappings, notes, saved arguments
and working folders intact. Inventory, diagnostic privacy, idle icon rebuilding
and empty staging were checked in the same disposable session.

The final serial suite passed 375 Rust unit tests and 6 integration tests
(2 existing unit tests ignored), plus 97 Python tests and 10 Swift argument
checks. The ARM64 app/core build, strict package signatures and DMG checksum
verified. Clippy retains existing style warnings; the new modules add none.
The signed candidate core passed a separate 15-check CLI smoke, including a
compressed roundtrip and cancellation with no published output or leftover
staging, while the original read-only source remained unchanged.

The original [version probe](../scripts/probes/windows-version.c), compiled for
Windows x64 and x86, measured Wine's API values for Windows 11 (10.0.22000),
Windows 7 (6.1.7601) and restored Default (10.0.19045). Per-app priority, busy
refusal, machine-version keys, display/renderer settings and a sibling bottle
were checked separately. The original
[working-folder probe](../scripts/probes/working-directory.c) verified saved
shortcut folders and ordinary EXE-parent behavior. Both sessions were removed
after prefix-specific stops and independent zero-owner checks. Probe source
is shareable; generated executables and local receipts remain ignored.

Packaged app/core checks and actual game/client acceptance are separate. The
runtime is the same reviewed switchable WineD3D/DXMT package; this maintenance
work does not add backends, change High Resolution defaults, or claim fixes
for the known publisher-client failures.
