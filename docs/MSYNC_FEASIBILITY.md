# MSync implementation and limits

_26 October 2026. The original Wine 11 port exists and has isolated API
validation. Final package acceptance and game testing remain separate._

## Current candidate

Caskrun ports the generic public Mach-semaphore implementation to its exact
Wine 11 source. The complete corresponding patch is
[the Wine 11 MSync patch](../runtime-patches/wine-11.0-msync.patch); provenance,
original input, license and port notes are in
[the MSync source record](../runtime-patches/msync/README.md).
No CX-specific patch or installed CrossOver component is used.

The coordinated NTDLL/server port uses protocol **932**, Wine 11's get_sync
objects and a common canonical wineserver for all renderer views. Wine tries a
view's adjacent server before WINESERVER, so secondary view entries are
internal relative links to that common server, rather than independent copies.
Do not mix this client/server set with an unmodified Wine 11 server.

MSync is **off by default**. The inspector offers an opt-in switch with the
matching reviewed package. Version-3 preferences retain backend, DXMT tuning
and MSync together; duplicate and backup/restore preserve them without paths.
Every normal, child and read-only control launch receives an explicit saved
WINEMSYNC=0 or 1. Switching requires the bottle lock, idle server and owner
checks; closing a window is not sufficient. A saved enabled choice on an
unavailable engine blocks launch clearly instead of pretending it is active.

## Actual synchronization evidence

The original bounded x86/x64 Win32 probe ran with MSync off and on against
both WineD3D and DXMT views: **37 checks × eight cells = 296 checks**, all
passing. Fixtures used separate prefixes and were stopped/cleaned independently.
The package proof binds the exact candidate inputs and the complete matrix.

Coverage includes events, semaphores, recursive/abandoned mutexes, wait-any/all,
relative timeouts, thread/process exit, cross-process wakeups, alertable APC
cancellation, access rights, type mismatches, close/reuse and
duplicate-close-source invalidation. The ordinary server path remains the
disabled baseline. No storefront or game ran in this engine pass, and no speed
improvement is claimed.

Mixed wait sets containing a server-only object without a shared proxy remain
a compatibility limit of the optional fast path. Unsupported handles are not
silently omitted and reported as a successful wait. Disable MSync if a title
depends on an unsupported wait combination; real title behavior and performance
still need testing.

## Public upstream and license

The generic [wine-msync source](https://github.com/marzent/wine-msync/tree/be7f3e2ff40670018cd7aa9042bad487fa7a83ec)
is pinned at be7f3e2ff40670018cd7aa9042bad487fa7a83ec.

| Original input | SHA-256 |
| --- | --- |
| msync-devel.patch | ba20515c8c89b2035f887d2131b22f67222598ea2cd3ffa0219b31df4a67453b |
| LICENSE | 20c17d8b8c48a600800dfd14f95d5cb9ff47066a9641ddeab48dc54aec96e331 |

The supplied license is LGPL 2.1 and the source headers permit LGPL 2.1 or
later. Original author notices, including CodeWeavers attribution, remain.
That public contribution is separately licensed; it is not source extracted
from the proprietary application. Local modifications, original inputs,
corresponding Wine source and licenses accompany the engine source record.

## Historical baseline

The retained V14 engine had no WINEMSYNC implementation. Wine 11's Linux
/dev/ntsync path did not provide a macOS backend, and setting an environment
variable alone could not add one. The earlier feasibility review correctly
identified the coordinated client/server port as a prerequisite. Those
source-only observations predated this implementation and its API matrix.
Neither Linux ntsync nor fsync is added by this macOS port.
