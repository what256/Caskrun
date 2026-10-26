# Wine 11 MSync port

This port starts from the generic public `msync-devel.patch` in
[marzent/wine-msync](https://github.com/marzent/wine-msync), pinned in
`UPSTREAM.json`. The original LGPL 2.1-or-later copyright headers remain in the
new client and server files. `COPYING.LIB` is the exact pinned upstream license.
No CrossOver source, CX-specific variant, runtime or application asset is used.

`../wine-11.0-msync.patch` is the complete corresponding source change against
unmodified Wine 11.0. Apply it after Caskrun's existing Wine patch stack, then
regenerate requests with `tools/make_requests` only if the protocol definition is
changed again. The client and server must always be rebuilt together; the
protocol version deliberately differs from unmodified Wine 11.0.

The port uses Wine 11's `get_sync` objects for server-created events and other
waitable objects. Its optional callback is appended to `object_ops` so unrelated
object tables retain their existing layout. This is a macOS engine component;
it is not a portable Linux/Windows MSync backend.

Run `python3 scripts/test-wine-msync.py --runtime <candidate-runtime>` against a
matching candidate. The original x86 and x64 Win32 probe covers event semantics,
semaphores, recursive and abandoned mutexes, wait-any/all, relative timeouts,
thread/process exit, cross-process wakeups, alertable APC cancellation, access
rights, type mismatches, close/reuse and duplicate-close-source invalidation.
Both MSync enabled and disabled run in new simultaneous isolated prefixes. Use
`--backend dxmt` to test the matching DXMT renderer view too; the default is
`wined3d`.

These checks validate synchronization behavior, not game compatibility or a
performance improvement. Caskrun must not overwrite a live engine or change a
running bottle's synchronization mode. A later release must retain this patch,
the pinned Wine source and license, plus all existing Caskrun runtime notices.

Mixed wait sets that contain a server-only synchronization object without a
shared proxy are refused through Wine's server wait path. They are never treated
as successful by silently omitting the unsupported handle. This is a remaining
compatibility limit of the optional fast path; disabling MSync retains Wine's
normal server synchronization.
