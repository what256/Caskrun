# Browser game links

Reviewed 9 October 2026. The catalogue contains 26 links: individual games,
community recreations, one collection and three links whose availability could
not be confirmed. GTA 5 is omitted because the supplied shortened link resolves
to an incomplete Wayback timestamp with no original game URL.

**Play in browser** opens the public HTTPS game page in the default browser.
Collections use **Browse games**; unverified entries use **Visit website**.
**Add to Home** saves a local bookmark. Users can also add a different public
HTTPS game page. Caskrun does not embed game code, download commercial game
assets, install a launcher, import an account or run browser games in Wine.

## Provenance and scope

These are links to independent projects, not official commercial releases.
Reaching a page or reading its documentation does not establish gameplay,
performance, device compatibility, publisher affiliation or future availability.
Own-file requirements below reflect the linked project, not a Caskrun test.

| Entry | Project or source | Scope and requirements |
| --- | --- | --- |
| BO1 Zombies | [vel.gg](https://vel.gg/bo1z) | Community browser Zombies recreation. |
| Moon Zombies | [Project](https://moon-zombies.pages.dev/) | Community recreation. |
| Kino der Toten | [Project](https://kino-der-toten.pages.dev/) | Community recreation. |
| Cheese Cube | [Project](https://cheese-cube.pages.dev/) | Recreation of a BO3 custom Zombies map. |
| Vibe Slops II | [Project](https://vibeslops.luckeysystems.com/) | BO2-inspired browser FPS, not the complete Black Ops 2 release. |
| Modern Warfare 2 community link | [Supplied site](https://ovz-game-production.up.railway.app/) | Rate-limited during review; identity, file requirements and gameplay unverified. |
| Skate 3 fan project | [Project](https://skate.aaddpp.lol/) | Requires the user's own converted game copy. |
| Skate × Rust | [Project website](https://global-terror.net/) | Catalogue points to the browser game linked by the project, rather than its merchandise portal. |
| Surfd | [Developer website](https://surfd.net/) | Source-style surfing with keyboard and mouse controls. |
| Halo CE community port | [Project](https://mitchellhynes.com/halo/) | Requires the user's own Xbox XISO; no game data provided by the project. |
| Halo CE Mobile | [Project](https://hcemobile.com/) | Requires the user's own Xbox disc image. |
| PES6 Web Online | [Project](https://pes6.optijuegos.net/) | Community PSP browser port, unaffiliated with Konami. |
| Vice City community port | [Project](https://joncodeofficial.github.io/gta-vice-city-wasm/) | Requires the user's own game files; unaffiliated with Rockstar Games. |
| The Simpsons: Hit & Run | [Supplied site](https://shar-wasm.cjoseph.workers.dev/?skipmovie) | Page reached its loading screen; file requirements and gameplay unverified. |
| QuakeWorld | [Project](https://q1.pieter.com/) | Multiplayer, not a complete single-player campaign. |
| Quake II | [Project](https://q2.pieter.com/) | Browser multiplayer preservation project. |
| Quake III Arena | [Project](https://q3.pieter.com/) | Browser multiplayer project. |
| Return to Castle Wolfenstein | [Project](https://rtcw.pieter.com/) | Multiplayer; site reports about 380 MB on first load and recommends desktop Chrome. |
| Urban Terror | [Project](https://ut.pieter.com/) | The supplied link is Urban Terror, not Unreal Tournament. |
| Half-Life Xash3D port | [Source project](https://github.com/Pixelsuft/hl) | Local game-data ZIP input; requires the user's own files. |
| Half-Life / CS 1.6 WebXash | [Source project](https://github.com/x8BitRain/webXash) | Full games require the user's own Valve/cstrike folders; demos also offered. |
| Diablo browser port | [Upstream](https://github.com/d07RiV/diabloweb) | Shareware offered; full game requires the user's own DIABDAT.MPQ. |
| Hedgewars community link | [Supplied site](https://webwars.link/) | HTTPS hostname mismatch during review; availability unverified. |
| WASM Arcade | [About](https://wasmarcade.com/about) | Community collection; file requirements vary by game. |
| Hextris | [Original project](https://github.com/Hextris/hextris) | Its linked website had an expired HTTPS certificate; availability unverified. |
| 2048 | [Creator's about page](https://play2048.co/about) | Original creator's website. |

The community examples were supplied by the user, including a
[public post](https://x.com/RadiantOpti/status/2108068628793082338), then checked
against their linked pages. Review did not install launchers, import game files,
accept agreements or test gameplay. Certificate validation was not bypassed.

## Scene previews

Seventeen entries have explicit remote scene/cover URLs in `browser-games.json`.
Most are preview images published by their game site. The Hit & Run preview is
from WASM Arcade's game-cover catalogue. No blanket asset-reuse licence was
established; images are not copied into this repository or the app bundle.
An entry without an approved image, or whose image fails validation, shows a
title card. No favicon scraping or invented publisher logos are used.

Discover loads only the exact reviewed artwork URL and any explicitly reviewed
redirect targets. Home can reuse a preview when a saved URL exactly matches a
bundled catalogue entry. Custom bookmarks never cause site crawling or artwork
requests. The loader sends no account credentials, cookies or referring-page
header, uses system HTTPS verification and keeps a bounded in-memory cache with
no disk writes. Viewing a preview still contacts its image host.

The loader accepts only checked PNG, JPEG or WebP responses, caps downloads at
2 MiB and dimensions at 8192 per axis / 32 million pixels, and downscales to
960 pixels. Three requests may run at once; the cache is limited to 24 MiB and
40 images. Cancellation and request deadlines release abandoned requests.

## Validation and saved-link privacy

`validate_browser_games.py` checks required and optional fields, stable IDs,
bounded metadata, duplicate links, entry kinds and conservative HTTPS syntax.
It never contacts a site. Artwork and kind metadata are additive; older entries
without them remain valid. Own-file badges remain visible before opening or
saving entries that explicitly require local game data.

Saved bookmarks stay in private per-user Caskrun data, outside bottle backups
and diagnostic reports. They are not uploaded automatically. Names and URLs
are bounded; files use checked reads and atomic writes. Credential-bearing
query or fragment keys such as `access_token`, `session`, `password` and `email`
are refused, including case and percent-escape variants. This is an input check,
not proof that an arbitrary link contains no private information. Save the
public game page. Each site controls its own storage, privacy and requirements.

```sh
python3 -B catalog/validate_browser_games.py
python3 -B -m unittest discover -s catalog -p 'test_browser_games.py'
```
