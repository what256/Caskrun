# Demo maintenance workflows

The development app uses Collection/Discover navigation and a flat item grid.
Select a Windows app/game, then open **Files & Maintenance** in its details.
Empty or hidden bottles remain reachable through + → Manage bottles or Command-3.
They do not establish publisher-client or game compatibility.

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
demo/core version, initialization state, selected engine and record agreement,
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
