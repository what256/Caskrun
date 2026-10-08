# Optional DXMT controls

_Private demo. These controls do not establish game compatibility or a performance advantage over another application._

## Behavior

The bottle inspector's experimental DXMT controls use the existing reviewed,
switchable Wine 11/DXMT package. No new runtime bytes are introduced. Automatic
still selects WineD3D. High Resolution remains independent, optional and off by
default.

- **Frame target:** application controlled by default (`0`); bounded integer
  targets from 1 to 240 FPS are accepted by the core. Common UI choices are 30,
  60 and 120. This requests Metal-coordinated presentation pacing, not a promise
  of exact displayed FPS. Display refresh and application behavior matter. A
  divisor of the display refresh is recommended by upstream.
- **MetalFX spatial scaling:** off by default (`100`); optional 1.25×, 1.5× and
  2× output scale factors (`125`, `150`, `200`). It scales the output of a
  Direct3D swap chain. It does not change a game's internal render resolution,
  provide temporal reconstruction, or guarantee better image quality or speed.

The upstream [customization document](https://raw.githubusercontent.com/3Shain/dxmt/e94c312f5c054263acf261cfa109edf13e757587/docs/CUSTOMIZATION.md)
and [configuration reference](https://github.com/3Shain/dxmt/blob/e94c312f5c054263acf261cfa109edf13e757587/dxmt.conf)
are pinned to the same source revision as the packaged translator.

Changing these controls requires verified DXMT selection, the bottle lock,
stopped tracked apps, an idle bottle Wine server and independent zero-file-owner
checks. A refused change preserves the previous preference and running apps.
The approved runtime-record migration and pre-Wine rollback boundary are the
same as renderer changes. Existing application files, DLLs and registry entries
are not replaced.

## Configuration precedence

With both controls at their defaults, Caskrun leaves the previous DXMT launch
configuration unchanged, including explicit environment and app-local config.
When either control is customized, Caskrun supplies a bounded managed DXMT
configuration for that launch. An external `DXMT_CONFIG` or `dxmt.conf` cannot
silently override the selected frame target or scaling mode. Existing config
files stay on disk, and DXMT's compiled per-game profiles still participate.
Resetting both controls to their defaults restores the prior launch behavior.
Users who maintain a custom `dxmt.conf` should keep the Caskrun controls at their
defaults to continue using that file.

The managed configuration uses Wine's empty `NUL` device plus typed
`DXMT_CONFIG` entries and the explicit `DXMT_METALFX_SPATIAL_SWAPCHAIN` flag.
This avoids a DXMT parser detail: an unmatched final application section in a
config file also disables later environment lines. No shell evaluation or
arbitrary renderer option string is exposed by Caskrun.

## Storage, portability and diagnostics

`graphics.json` version 2 records the renderer, `dxmt_frame_rate` and
`dxmt_spatial_scale`. It contains no engine path or device identity. Version-1
preferences still read as defaults; a version-1 record claiming either newer
field is refused. Version 2 requires both typed, bounded fields. Unknown fields,
versions, links, oversized records and duplicate fields are refused.

Changing renderer preserves the optional DXMT preferences, even while another
renderer is selected. Duplicate and both Caskrun backup formats carry them.
Validation occurs before the folder importer creates bottle storage. Diagnostics
reuse the same bounded parser and emit only their existing allowlisted renderer
summary; they do not export these options or the raw preference.

CLI contracts:

```text
bottle-graphics <bottle-id>
set-bottle-dxmt-options <bottle-id> <frame-target> <spatial-scale>
```

The graphics result retains its prior fields and adds `dxmt_frame_rate`,
`dxmt_spatial_scale` and `dxmt_spatial_available`. Capabilities remain gated by
the exact reviewed runtime package. The app also checks MetalFX's public
`supportsDevice` capability before offering a spatial scale on this Mac. A
missing or unsupported Metal device prevents enabling scaling, while a saved
scale stays visible and can be reset to Off. The core's availability field
describes the package capability, not a per-device gameplay guarantee. The UI
preserves valid nonstandard saved frame targets among its common choices.

## Reproduction and evidence boundary

Compile the original probe for both Windows architectures:

```sh
mkdir -p .build/dxmt-options-probe
x86_64-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror \
  scripts/probes/dxmt-smoke.c -o .build/dxmt-options-probe/dxmt-options-x64.exe \
  -ladvapi32 -ldxguid -luser32
i686-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror \
  scripts/probes/dxmt-smoke.c -o .build/dxmt-options-probe/dxmt-options-x86.exe \
  -ladvapi32 -ldxguid -luser32
python3 -B scripts/test-dxmt-options-runtime.py \
  --core .build/dxmt-options-probe/caskrun-core \
  --runtime .build/dxmt-wine-runtime-dev-v4 \
  --probe-x64 .build/dxmt-options-probe/dxmt-options-x64.exe \
  --probe-x86 .build/dxmt-options-probe/dxmt-options-x86.exe --matrix
```

Use a newly built ARM64 core copied into that ignored probe directory. The
harness creates separate fresh owned x64 and x86 WoW64 bottles, proves exact
live mapped renderer files, exercises both Direct3D feature levels, the original
shader/readback and two-window resize/recreate/destruction sequence, and rejects
both renderer and option changes while a held probe is running. All probes and
raw logs stay ignored. Cleanup stops only those prefixes, proves zero owners
and removes their disposable storage.

The original probe's optional `--pacing-frames` trial reports API-submission
elapsed time. Its measurements are deliberately labeled separately from display
scanout and game FPS. The MetalFX capability probe checks the system's spatial
scaler support through the same public MetalFX device query used by the bridge.
The harness refuses a reported unsupported-device fallback.

## Verified development matrix

The unchanged reviewed v4 package passed all ten combinations below in fresh,
owned bottles on the development Apple Silicon Mac. The public MetalFX spatial
device-capability query returned supported, and the translator reported no
unsupported-device fallback. These are single probe observations, not benchmarks.

| Optional controls | Windows x64 | Windows x86 | API submission time for 60 frames, x64 / x86 |
| --- | --- | --- | --- |
| 30 FPS target, spatial off | Pass | Pass | 1.614 s / 1.548 s |
| 60 FPS target, spatial off | Pass | Pass | 0.765 s / 0.848 s |
| Application controlled, 1.25× spatial | Pass | Pass | 0.198 s / 0.230 s |
| Application controlled, 1.5× spatial | Pass | Pass | 0.206 s / 0.207 s |
| Application controlled, 2× spatial | Pass | Pass | 0.206 s / 0.223 s |

Every state passed both Direct3D feature levels 10_0 and 11_0, original shader
and texture readback, alternating two-window rendering, resize, recreation on
the same window, release after window destruction and surviving-window
readback. The harness proved the selected runtime files were mapped into the
live process. Both renderer and optional-control edits were refused while the
probe was running, preserving the exact preference and runtime record. The
translator logged the requested managed values despite deliberately conflicting
inherited configuration. Both bottles returned to Automatic with default
options, then were stopped with prefix-scoped controls, independently checked
for zero owners and removed.

The redacted local receipt reports `passed: true` and `cleanup: removed`;
runtime logs, executable probes, account paths and timing receipts remain in
ignored build storage. The runtime component receipt SHA-256 was
`d9a76ac2188e2dd3cb9bbb17e353680cfe2d373b771754ad86cf47cd2bfeb42e`.

The final packaged core and its bundled runtime also passed a separate combined
60 FPS / 1.5× trial for both Windows architectures, with the same rendering,
lifecycle, live-mapping and busy-change gates. Its two disposable bottles were
removed after scoped shutdown and zero-owner verification. A subsequent
packaged artwork smoke passed the original executable-icon composition,
publisher-color preservation, app/bottle cache reuse and no-launch-alias checks;
its fixture was also removed after a zero-owner check. These checks did not
modify the app package or any user bottle.

Visible output pixels, image quality, real game performance, fullscreen behavior
and other Macs still need manual tests. Test both controls off first, then each
independently; reset both if a game regresses. DXVK, D3DMetal, D3D12, MSync and
media components remain separate work.

