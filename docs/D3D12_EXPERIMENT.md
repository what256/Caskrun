# Experimental Direct3D 12 in Caskrun

_26 October 2026. Integrated in the 0.2.0 development app through the reviewed
V16 engine and the existing DXMT bottle option. Game acceptance remains a
separate user test; Automatic does not select Direct3D 12._

## Working offscreen path

The existing pinned [DXMT source](https://github.com/3Shain/dxmt/tree/e94c312f5c054263acf261cfa109edf13e757587)
already contains an experimental D3D12 implementation. Earlier Caskrun builds
did not enable that component. The development builder compiles its Windows
DLL against the matching reviewed DXMT DXGI and Winemetal bridge. It needs no
new LLVM build, Apple payload, CrossOver component or downloaded game.

Original x64 Windows probes passed through Wine on Apple Silicon:

| Workload | Evidence |
| --- | --- |
| Offscreen clear | Hardware FL11_0 device; command queue/list, RGBA8 render target, clear, texture copy, GPU fence and readback; 256/256 expected pixels. |
| Shader draw | Standard VS/PS 5.0 compilation, serialized root signature, graphics pipeline and full-screen triangle; 256/256 shader-colored pixels, distinct from the clear color. |
| Patched presentation lifecycle | 193 API checks, six successful Present calls and 342,400 correct back-buffer pixels across initial 240×180, resized 320×220 and recreated 288×200 windows. |

Each probe checks its logical DLL hashes, then waits while the host verifies
the exact candidate DLL, matching DXGI/Winemetal PE files and native Winemetal
bridge are mapped in the same owned process. The draw probe additionally
checks the exact shader compiler. Wine's compiler can import the reviewed
WineD3D module as a compiler dependency; that is not a fallback rendering
result. No Apple or DXVK rendering implementation is accepted by this gate.

The first app-local load failed because the DLL carried Wine's builtin
classification. The builder now has two explicit source-build modes:
Wine-builtin output for the reviewed runtime view, and `--app-local` output for
the isolated native-override experiment. The latter is linked from source
without applying the builtin marker. Packaging creates a new complete engine;
it never edits an installed engine in place.

## Presentation bridge

The D3D12 swapchain lacked the original Caskrun surface preparation already
used by D3D11. The additional LGPL patch passes the swapchain-owned opaque
surface into the private queue Present method. It prepares that surface under
the queue's commit lock, immediately before drawable encoding. The call is
synchronous; the queue does not retain the surface or change public COM slots.
The patched candidate passed the separate presentation, resize and window
recreation API test above. This includes back-buffer readback, GPU completion,
buffer-index rotation, descriptor/resource release before resize and owned
window teardown. It does not measure pixels on the composited macOS screen,
fullscreen behavior or a game. The initial 64×48 fixture failed its exact
client-size assertion before swapchain creation; realistic small windows and
a bounded size-settle check corrected that fixture without relaxing assertions.

## Reproduce without touching installed apps

Start with the exact reviewed runtime inputs named and hashed by the builder.
The default command verifies prerequisites only. `--build` creates the isolated
candidate; it neither installs it nor launches Wine. A changed source tree,
linked output path, unexpected archive entry or mismatched input is refused.

```sh
python3 scripts/build-dxmt-d3d12-dev.py --app-local
python3 scripts/build-dxmt-d3d12-dev.py --app-local --build
python3 scripts/test_dxmt_d3d12_dev.py
```

Compile the original probes with the MinGW commands in their headers. Use
`scripts/test-dxmt-d3d12-dev.py --help` for the isolated host mapping gate and
prefix-scoped shutdown. Keep its generated logs and evidence private. Both
the whole-process watchdog and GPU-fence deadline are bounded. No storefront,
account or publisher content is needed.

## App integration and test steps

The V16 package identity is
`sha256:a235fd32318839ec2aa164befec58a83bad8bd988ec04f7433e9a8faa936d9fa`.
Its x64 DXMT D3D12 DLL is
`c22f3717f2ff21b04d4206fe68f1c2f4e0928625d145ca52b1f35cfa9456974c`.
The documented Wine builtin conversion changes only its reserved DOS marker;
source build provenance, LGPL notices and all original component inputs remain
in the package. Base WineD3D, x86 and DXVK component bytes are preserved.

The actual ARM64 Core and bundled dispatcher passed the 38-check shader draw
and 193-check presentation lifecycle again. A real idle V15-to-V16 app-bundle
migration preserved the saved DXMT preference, nondefault 60 FPS target and an
original save sentinel. Busy renderer changes were refused, switchback worked,
prefix graphics files stayed unchanged and owned storage was removed after
independent process/file-owner checks. Source test harness:
`scripts/test-dxmt-d3d12-core.py`.

To test a game, quit that bottle's apps, select **DXMT · Experimental** in its
existing Display & Graphics inspector, then open the game. Keep a known working
setting for comparison. The package-gated capability appears only with the
exact reviewed engine. Direct3D 12 stays outside Automatic's decision rules.
The managed D3D12 DLL override is bottle-wide; x86 children still have the
original Wine stub and gain no D3D12 support.

Frame target forwards the same saved bounded value to both D3D11 and D3D12.
MetalFX spatial remains Direct3D 10/11 only and is labelled accordingly. Saved
settings stay portable through duplicate and backup/restore. Running bottles
cannot change renderer, synchronization or tuning in place.

## Limits

This pinned implementation advertises at most feature level 11_1 and shader
model 5_1. DXIL, geometry/tessellation/stream-output pipelines and other paths
are incomplete. Some unimplemented calls abort. A successful simple DXBC draw
does not make modern DirectX 12 games supported or supply ray tracing, DLSS,
temporal reconstruction or frame generation.

There is no separate D3D12 backend or general compatibility guarantee. Use
manual DXMT for this experiment. Visible screen composition, fullscreen,
controllers, sustained play and performance remain title-specific tests.

## Separate Apple D3DMetal control

The official, locally accepted Apple framework also passed a native x86_64
factory/device/clear/readback control using the independent MIT-licensed
[UTM host interface](https://github.com/utmapp/d3dmetal-native/tree/422df8736de0f371d5ecee71e04b3ed19c4a2183).
Its optional hooks, swizzles and private-global modifications were excluded.
All 256 pixels matched, all 34 Apple file/link fingerprints stayed unchanged,
and the Apple signature verified before and after.

That control runs outside Wine. The Wine-host factory failure remains
unresolved: native callback handles and thread/COM dispatch cannot simply be
substituted for Wine's interfaces. The native result establishes framework
health, not Windows application compatibility. Apple binaries, account data
and raw diagnostics remain private; local evaluation acceptance is not
redistribution approval. DXMT's separate open-source D3D12 path is not Apple's
D3DMetal backend.
