# Wine GFXT mock transport probe

This is an experimental, mock-only fixture. It exercises an original PE DLL and Wine Unix-call helper with an actual disposable Wine registry key. It does **not** load Apple's D3DMetal framework, create a D3D device, implement Wine's graphics backend or prove game compatibility. Only a mock factory object is created and released on the Unix side; no native object pointer is returned to PE.

## Provenance

`typed_gfxt.h` contains partial GFXT class and method declarations based on UTM's `d3dmetal-native`, revision `422df8736de0f371d5ecee71e04b3ed19c4a2183`, MIT license. Its license is in `UTM-MIT-LICENSE`. The Wine registry declarations in `native.cpp` describe Wine's public x64 NT interface. Other source files are original Caskrun test code under MIT. No Apple code or binary is included.

## Reproducing the mock gate

From the project root, run `python3 scripts/probes/gfxt-wine-mock/supervisor.py` for a read-only preparation check. Add `--run` to build and execute the fixture against a pinned V16 runtime clone and fresh Wine prefix. It needs macOS, Clang and `x86_64-w64-mingw32-gcc`, the existing V16 build, and the project session helpers. It does not use an installed app or a user bottle.

The supervisor checks the full V16 tree identity, marks only its own compiled DLL as a Wine builtin, signs only its own Unix library ad hoc, and gates execution on the exact same Wine process mapping the owned fixture, PE/Unix helpers and Wine ntdll. It checks that no D3DMetal framework or libd3dshared is mapped at the gate. It stops only the fresh owned prefix and removes scratch only after identity, no-owner and safe deletion checks; any failure is nonzero and failed cleanup preserves scratch. Diagnostics report fixed stages without command arguments or environment values.

The bounded `--run` retest passed all seven fixture checks and removed its owned scratch. `mock-gate-receipt.json` records the earlier and repeated success. This does not establish graphics backend or game compatibility.
