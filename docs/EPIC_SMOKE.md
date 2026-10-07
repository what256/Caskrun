# Epic Games Store installer smoke

_23 September 2026. Experimental local test on Apple Silicon; no compatibility rating._

The original `install-catalog epic-games-store` command used the ARM64 Caskrun core and the independent GnuTLS-enabled Wine engine bundled in the local development app. It resolved the current Windows MSI from [Epic's official installer endpoint](https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/installer/download/EpicGamesLauncherInstaller.msi), accepted only the reviewed Epic CDN host/path over HTTPS, checked the MSI compound-file structure, and recorded SHA-256. The downloaded MSI was **87,310,336 bytes**, SHA-256 `1513d6cc2afda0367c8375b6f25f490c162da5607ce4b4adbb41906a2d742236`. This digest identifies the tested bytes; it does not prove an Epic publisher signature.

The command created a disposable bottle under ignored `.build/epic-recipe-smoke/`, initialized its Wine prefix, and started `msiexec /i` with the publisher's interactive setup wizard. `msiexec` and a prefix-specific Wine server were running, but no `EpicGamesLauncher.exe` appeared during the bounded observation. The Mac was locked, so local UI inspection could not reach the wizard. After roughly five minutes without installer completion, the test core and only this bottle's Wine processes were stopped. The bottle, MSI, and Wine logs remain for diagnosis; no user data or installed CrossOver files were changed.

**Outcome:** installer download and startup passed; wizard completion, automatic launcher registration, Epic sign-in, game download, and play remain unverified. The SwiftUI Install action was implemented but not run in this test. Caskrun must show publisher prompts and verify a completed installer before claiming Epic is installed. See [Epic's launcher installation guide](https://www.epicgames.com/help/c-6465950763803/c-6465903530139/a202300000012839?lang=en-US) and [feature gaps](FEATURE_GAPS.md).


## 7 October 2026: current installer regression

A fresh isolated test downloaded EpicInstaller 20.3.3 from the reviewed official CDN: **264,237,056 bytes**, SHA-256 `d55d79710edfeaa107a62f2ea007bec1eeeac8b715a43c2ba3858eec7bc60337`. Interactive `msiexec /i` failed with an execute-access fault and installer exit code 5. No launcher was saved. This test does **not** establish an Epic installation or usable launcher.

A separate, original Caskrun MSI installed its exact test payload successfully with the same independent runtime in another disposable prefix. That narrows the regression beyond basic MSI startup, but does not identify the failing Epic component. The fault address resembled bytes from the installer path; path corruption remains an unproven diagnosis. The current Epic recipe also rejects empty, malformed, unsupported-architecture, and symbolic-link launcher candidates rather than reporting them as installed.

Both October test prefixes and their downloads/logs were stopped and removed after ownership and process checks. The older September fixture is retained separately. Current follow-up results belong in [launcher validation](LAUNCHER_VALIDATION.md); account authentication and gameplay remain untested.
