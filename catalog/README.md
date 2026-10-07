# Starter app catalog

`apps.json` is original Caskrun metadata for Windows-app setup. It links to publisher pages; it does not contain installers, copied icons, CrossOver recipes, or compatibility claims. All eight entries have original, experimental automatic setup recipes in the core. Every entry is marked `untested` because complete sign-in, library, and gameplay compatibility has not been validated.

## Format and integration

`schema.json` defines version 1. A Swift loader can decode the bundled `apps.json` as:

```swift
struct CatalogDocument: Decodable {
    let schemaVersion: Int
    let entries: [CatalogEntry]
}

struct CatalogEntry: Decodable, Identifiable {
    let id: String
    let name: String
    let category: String
    let symbol: String
    let detail: String
    let downloadPageURL: URL
    let compatibilityStatus: String
    let installMode: String?
    let installerNotes: String?
}
```

The SwiftUI loader rejects unsupported `schemaVersion` values, uses each stable `id`, and shows `untested` beside every entry. An Install action calls `caskrun-core install-catalog <id>`. Each recipe obtains a Windows installer from reviewed publisher-controlled HTTPS locations, checks format and size, records its SHA-256 for retry, creates a separate bottle, runs the installer, and saves a launcher only after finding a checked installed executable. Steam uses Valve's setup program and a guarded recovery path. Other publisher installers may still show their own terms and installation prompts. These digests identify downloaded bytes; they are not publisher signatures. Account credentials stay inside the publishers' clients. `symbol` remains Caskrun-owned metadata. Discover fetches publisher artwork at runtime; no third-party artwork is bundled. `scripts/build.sh` validates and bundles `apps.json` into `Contents/Resources/catalog/`.

`installerNotes` is optional. Automatic install flows are implemented in the core, not encoded as remotely writable catalog instructions. Categories are intentionally small and can be expanded in a later schema version. Each future automatic installer needs reviewed publisher provenance, download limits, test evidence, and its own core implementation.

Validate after edits with:

```sh
python3 catalog/validate.py
```

The validator uses only Python's standard library. It checks the schema's structure and field constraints, duplicate IDs/names, and HTTPS URL syntax. It does **not** fetch installers, prove that a page is publisher-owned, or test Wine compatibility. Review links against publisher sites before merging catalog changes.

## Discover artwork

Discover loads the eight catalog logos automatically, before installation. The fixed source list in `app/CaskrunApp.swift` uses publisher-controlled HTTPS image URLs discovered on the official pages below. It accepts only the expected image type and original host, blocks redirects, caps each response at 512 KiB and image dimensions at 2048 pixels, then caches the bytes in `~/Library/Caches/Caskrun/catalog-icons/`. Offline or unavailable images show original Caskrun text tiles. Once an app is installed, an icon extracted from its own Windows executable takes priority. Artwork loading does not start any installer or prove compatibility.

| Entry | Artwork source | Display detail |
| --- | --- | --- |
| Steam | [Valve favicon](https://store.steampowered.com/favicon.ico) | Replaced by its installed PE icon |
| Epic Games Store | [Epic Developer Community favicon](https://edc-cdn.net/assets/images/favicon-light.png) | Epic Games brand mark |
| GOG Galaxy | [GOG Galaxy site icon](https://www.gogalaxy.com/favicons/apple-icon-180x180.png) | Purple Galaxy mark |
| Battle.net | [Blizzard Battle.net logomark](https://blz-contentstack-images.akamaized.net/v3/assets/blt13393558c8f39060/bltc033281ebe085f52/6442f1f9fac1726ff56fc234/nexus_logomark.svg) | Caskrun crops the icon portion for its square tile |
| Ubisoft Connect | [Ubisoft site icon](https://static-dm.ubisoft.com/ubisoft/prod/favicon-195x195.png) | Ubisoft mark; Connect's own PE icon takes priority after install |
| EA app | [EA site logo](https://media.contentapi.ea.com/content/dam/ea/EA_logo.png) | EA mark; app's own PE icon takes priority after install |
| itch.io app | [itch.io app icon](https://static.itch.io/images/app-icon.png) | App-specific icon |
| Rockstar Games Launcher | [Rockstar site icon](https://media-rockstargames-com.akamaized.net/favicons/rockstar/favicon-180x180.png) | Rockstar mark |

The publishers own their logos and trademarks. Caskrun does not redistribute their files in its repository or app bundle. If a publisher changes an artwork URL or denies a request, the original text tile remains usable and the image can be retried on the next app launch.

## Publisher sources

Pages reviewed on 2026-09-23:

| Entry | Official source |
| --- | --- |
| Steam | [Valve's Steam install page](https://store.steampowered.com/about/) |
| Epic Games Store | [Epic's launcher download page](https://store.epicgames.com/en-US/download) |
| GOG Galaxy | [GOG Galaxy page](https://www.gog.com/galaxy) |
| Battle.net | [Battle.net desktop download page](https://download.battle.net/en-us/desktop) |
| Ubisoft Connect | [Ubisoft Connect PC download page](https://www.ubisoft.com/en-us/ubisoft-connect/download) |
| EA app | [EA app page](https://www.ea.com/ea-app) and [EA's installation guide](https://help.ea.com/en/articles/platforms/ea-app-download-install-update/) |
| itch.io app | [itch.io app page](https://itch.io/app) and [Windows installation guide](https://itch.io/docs/itch/installing/windows.html) |
| Rockstar Games Launcher | [Rockstar Support's launcher download article](https://support.rockstargames.com/articles/4extB4aITvMKdDEZzsFAwE/rockstar-games-launcher-download) |

Several publisher pages offer native Mac and Windows versions. Caskrun's automatic recipes explicitly request the **Windows** installer. The catalog URL is a human-facing publisher page; the reviewed download URL and validation belong to each core recipe because publishers can change their delivery paths.

On 23 September 2026, the direct Rockstar publisher CDN installer URL returned a Windows installer response from this Mac. The real Rockstar installer has not been run in Wine, so its setup, sign-in, and games remain unverified.
