# Browser game links

`browser-games.json` contains original Caskrun link metadata. The native app
does not embed these games, bundle their artwork or assets, import accounts,
download commercial game data, or run their websites in Wine. An explicit
**Play in browser** action opens the checked HTTPS link in the user's default
browser. **Add to Home** saves a local bookmark. A user can also add a different
public HTTPS game page.

## Provenance

| Entry | Developer or project page | Scope |
| --- | --- | --- |
| Moon Zombies | [Browser game](https://moon-zombies.pages.dev/) | Community browser shooter; unrelated to an official commercial release. |
| Surfd | [Developer website](https://surfd.net/) | Browser game with keyboard and mouse controls. |
| Vice City community port | [Community port page](https://joncodeofficial.github.io/gta-vice-city-wasm/) | Explicitly requires game files the user already owns. The page identifies the port as unaffiliated with Rockstar Games. Caskrun provides only its link. |
| Hextris | [Original project](https://github.com/Hextris/hextris) | The project README links to `hextris.io`. Caskrun does not incorporate its GPL game code or assets. |
| 2048 | [Creator's about page](https://play2048.co/about) | The creator identifies `play2048.co` as the original website. Caskrun provides only a link. |

The community browser examples were identified through a user-supplied
[public post](https://x.com/RadiantOpti/status/2108068628793082338) and checked
against the linked pages. A website loading does not establish gameplay,
performance, device support, publisher affiliation or future availability.
Browser sites control their own storage, privacy, accounts and requirements.

## Validation and privacy

`validate_browser_games.py` checks exact fields, stable IDs, bounded metadata,
duplicate links and conservative HTTPS URL syntax. It never contacts a site.
The own-files flag must remain visible before opening or saving the community
Vice City port.

Saved bookmarks are kept in Caskrun's own per-user data, outside bottle backups
and diagnostic reports. They are not uploaded automatically. Names and URLs
are bounded; files use private permissions, checked reads and atomic writes.
Account-bearing query or fragment keys such as `access_token`, `session`,
`password` and `email` are refused, including case and percent-escape variants.
This is a conservative input check, not a guarantee that an arbitrary website
link contains no private data. Users should save the public game page.

```sh
python3 -B catalog/validate_browser_games.py
python3 -B -m unittest discover -s catalog -p 'test_browser_games.py'
```
