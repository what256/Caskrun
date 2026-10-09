# Game compatibility evidence

The standalone Library and Game reports screens were removed on 9 October
2026 at the user's request. The source components, reviewed snapshot and
manifest parsers described below remain; compatibility comparison is not
exposed in the current navigation. Earlier UI validation retains its dated
scope.

Caskrun's game library can show a local device summary and direct community
reports beside a Steam game. **No game in this snapshot is verified in Caskrun.**
A result from a different launcher, engine, renderer, game version or Mac cannot
establish that the same game works here.

## Included data

The bundled `catalog/game-compatibility.json` is a reviewed, offline snapshot of
[Highball's public compatibility database](https://github.com/gauthierpiarrette/highball-db/tree/d8d165834cd0b77613ac0f00300e8fac0b98b143).
It contains 373 Steam app IDs and titles, with 71 rated, direct reports. Some
source report files concern non-Steam games or contain no rating; those records
are omitted rather than assigned a guessed Steam identity or score.

Only these report fields are retained: rating, renderer, generic Apple chip,
macOS version, test date, public engine package ID and source link. Reported RAM
remains unknown because these direct reports do not record it. Submitter names,
account identifiers, model identifiers, narrative notes, screenshots, recipes,
launch commands and executable paths are absent.

Highball declares its curated game data and direct reports
[CC0-1.0](https://github.com/gauthierpiarrette/highball-db/blob/d8d165834cd0b77613ac0f00300e8fac0b98b143/LICENSE).
The original licence is included at `catalog/licenses/highball-db-CC0.txt`.
Caskrun preserves the source repository and revision and links each report to
its public issue or exact source-file line. The repository owner's name appears
only in public source URLs; no contributor identity field is imported.

The ProtonDB-derived prediction layer, anti-cheat database, curated narrative
summaries, artwork and install recipes are **not included**. The data's public
package IDs sometimes name CrossOver-derived engines. They describe the external
test environment; no CrossOver code, engine, recipe or asset is copied.

## What the interface says

- Every detail view says **Not tested in Caskrun**.
- A matching chip is labelled **Reported on your chip**, with memory, OS, game
  version and runtime limitations. It is not a compatibility guarantee.
- Missing chip, RAM, graphics mode or OS fields remain unknown. A report using a
  renderer absent from a checked bottle is identified explicitly. Until the
  bottle's runtime capabilities have been checked, renderer availability stays
  unknown; the app never assumes DXMT is installed merely because it supports it.
- Both high and low community ratings remain visible. Mixed results are labelled
  rather than reduced to an optimistic average.
- A missing report is missing evidence; it is not an incompatible verdict.
- No report automatically changes a bottle, renderer, launch argument or recipe.

The device summary reads the generic CPU brand, physical memory and macOS version
locally. It never reads a serial number, hardware UUID, account or computer name.
Neither the device summary nor the game library is uploaded. Opening an external
source link uses the default browser and contacts that source normally.

## Installed library and account boundary

The current library reads locally installed games and saved app shortcuts. The
compatibility catalog is a separate list of public game identities and reports;
it is not an account's complete owned library. Asking Steam to install a title
hands its numeric AppID to the saved Windows Steam client in the chosen bottle.
Steam handles sign-in, ownership and downloads.

Full account-library sync needs a separate, explicit account integration.
[Steam OpenID](https://partner.steamgames.com/doc/features/auth#website) verifies
identity; it does not itself grant access to private game details.
[GetOwnedGames](https://partner.steamgames.com/doc/webapi/IPlayerService#GetOwnedGames)
requires a Web API key and returns a library only when it is visible to the
requester. Valve's documented
[OAuth registration](https://partner.steamgames.com/doc/webapi_overview/oauth)
requires a Valve-issued client ID and currently limits issuance to integrations
supporting cross-platform Cloud sync or Workshop. Caskrun therefore does not
present a generic Steam login as permission to fetch a private library, scrape
Steam session credentials, or bundle a shared API key. A future integration must
establish the supported authentication route, visibility limits, consent and
credential storage before adding account sync.

## Validation and updates

Builds validate the bundled JSON offline. The Swift loader accepts one format,
one version, one reviewed revision, an exact field allowlist, bounded text and
counts, valid dates, unique IDs and allowlisted HTTPS source-link shapes. A missing
or invalid catalog shows an error and leaves installed games usable.

The maximum snapshot is 1 MiB, 1,024 games and 4,096 reports. Updates are a reviewed
source change, never a downloaded executable or a silent background refresh.
To regenerate, fetch the complete Git tree plus only `db/games/*.json` and
`db/reports/*.jsonl` at the pinned revision into an ignored `.build/` research
directory, then run:

```sh
python3 -B scripts/generate-game-compatibility.py --source-dir .build/research/game-compatibility
python3 -B scripts/generate-game-compatibility.py --check
```

The generator verifies each source file against its Git blob hash and refuses a
changed, added or missing source file. It writes only the allowlisted metadata and
publishes the final catalog atomically. Changing the source revision also requires
reviewing the upstream licence, schema, filtered output, privacy gate and tests.

## Other projects reviewed

[MacGamingDB](https://github.com/neo773/macgamingdb) has MIT-licensed application
code and a public, documented REST surface with chip, renderer and performance
reports. Its production reviews, Steam/IGDB metadata and artwork have no separate
redistribution licence established in this review, so they are not bundled.
[Heroic](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher) and
[Soju](https://github.com/BCD1210/soju) are useful GPL-licensed examples of store
library adapters. Their Wine builds and compatibility claims do not establish
Caskrun support. Proton/Linux ratings also do not establish macOS compatibility.

## Verification boundary

Automated checks cover metadata removal, provenance, source pinning, bounded
decoding, malformed input, unknown device fields, unavailable renderer warnings
and the checked-in snapshot. The reports have not been reproduced through
Caskrun. Actual game testing remains necessary before adding Caskrun results.
