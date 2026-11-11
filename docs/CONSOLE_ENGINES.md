# Console engine research

Reviewed 11 November 2026. Engine developers' reports are not Caskrun test
results. No console games, keys or firmware are included with Caskrun.

## Switch

[Suyu](https://github.com/suyu-emu/suyu-main) publishes a macOS Apple Silicon
application. Its developers report limited Mario Kart 8 Deluxe and Tears of the
Kingdom checks on an M5, not full playthroughs. Its
[macOS package](https://github.com/suyu-emu/suyu-main/releases/tag/v14.1.0)
is not notarized; follow macOS security guidance and independently assess any
third-party software. The package has no games, keys or firmware.

[nxnative](https://github.com/seedds/nxnative) recompiles selected Switch and
Switch 2 games into native Apple Silicon apps; it is not a universal converter.
Its current requirements include macOS 26.6 and Xcode 27.2. A separate option,
[LAS Recomp](https://github.com/peiikai/LASRecomp), targets the 2019 Switch
release of Link's Awakening and requires the user's own extracted game data.

## PS5

[SharpEmu for Mac](https://github.com/sharpemu/sharpemu-mac) is an experimental
x86-64 PS5 emulator running through Rosetta on Apple Silicon. Its developers
describe a Metal renderer and publish macOS builds; their game screenshots are
not a guarantee of compatibility or performance. Its macOS release is ad-hoc
signed and not notarized. Original game data must be supplied separately.

[KytyPS5](https://github.com/KytyPS5/KytyPS5) also supports macOS experimentally,
but an [open upstream report](https://github.com/KytyPS5/KytyPS5/issues/1068)
documents GPU feature checks preventing affected official Mac builds from
starting games, followed by further shader limitations on patched builds. Other
builds and games may differ. Caskrun must not imply this is resolved.

[AnyPS5-ARM](https://github.com/AnyTogether/AnyPS5-ARM) is a separate experimental
converter, derived from AnyPS5. Its maintainers report selected games working;
those reports have not been independently verified in Caskrun. Its translated
PS5 x86-64 code still uses Rosetta, and its graphics path uses MoltenVK. Neither
this converter nor commercial game data is bundled or configured by Caskrun.

These projects may change requirements and availability. In particular,
importing a path or opening an emulator does not establish that gameplay,
controllers, sound, saves or shutdown work on a given Mac.
