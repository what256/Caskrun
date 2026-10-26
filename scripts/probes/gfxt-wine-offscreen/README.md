# Experimental Wine-hosted offscreen control

This is a **source draft**, not an integrated D3DMetal backend. The separate native host uses UTM's fixture services, including an in-memory registry and simulated window/monitor handles. It is not a Wine implementation of those services. The test keeps all native COM objects on the Unix side and returns scalar status only. It cannot run Windows games, provide presentation or establish app compatibility.

The PE fixture exposes two identity gates: `<base>.before.ready` waits for `<base>.before.release` before any offscreen operation; `<base>.after.ready` waits for `<base>.after.release` after all test COM objects have been released. A separate reviewed supervisor must bind both gates to the same owned process and inspect loaded images. Source files and the build helper do not run the fixture.

## Provenance

The MIT `utmapp/d3dmetal-native` source and Apache-2.0 `osy/directx-native` revision, archive URLs and SHA-256 hashes are recorded in the two pin files. Their licenses are retained verbatim here. `basic-host-only.patch` removes optional swizzles, COM vtable hooks, import shims and private-global accesses from the UTM host. The original Caskrun fixture code is MIT-licensed. The official Apple framework is not included.

The restricted host comprises `dmn_init.cpp`, `dmn_log.cpp`, `dmn_gfxt_os.cpp`, `dmn_gfxt_event.cpp`, `dmn_gfxt_path.cpp`, `dmn_gfxt_registry.cpp`, `dmn_gfxt_allocation.cpp`, `dmn_gfxt_library.cpp`, `dmn_gfxt_monitor.cpp`, `dmn_gfxt_adapter.mm`, `dmn_window.mm`, and `dmn_gfxt_swapchain.mm`; the statically linked directx-native dependency comprises `dxn_version.c`, `dxn_log.c`, `dxn_shm.c`, `dxn_shm_darwin.c`, and `dxn_event_kqueue.c`. The UTM source declares the GFXT initializer as `void (*)(GFXTOSInterface*)` and its native D3D/DXGI calls use the Windows x64 ABI. This describes UTM's published interface, not an Apple API compatibility guarantee.

## Build-only draft

`build.py` requires the two separately obtained pinned archives, Meson, Ninja, Clang and x86_64 MinGW GCC. If Meson or Ninja is not on PATH, use `--meson` or `--ninja` to supply an executable path to an existing project-local tool; it never installs them globally. It checks archive/patch hashes before extraction and builds only into a new project-local `.build/gfxt-offscreen-build-*` directory. By default it validates inputs without building; `--build` opts into compilation. It does not execute Wine, load or link against the Apple framework, or access a user bottle. It verifies and reports the exact 12 host and five dependency source/object inputs and their hashes, then retains its scratch and prints its project-relative location and artifact hashes for review; clean it only after checking for owners. A reviewed build-only run produced all four fixture artifacts, verified the source/object inventory and confirmed the helper's relative install name; neither Wine nor Apple D3DMetal was executed.

No Apple execution is authorized by this fixture. Any later offscreen run requires separate review of its exact source, link inputs, owned supervisor, same-process mappings before and after the operation, framework provenance and cleanup. Passing such a run would only show an offscreen D3D12 clear/readback under fixture services; it would not establish a production integration.
