# MSync feasibility for Caskrun

_Research snapshot: 23 September 2026. No MSync code was added to Caskrun's source or runtime._

## Current engine

Caskrun's development engine is built from the independently downloaded [Wine 11.0 source archive](https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz), pinned in `scripts/build-wine-steam-dev.sh` by SHA-512 `a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b`. The local source tree has no `WINEMSYNC` switch or MSync implementation. Wine's existing in-process [server synchronization](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/server/inproc_sync.c) uses Linux `/dev/ntsync` behind Linux-header and ioctl guards; that path is unavailable to this macOS build. Setting `WINEMSYNC=1` on the current engine would not add MSync.

## Public patch and provenance gate

The public [wine-msync project](https://github.com/marzent/wine-msync) describes a Mach-semaphore based synchronization patch and [licenses it under LGPL-2.1](https://github.com/marzent/wine-msync/blob/main/LICENSE), explicitly welcoming open-source integration. The latest observed repository revision is [`be7f3e2ff40670018cd7aa9042bad487fa7a83ec`](https://github.com/marzent/wine-msync/commit/be7f3e2ff40670018cd7aa9042bad487fa7a83ec), dated 13 August 2024; its update labels the staging patch for Wine **9.15**, not 11.0. Its patch author identifies a `codeweavers.com` address, and the repository includes CX-specific variants. The LGPL license permits reuse under its terms, but the available provenance does not establish that importing this implementation meets Caskrun's separate no-CrossOver-code requirement in `AGENTS.md`.

For that reason, the public patch was used only for a provenance review. It was **not applied, ported, compiled, bundled, or exposed in the UI**. The research checkout is ignored under `.build/msync-probe/`; no MSync code is tracked or included in the development DMG. We did not run a patch-compatibility test or runtime smoke, so Wine 11.0 compatibility and performance are unknown. The bottle controls correctly label MSync unavailable.

## What would make it shippable

Use an implementation with independently established provenance, or design and implement a clean-room macOS synchronization path for Wine 11.0. Either route requires coordinated Wine client and wineserver changes, a fresh isolated runtime build, cross-process wait/signal/timeout/cancellation tests, x86 and x64 Windows test programs, and game launch/play/exit and performance regression tests. A verified idle-bottle migration is needed before replacing the engine for existing bottles. Only then can Caskrun consider automatic use or a selectable advanced control.
