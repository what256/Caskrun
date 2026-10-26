# MSync feasibility for Caskrun

_Updated 26 October 2026. No MSync implementation is included in Caskrun's
source, current runtime or installer._

## Current engine

Caskrun's retained V14 package uses independently sourced
[Wine 11.0](https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz), with the
source archive and original patches retained beside the runtime. Its NTDLL has
no `WINEMSYNC` implementation. Wine 11's in-process synchronization path uses
Linux `/dev/ntsync` and Linux ioctls; that does not supply a macOS backend.
Setting `WINEMSYNC=1` cannot add one.

## Public implementation and provenance

The independently published [wine-msync project](https://github.com/marzent/wine-msync)
offers a Mach-semaphore synchronization implementation. It publishes separate
`msync-devel.patch`, `msync-staging.patch`, `msync-cx22.patch` and
`msync-cx23.patch` variants. The generic development patch targets upstream
Wine; the CX-specific variants are distinct files and are not required merely
to study or port the generic implementation.

Reviewed generic source is pinned to revision
[`be7f3e2ff40670018cd7aa9042bad487fa7a83ec`](https://github.com/marzent/wine-msync/commit/be7f3e2ff40670018cd7aa9042bad487fa7a83ec):

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `msync-devel.patch` | 176111 | `ba20515c8c89b2035f887d2131b22f67222598ea2cd3ffa0219b31df4a67453b` |
| `LICENSE` | 26526 | `20c17d8b8c48a600800dfd14f95d5cb9ff47066a9641ddeab48dc54aec96e331` |

The repository supplies LGPL 2.1 and its new source headers permit LGPL 2.1 or
later. Its public author attribution includes CodeWeavers. That affiliation
alone is not evidence of proprietary CrossOver code and is not a separate
licensing barrier: Caskrun already uses independently licensed upstream Wine
and DXMT contributions. The earlier research note treated that attribution
too broadly. Any eventual integration must retain the actual source notices,
license, pinned original patch, local modifications and corresponding source;
it must not extract code or components from CrossOver's bundle.

## Wine 11 port requirements

The reviewed patch is dated August 2024 and changes 52 files, including NTDLL,
Wine server objects and the client/server protocol. It adds a `get_msync_idx`
object operation and new protocol requests. Wine 11 has since changed these
interfaces: its object operations include `get_sync`, the signal operation has
a reset argument, and its protocol version is 930. The current Linux
in-process synchronization implementation is another path that a macOS port
must preserve. This is a coordinated engine port, not an application setting.

Before exposing an MSync choice:

1. Adapt the generic implementation to the exact Wine 11 source and current
   protocol/object lifecycle, retaining the ordinary server fallback. Do not
   substitute a CX patch or claim a successful port from patch application.
2. Build matching NTDLL, server and affected modules for both runtime views;
   keep x86 and x64 Windows callers compatible with the same server. A partial
   overlay would create an unreviewed protocol mixture.
3. Verify cross-process events, semaphores, mutexes, wait-any/wait-all, timeouts,
   cancellation, APC wakeups, abandoned mutexes, process exit and handle reuse
   with original bounded fixtures. Include fallback and separate-prefix tests.
4. Record a new exact runtime identity and an idle-only migration with rollback.
   Keep a running bottle on one synchronization mode for its whole session.
5. Test games and compare measured behavior/performance before assigning an
   automatic policy or claiming an improvement. The upstream benchmarks are not
   Caskrun measurements.

This review performed bounded source reads only. It applied no patch, built no
engine and launched no Windows process. The macOS synchronization controls
remain unavailable until the engine and its validation exist.
