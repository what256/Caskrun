# Ready-built GitHub updates

Caskrun uses the pinned Sparkle 2.10.0 macOS updater for ready-built application
archives. Users do not need Rust or Xcode to install an update. **Check for
Updates…** is in the native Caskrun menu; Collection's floating header stays
unchanged. Automatic checking uses Sparkle's consent flow. Automatic download
and installation are disabled.

## Current demo boundary

The updater integration and its configuration, URL, shutdown and file-owner
policies passed source and inert-fixture checks. There is no public signed
binary release or configured update feed yet. Current builds state that instead
of claiming to be up to date. A real install/relaunch and clean-Mac test remain
required before release delivery is called validated.

The local build is ad-hoc signed. No Developer ID signing identity is available
in the current build environment. Public release work must establish signing,
notarization and permission continuity, and complete the separate binary privacy
and runtime-license review described in [publication](PUBLICATION.md). Do not
publish the development DMG as an updater asset.

## Verification and user data

Release packaging sets both a public GitHub `SUFeedURL` and its 32-byte
`SUPublicEDKey`. Signed feeds and verification before extraction are mandatory.
App archives and delta files must belong to the same configured repository;
package installers and arbitrary download locations are refused. HTML release
notes are disabled. System profile fields are not sent, and no account token,
bottle contents, raw diagnostic log or personal identifier is attached to a
check. Ordinary network requests still go to GitHub's servers.

Before installation, Caskrun stops only its own Windows and PS5 sessions and
independently checks open handles on the app helpers and bundled Wine engine.
Its final termination delegate repeats the ownership check, including when
Sparkle skips its postpone callback. A failed stop or inspection leaves the app
in place and restores its usable state. Tests cover stale installer callbacks
and retry after an unsafe or failed stop.

An application update does not move or delete bottle storage. Sparkle replaces
the whole app bundle, so the publisher must keep the bundled engine's exact
content, modes and links unchanged for an app-only release. Recursive re-signing
of Wine can change its identity. Engine changes require a separately tested,
reviewed migration supported by the new core; they are not implicit app updates.

## Preparing a future release

1. Generate and protect a Sparkle Ed25519 signing key using the official SDK's
   `generate_keys` tool. Only the public key belongs in application metadata.
   Keep the private key outside Git and build logs, with a secure backup.
2. Build with a release version, increasing build number and trusted feed:

   ```sh
   CASKRUN_APP_VERSION=0.1.1 CASKRUN_APP_BUILD=100 \
     CASKRUN_UPDATE_FEED_URL="$REVIEWED_GITHUB_APPCAST_URL" \
     CASKRUN_UPDATE_PUBLIC_KEY="$PUBLIC_UPDATE_KEY" scripts/build.sh
   ```

3. Prepare the complete runtime-containing candidate using reviewed release
   packaging. Verify engine identity, signatures, architecture, corresponding
   runtime sources/notices, privacy and clean-machine behavior. The lightweight
   app build alone contains no Windows engine.
4. Sign and notarize the application, then create its update archive and signed
   appcast with the official SDK's `generate_appcast` tool. Verify the feed,
   archive signature and declared version. Publish only reviewed release assets
   to GitHub; the source publication hooks do not audit binary uploads.
5. Test update rejection for modified feeds/archives, interrupted download,
   active Windows sessions, failed shutdown and unavailable owner inspection.
   Confirm that one app reopens, permissions behave correctly, bottle contents
   remain intact and their unchanged engine records still validate.

The SDK cache lives only under ignored `.build/dependencies`. Its fetch helper
verifies the pinned SHA-256, extraction paths, links, file types and size bounds,
then rechecks cached bytes, modes and links. Core builds use the locked Cargo
dependency set. No third-party updater binaries are committed to Git.
