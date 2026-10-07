# Adaptive app icons

Caskrun converts the publisher artwork already in an installed EXE, local Steam game image, or checked catalog asset into macOS-sized artwork. These are Caskrun presentations of the original artwork, not newly supplied official macOS publisher icons. Caskrun’s own app and menu-bar logos are unchanged.

## Composition

The shared original MIT renderer samples a bounded 128-pixel image and checks alpha coverage, silhouette, colour variation and contrast:

- Finished rounded tiles and intentional circles keep their own shape and background.
- Opaque square artwork fills a rounded Mac mask directly, avoiding a tile inside another tile.
- Excess transparent padding is removed with an antialias margin; aspect ratio and publisher colours are retained.
- Sparse, low-variation monochrome or extremely light/dark marks receive a restrained contrasting plate with a small contribution from their original colour.
- Strong multicolour and ambiguous silhouettes remain standalone artwork. Classification is deliberately conservative.

Home, Discover and Steam game tiles display this composition directly without extra Steam-specific backgrounds, padding or clipping. Discover and game image conversion run outside the UI thread. Home can prepare an image-only cache before an app’s first launch; it creates neither a native app bundle nor a Windows process during listing. The same policy creates PNG/ICNS artwork for native Dock identities.

## Cache and engine boundaries

The app contains a signed ARM64 `caskrun-native-icon` sibling of `caskrun-core`. Only a validated generated Caskrun app selects that helper; a development CLI uses the recorded runtime helper. This separates icon changes from Wine updates: the current included native Windows engine remains byte-identical, and existing bottles need no engine migration for icon polish.

Image cache keys include the exact publisher ICO and renderer bytes. Stable source reads, retained prefix/cache guards, bounded conversion, exact source/output receipts and exclusive atomic publication reject modified or linked caches. A previous native fallback does not permanently conceal available publisher artwork. Runtime-less or failed conversion paths retain the original ICO where available. UI requests track source/name/renderer changes and discard stale responses, then retry the current request.

## Validation and limits

The real offscreen CLI passed 23 original synthetic-image cases: finished tile, circles, black/white/yellow marks, multicolour silhouettes, excess padding, crop orientation, opaque square, ICO/PNG, all native/Retina representations, malformed/empty/oversized inputs, FIFO, symbolic links, writable destinations and existing-output preservation. The slowest measured successful conversion was 0.365 seconds against the existing 1.1-second deadline; this is a sample on the development Mac, not a universal performance guarantee. Fixtures were removed after zero-owner checks.

Run `python3 scripts/test-native-icon-renderer.py` on macOS. Rust tests separately cover cache publication/invalidation/tampering, manual host EXE artwork, helper selection, fallback retry, concurrent requests and timeout cleanup. The native UI typecheck, signed app and development DMG build/verification passed. `scripts/test-packaged-artwork.py --app <owned-build/Caskrun.app>` also passed with an original MinGW PE icon: the packaged core selected its adaptive helper, produced Home artwork before launch, reused it for app/bottle requests and created no launch alias. Both owned fixture attempts were cleaned after zero-owner checks. The included runtime matched the existing native package byte for byte. These checks do not open vendor apps or validate their GUI.

This does not recover missing pixels, invent a vendor logo, or establish every publisher app’s Dock rendering/focus. Small source icons remain limited by their original detail. Actual vendor, manual app and child-game acceptance remains in [launcher validation](LAUNCHER_VALIDATION.md).
