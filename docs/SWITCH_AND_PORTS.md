# Switch and native-port research

**Implementation update:** Caskrun now supports explicitly selecting an
external Suyu app and adding individual game-file references to Collection.
That launch path has fixture coverage, but no retail game has been tested in
Caskrun. Nothing below implies an engine or game is bundled.

Reviewed 11 November 2026. These independently developed projects do not
provide compatibility guarantees in Caskrun. Their availability,
system requirements, licenses and supported games can change.

- [Suyu](https://github.com/suyu-emu/suyu-main) publishes an Apple Silicon
  macOS build for Nintendo Switch. Its maintainers report bounded opening checks
  for selected games on an M5. They do not claim complete compatibility, and
  Caskrun has not validated gameplay with this engine. It does not ship games,
  keys or firmware.
- [nxnative](https://github.com/seedds/nxnative) builds individual Switch and
  Switch 2 games into native Apple Silicon applications from user-provided game
  files. Its developers list the Switch 2 Ocarina of Time remake among tested
  Mac games. Their current requirements are macOS 26.6 or later and Xcode 27.2;
  check requirements again before proposing installation or building a game.
  A game's successful report on another machine is not a Caskrun validation.
- [LAS Recomp](https://github.com/peiikai/LASRecomp) is a separate native port
  of the 2019 Switch release of Link's Awakening. It supports Apple Silicon
  Macs and needs the user's own extracted files; it is not the Switch 2
  Ocarina of Time remake.
- [Hookshot](https://github.com/NotRexed/Hookshot) targets the Switch 2 Ocarina
  of Time remake on Windows and Linux. A Windows executable is not evidence
  that it will work through Caskrun's experimental graphics paths on macOS.

No game files, console keys, firmware, third-party executables or copyrighted
assets are provided here. Do not add a launcher as functional without testing
its installation, game launch, controller input, saves and shutdown on the
actual target machine with a user's own files. Do not automatically scan or
copy existing game libraries, keys or personal directories.
