//! Reviewed exact-package transitions for Caskrun bottles. Historical Steam
//! transitions retain their launcher gates; reviewed native/switchable packages
//! can advance while idle. Other changes retain the ordinary exact match.

use super::*;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

// Complete package-tree SHA-256 values produced by runtime_identity.rs. The
// TLS and Steam values preserve the original reviewed transition. The native
// successor was measured by this module's complete package attestation after
// an independently packaged runtime initialized a fresh disposable prefix.
const TLS_WINE_11_IDENTITY: &str =
    "sha256:6df2a9f5dfaa119ba3d6c84214d79f885c70e887ef7f3bc9d2255aeb8b2f88e6";
const STEAM_WINE_11_IDENTITY: &str =
    "sha256:07c5721ad17efa9d4ea89ff61522133407756c4aea8a47866625dcf118e7347c";
const NATIVE_WINE_11_IDENTITY: &str =
    "sha256:9dcab17f21490998ad03f6ee4f553da0992a1d9234dfe40efd82956b63fd47ce";
// Complete switchable package measured by the ordinary tree attestation.
pub(super) const SWITCHABLE_WINE_11_IDENTITY: &str =
    "sha256:4d82b50b96eb5995a78d848fb67166979577cd98e36d7db72fbb08636f8f44c5";
// Complete package measured after rebuilding both Wine driver views, signing
// the changed Unix drivers and retaining every unrelated engine file. The
// original native text command bridge adds no translator or game claim.
pub(super) const NATIVE_TEXT_EDITING_WINE_11_IDENTITY: &str =
    "sha256:4f736f73c87adeb030507e2f2fcb74989fbd3460ddf750e3b5d47f8f2d6f3039";
// Locally deployed signed package with the reviewed menu handshake and Steam
// sign-in controls. Its native header obscured the client image; retain its
// exact identity so existing bottles can advance without a manual record edit.
pub(super) const REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY: &str =
    "sha256:f3c98358ec68997c3c7f5ed10e406a20e34022bbd8f17fd0827ce21086b16886";
// Complete signed package with bounded native-header painting, independently
// measured by this module's full tree attestation. This preserves the existing
// renderer protocol; identity approval is not a game-compatibility claim.
pub(super) const BOUNDED_NATIVE_UI_WINE_11_IDENTITY: &str =
    "sha256:744741740221a92dc10849620661801dcbf3361fdda5fbbc3bf4bca6a6f252a1";
// Complete signed package measured after rebuilding both native child loaders
// and Wine driver views. Only the verified Steam helper reuses its prepared
// bundle; translators and the renderer protocol remain unchanged.
pub(super) const BUNDLED_STEAM_UI_WINE_11_IDENTITY: &str =
    "sha256:d8bd4eb824d7ad1b340cbd32ae70f04cdeafb7a1d6016ee4754e472699e61ec8";

// Complete signed package independently measured by the full Rust tree
// attestation. Only the visible Steam browser helper receives its prepared
// bundle; background helpers keep the raw loader. The existing graphics
// translator protocol and native text/header drivers remain unchanged.
pub(super) const SINGLE_STEAM_UI_WINE_11_IDENTITY: &str =
    "sha256:aae295ed019afb077ebb6e58d90445d7cbaf0b3a77fe69dc28f42683622715b9";

// Complete signed dual-view package independently rebuilt and attested after
// adding the bounded captured Steam text-menu handshake. Query state carries
// the original GUI focus and exact popup; renderer protocol ordinals and
// graphics translators remain unchanged. This is not a gameplay claim.
pub(super) const CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY: &str =
    "sha256:46998fc3cf95d882e14c3b5978dba2ee086c5b92e826400fda528517f3808082";

// Complete signed dual-view package independently rebuilt after correcting the
// captured Chromium menu recognizer. Chromium toggles the layered style while
// its menu is shown, so only tool-window and no-activate are required; the
// transparent style, exact class, popup shape, direct ownership, GUI thread,
// capture, focus and serial guards are unchanged. This is not a gameplay claim.
pub(super) const RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY: &str =
    "sha256:fb91add5bbcdddef39738259595c2da592bed8afb863fccd6384ae6b1337a5ee";

/// Capability proof stays tied to explicitly reviewed complete packages. A
/// component receipt or an engine's major version cannot add a new package.
pub(super) fn is_reviewed_switchable_runtime(identity: &str) -> bool {
    matches!(
        identity,
        SWITCHABLE_WINE_11_IDENTITY
            | NATIVE_TEXT_EDITING_WINE_11_IDENTITY
            | REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY
            | BOUNDED_NATIVE_UI_WINE_11_IDENTITY
            | BUNDLED_STEAM_UI_WINE_11_IDENTITY
            | SINGLE_STEAM_UI_WINE_11_IDENTITY
            | CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
            | RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
    )
}

const RECORDED_DISPATCH: &str = "Caskrun.app/Contents/Resources/wine-runtime/bin/_dispatch";
const SELECTED_WINE: &str = "Caskrun.app/Contents/Resources/wine-runtime/bin/wine";

struct MigrationPolicy<'a> {
    previous: &'a str,
    approved: &'a str,
    backup_suffix: &'a str,
    verify_launcher_pe: bool,
}

const TLS_TO_STEAM_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: STEAM_WINE_11_IDENTITY,
    backup_suffix: "tls-to-steam",
    verify_launcher_pe: false,
};

const STEAM_TO_NATIVE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: NATIVE_WINE_11_IDENTITY,
    backup_suffix: "steam-to-native",
    verify_launcher_pe: true,
};

// Skipping the intervening app release needs its own reviewed exact edge.
// It never adopts an arbitrary successor or rewrites the TLS record twice.
const TLS_TO_NATIVE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: NATIVE_WINE_11_IDENTITY,
    backup_suffix: "tls-to-native",
    verify_launcher_pe: true,
};

// Steam bottles can skip directly from either older reviewed package to the
// switchable package. These are separate, Steam-gated edges, not a general
// upgrade of every bottle using those older packages.
const STEAM_TO_SWITCHABLE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: SWITCHABLE_WINE_11_IDENTITY,
    backup_suffix: "steam-to-switchable",
    verify_launcher_pe: true,
};

const TLS_TO_SWITCHABLE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: SWITCHABLE_WINE_11_IDENTITY,
    backup_suffix: "tls-to-switchable",
    verify_launcher_pe: true,
};

const STEAM_TO_NATIVE_TEXT_EDITING_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "steam-to-native-text-editing",
    verify_launcher_pe: true,
};

const TLS_TO_NATIVE_TEXT_EDITING_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "tls-to-native-text-editing",
    verify_launcher_pe: true,
};

const STEAM_TO_REFINED_NATIVE_TEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "steam-to-refined-native-ui",
    verify_launcher_pe: true,
};

const TLS_TO_REFINED_NATIVE_TEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "tls-to-refined-native-ui",
    verify_launcher_pe: true,
};

const STEAM_TO_BOUNDED_NATIVE_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    backup_suffix: "steam-to-bounded-native-ui",
    verify_launcher_pe: true,
};

const TLS_TO_BOUNDED_NATIVE_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    backup_suffix: "tls-to-bounded-native-ui",
    verify_launcher_pe: true,
};

const STEAM_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "steam-to-bundled-steam-ui",
    verify_launcher_pe: true,
};

const TLS_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "tls-to-bundled-steam-ui",
    verify_launcher_pe: true,
};

const TLS_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "tls-to-single-steam-ui",
    verify_launcher_pe: true,
};

const STEAM_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "steam-to-single-steam-ui",
    verify_launcher_pe: true,
};

const TLS_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
    backup_suffix: "tls-to-captured-text-context",
    verify_launcher_pe: true,
};

const STEAM_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
    backup_suffix: "steam-to-captured-text-context",
    verify_launcher_pe: true,
};

const TLS_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: TLS_WINE_11_IDENTITY,
    approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
    backup_suffix: "tls-to-recognized-text-popup",
    verify_launcher_pe: true,
};

const STEAM_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: STEAM_WINE_11_IDENTITY,
    approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
    backup_suffix: "steam-to-recognized-text-popup",
    verify_launcher_pe: true,
};

const STEAM_RUNTIME_POLICIES: [&MigrationPolicy<'static>; 19] = [
    &TLS_TO_STEAM_POLICY,
    &STEAM_TO_NATIVE_POLICY,
    &TLS_TO_NATIVE_POLICY,
    &STEAM_TO_SWITCHABLE_POLICY,
    &TLS_TO_SWITCHABLE_POLICY,
    &STEAM_TO_NATIVE_TEXT_EDITING_POLICY,
    &TLS_TO_NATIVE_TEXT_EDITING_POLICY,
    &STEAM_TO_REFINED_NATIVE_TEXT_POLICY,
    &TLS_TO_REFINED_NATIVE_TEXT_POLICY,
    &STEAM_TO_BOUNDED_NATIVE_UI_POLICY,
    &TLS_TO_BOUNDED_NATIVE_UI_POLICY,
    &STEAM_TO_BUNDLED_STEAM_UI_POLICY,
    &TLS_TO_BUNDLED_STEAM_UI_POLICY,
    &STEAM_TO_SINGLE_STEAM_UI_POLICY,
    &TLS_TO_SINGLE_STEAM_UI_POLICY,
    &TLS_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &STEAM_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &TLS_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &STEAM_TO_RECOGNIZED_TEXT_POPUP_POLICY,
];

const NATIVE_TO_SWITCHABLE_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: SWITCHABLE_WINE_11_IDENTITY,
    backup_suffix: "native-to-switchable",
    verify_launcher_pe: false,
};

// Direct reviewed edges let an older app skip releases without temporarily
// binding the bottle to a package that is no longer installed.
const NATIVE_TO_NATIVE_TEXT_EDITING_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "native-to-native-text-editing",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_NATIVE_TEXT_EDITING_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-native-text-editing",
    verify_launcher_pe: false,
};

const NATIVE_TO_REFINED_NATIVE_TEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "native-to-refined-native-ui",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_REFINED_NATIVE_TEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-refined-native-ui",
    verify_launcher_pe: false,
};

const NATIVE_TEXT_TO_REFINED_NATIVE_TEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    backup_suffix: "native-text-editing-to-refined-native-ui",
    verify_launcher_pe: false,
};

const NATIVE_TO_BOUNDED_NATIVE_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    backup_suffix: "native-to-bounded-native-ui",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_BOUNDED_NATIVE_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-bounded-native-ui",
    verify_launcher_pe: false,
};

const NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    backup_suffix: "native-text-editing-to-bounded-native-ui",
    verify_launcher_pe: false,
};

const REFINED_NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    backup_suffix: "refined-native-ui-to-bounded-native-ui",
    verify_launcher_pe: false,
};

const NATIVE_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "native-to-bundled-steam-ui",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-bundled-steam-ui",
    verify_launcher_pe: false,
};

const NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "native-text-editing-to-bundled-steam-ui",
    verify_launcher_pe: false,
};

const REFINED_NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "refined-native-ui-to-bundled-steam-ui",
    verify_launcher_pe: false,
};

const BOUNDED_NATIVE_UI_TO_BUNDLED_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    approved: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "bounded-native-ui-to-bundled-steam-ui",
    verify_launcher_pe: false,
};

const NATIVE_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "native-to-single-steam-ui",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-single-steam-ui",
    verify_launcher_pe: false,
};

const NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "native-text-editing-to-single-steam-ui",
    verify_launcher_pe: false,
};

const REFINED_NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "refined-native-ui-to-single-steam-ui",
    verify_launcher_pe: false,
};

const BOUNDED_NATIVE_UI_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "bounded-native-ui-to-single-steam-ui",
    verify_launcher_pe: false,
};

const BUNDLED_STEAM_UI_TO_SINGLE_STEAM_UI_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
    approved: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    backup_suffix: "bundled-steam-ui-to-single-steam-ui",
    verify_launcher_pe: false,
};

const NATIVE_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
    backup_suffix: "native-to-captured-text-context",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-captured-text-context",
    verify_launcher_pe: false,
};

const NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
    backup_suffix: "native-text-editing-to-captured-text-context",
    verify_launcher_pe: false,
};

const REFINED_NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
        approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
        backup_suffix: "refined-native-ui-to-captured-text-context",
        verify_launcher_pe: false,
    };

const BOUNDED_NATIVE_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
        approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
        backup_suffix: "bounded-native-ui-to-captured-text-context",
        verify_launcher_pe: false,
    };

const BUNDLED_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
        approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
        backup_suffix: "bundled-steam-ui-to-captured-text-context",
        verify_launcher_pe: false,
    };

const SINGLE_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    approved: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
    backup_suffix: "single-steam-ui-to-captured-text-context",
    verify_launcher_pe: false,
};

const NATIVE_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_WINE_11_IDENTITY,
    approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
    backup_suffix: "native-to-recognized-text-popup",
    verify_launcher_pe: false,
};

const SWITCHABLE_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SWITCHABLE_WINE_11_IDENTITY,
    approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
    backup_suffix: "switchable-to-recognized-text-popup",
    verify_launcher_pe: false,
};

const NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
    approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
    backup_suffix: "native-text-editing-to-recognized-text-popup",
    verify_launcher_pe: false,
};

const REFINED_NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
        approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
        backup_suffix: "refined-native-ui-to-recognized-text-popup",
        verify_launcher_pe: false,
    };

const BOUNDED_NATIVE_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
        approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
        backup_suffix: "bounded-native-ui-to-recognized-text-popup",
        verify_launcher_pe: false,
    };

const BUNDLED_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: BUNDLED_STEAM_UI_WINE_11_IDENTITY,
        approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
        backup_suffix: "bundled-steam-ui-to-recognized-text-popup",
        verify_launcher_pe: false,
    };

const SINGLE_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> = MigrationPolicy {
    previous: SINGLE_STEAM_UI_WINE_11_IDENTITY,
    approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
    backup_suffix: "single-steam-ui-to-recognized-text-popup",
    verify_launcher_pe: false,
};

const CAPTURED_TEXT_CONTEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY: MigrationPolicy<'static> =
    MigrationPolicy {
        previous: CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
        approved: RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
        backup_suffix: "captured-text-context-to-recognized-text-popup",
        verify_launcher_pe: false,
    };

const BUNDLED_RUNTIME_POLICIES: [&MigrationPolicy<'static>; 36] = [
    &NATIVE_TO_SWITCHABLE_POLICY,
    &NATIVE_TO_NATIVE_TEXT_EDITING_POLICY,
    &SWITCHABLE_TO_NATIVE_TEXT_EDITING_POLICY,
    &NATIVE_TO_REFINED_NATIVE_TEXT_POLICY,
    &SWITCHABLE_TO_REFINED_NATIVE_TEXT_POLICY,
    &NATIVE_TEXT_TO_REFINED_NATIVE_TEXT_POLICY,
    &NATIVE_TO_BOUNDED_NATIVE_UI_POLICY,
    &SWITCHABLE_TO_BOUNDED_NATIVE_UI_POLICY,
    &NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY,
    &REFINED_NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY,
    &NATIVE_TO_BUNDLED_STEAM_UI_POLICY,
    &SWITCHABLE_TO_BUNDLED_STEAM_UI_POLICY,
    &NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY,
    &REFINED_NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY,
    &BOUNDED_NATIVE_UI_TO_BUNDLED_STEAM_UI_POLICY,
    &NATIVE_TO_SINGLE_STEAM_UI_POLICY,
    &SWITCHABLE_TO_SINGLE_STEAM_UI_POLICY,
    &NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY,
    &REFINED_NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY,
    &BOUNDED_NATIVE_UI_TO_SINGLE_STEAM_UI_POLICY,
    &BUNDLED_STEAM_UI_TO_SINGLE_STEAM_UI_POLICY,
    &NATIVE_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &SWITCHABLE_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &REFINED_NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &BOUNDED_NATIVE_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &BUNDLED_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &SINGLE_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
    &NATIVE_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &SWITCHABLE_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &REFINED_NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &BOUNDED_NATIVE_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &BUNDLED_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &SINGLE_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
    &CAPTURED_TEXT_CONTEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
];

/// Returned only after the old record has been backed up and replaced. The
/// caller may restore it if prelaunch verification/configuration fails. This
/// restores only the runtime record, never changes made after Wine starts.
pub(super) struct RecordRollback {
    record_path: PathBuf,
    backup_path: PathBuf,
    original_bytes: Vec<u8>,
    migrated_bytes: Vec<u8>,
}

impl RecordRollback {
    pub(super) fn restore(self) -> Result<(), String> {
        if read_record_bytes(&self.backup_path)? != self.original_bytes {
            return Err(format!(
                "Bottle runtime backup changed; restore the bottle from {}",
                self.backup_path.display()
            ));
        }
        if read_record_bytes(&self.record_path)? != self.migrated_bytes {
            return Err(format!(
                "Bottle runtime record changed; review its backup at {}",
                self.backup_path.display()
            ));
        }
        replace_record_bytes(&self.record_path, &self.original_bytes)
    }
}

/// Called while the launcher's per-bottle lock is held, before Wine starts.
/// This hook is intentionally absent from generic initialize, tools, and
/// custom launch paths.
pub(super) fn maybe_migrate_steam_runtime(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
) -> Result<Option<RecordRollback>, String> {
    migrate_with_policies(
        home,
        bottle_id,
        wine,
        launcher_path,
        &STEAM_RUNTIME_POLICIES,
        WINE_SERVER_CHECK_TIMEOUT,
    )
}

/// Called under the bottle lock, before any Wine command. Only the reviewed
/// complete native/switchable packages can advance to reviewed successors.
/// No prefix content is modified, and the returned rollback is record-only.
/// Ordinary tracked-process cleanup and managed XDG preparation still run
/// during preflight; the rollback does not reverse that routine bookkeeping.
pub(super) fn maybe_migrate_bundled_runtime(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
) -> Result<Option<RecordRollback>, String> {
    migrate_bundled_with_policies(
        home,
        bottle_id,
        wine,
        &BUNDLED_RUNTIME_POLICIES,
        WINE_SERVER_CHECK_TIMEOUT,
    )
}

fn migrate_bundled_with_policies(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    policies: &[&MigrationPolicy<'_>],
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    let Some(rollback) = migrate_runtime(home, bottle_id, wine, None, policies, idle_timeout)?
    else {
        return Ok(None);
    };
    // Re-read the installed record and use the same verifier as ordinary
    // launches. A package changed during the idle check must not be adopted.
    let verified: Result<(), String> = (|| {
        let bytes = read_record_bytes(&rollback.record_path)?;
        let record: PrefixRecord = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid migrated bottle runtime record: {e}"))?;
        if !runtime_record_matches(&record, wine)? {
            return Err("The migrated runtime failed ordinary verification".into());
        }
        Ok(())
    })();
    if let Err(error) = verified {
        return match rollback.restore() {
            Ok(()) => Err(format!("{error}; the original runtime record was restored")),
            Err(restore_error) => Err(format!("{error}; {restore_error}")),
        };
    }
    Ok(Some(rollback))
}

#[cfg(test)]
fn migrate_bundled_with_policy(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    policy: &MigrationPolicy<'_>,
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    migrate_bundled_with_policies(home, bottle_id, wine, &[policy], idle_timeout)
}

fn migrate_with_policies(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
    policies: &[&MigrationPolicy<'_>],
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    migrate_runtime(
        home,
        bottle_id,
        wine,
        Some(launcher_path),
        policies,
        idle_timeout,
    )
}

fn migrate_runtime(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: Option<&Path>,
    policies: &[&MigrationPolicy<'_>],
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    let dir = bottle_dir(home, bottle_id)?;
    let prefix = dir.join("prefix");
    if !path_is_real_file(&prefix.join("system.reg")) {
        return Ok(None);
    }
    let (prefix_guard, prefix_snapshot) = open_stable_directory(&prefix)?;
    let record_path = dir.join("prefix-record.json");
    // Imported prefixes can have a registry before Caskrun records an engine.
    // Their normal initialization path decides how to adopt that prefix.
    if launcher_path.is_none()
        && matches!(fs::symlink_metadata(&record_path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let original_bytes = read_record_bytes(&record_path)?;
    let record: PrefixRecord = serde_json::from_slice(&original_bytes)
        .map_err(|e| format!("Invalid bottle runtime record: {e}"))?;
    let matching: Vec<_> = policies
        .iter()
        .copied()
        .filter(|policy| record.runtime_identity.as_deref() == Some(policy.previous))
        .collect();
    if matching.is_empty() {
        return Ok(None);
    }
    // A custom Wine path, a legacy path-only record, and every other package
    // mismatch keep the existing strict behavior.
    if !Path::new(&record.wine_path).is_absolute()
        || !Path::new(&record.wine_path).ends_with(RECORDED_DISPATCH)
        || !wine.is_absolute()
        || !wine.ends_with(SELECTED_WINE)
        || !record
            .runtime_fingerprint
            .as_deref()
            .is_some_and(bound_fingerprint_shape)
    {
        return Ok(None);
    }
    if let Some(launcher_path) = launcher_path {
        let Some(attempt) = steam_install::catalog_attempt(home, bottle_id)? else {
            return Ok(None);
        };
        if attempt.catalog_id != "steam" || attempt.stage != steam_install::CatalogStage::Installed
        {
            return Ok(None);
        }
        let launchers = list_launchers(home, bottle_id)?;
        if !launchers.iter().any(|launcher| {
            Path::new(&launcher.path)
                .canonicalize()
                .is_ok_and(|path| path == launcher_path)
                && steam_install::is_recipe_steam_launcher(home, bottle_id, launcher)
        }) {
            return Ok(None);
        }
    }
    if !is_usable_wine(wine) {
        return Err("The approved Caskrun Windows engine is unavailable".into());
    }
    // Select by both pinned endpoints after hashing the complete destination.
    // Multiple successors for one predecessor must not cause the first edge
    // to reject a different explicitly reviewed edge, or hash the tree twice.
    let attestation = runtime_identity::bundled_runtime_attestation(wine)?
        .ok_or("The selected Windows engine is not a complete Caskrun package")?;
    if record.runtime_identity.as_deref() == Some(attestation.identity.as_str()) {
        return Ok(None);
    }
    let policy = matching
        .into_iter()
        .find(|policy| attestation.identity == policy.approved)
        .ok_or("The selected Windows engine is not the approved migration package; this bottle was unchanged")?;
    // New native transitions additionally validate the installed program's PE
    // content. The established TLS-to-Steam policy retains its original gate.
    let verified_launcher = if policy.verify_launcher_pe {
        let launcher_path =
            launcher_path.ok_or("This engine transition requires a Steam launcher")?;
        let before = fs::symlink_metadata(launcher_path)
            .map_err(|e| format!("Could not inspect the installed Steam launcher: {e}"))?;
        if !before.file_type().is_file() {
            return Err("The installed Steam launcher must be a regular Windows program".into());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(launcher_path)
            .map_err(|e| format!("Could not safely open the installed Steam launcher: {e}"))?;
        steam_install::inspect_pe_and_sha256(&mut file)
            .map_err(|e| format!("Could not verify the installed Steam launcher: {e}"))?;
        if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("The installed Steam launcher changed during verification".into());
        }
        Some((launcher_path, file, before))
    } else {
        None
    };
    refuse_running_bottle(&dir)?;
    wait_for_idle_wineserver(&dir, wine, idle_timeout, launcher_path.is_some())?;
    // A server can exit while an orphaned client retains this exact bottle's
    // files. The read-only handle guard never signals an inventoried process.
    prefix_processes::wait_until_idle(&dir, idle_timeout).map_err(|error| {
        format!("Could not verify this bottle is idle before changing its engine: {error}")
    })?;
    refuse_running_bottle(&dir)?;
    if !same_identity(
        &prefix_snapshot,
        &prefix_guard.metadata().map_err(|e| e.to_string())?,
    ) || !same_identity(
        &prefix_snapshot,
        &fs::symlink_metadata(&prefix).map_err(|e| e.to_string())?,
    ) {
        return Err("Bottle prefix changed during engine migration".into());
    }
    if let Some((launcher_path, file, snapshot)) = verified_launcher
        && (!same_snapshot(&snapshot, &file.metadata().map_err(|e| e.to_string())?)
            || !same_snapshot(
                &snapshot,
                &fs::symlink_metadata(launcher_path).map_err(|e| e.to_string())?,
            ))
    {
        return Err("The installed Steam launcher changed during migration".into());
    }
    if read_record_bytes(&record_path)? != original_bytes {
        return Err("Bottle runtime record changed during migration".into());
    }

    let backups = real_subdirectory(home, "runtime-migration-backups")?;
    let backup_path = backups.join(format!(
        "{bottle_id}-{}-prefix-record.json",
        policy.backup_suffix
    ));
    retain_original_record(&backup_path, &original_bytes)?;
    let migrated = PrefixRecord {
        wine_path: wine
            .canonicalize()
            .map_err(|e| e.to_string())?
            .display()
            .to_string(),
        runtime_identity: Some(attestation.identity.clone()),
        runtime_fingerprint: Some(runtime_identity::bind_record_fingerprint(
            &attestation.identity,
            &attestation.metadata_fingerprint,
        )),
    };
    let migrated_bytes = serde_json::to_vec_pretty(&migrated).map_err(|e| e.to_string())?;
    replace_record_bytes(&record_path, &migrated_bytes)?;
    Ok(Some(RecordRollback {
        record_path,
        backup_path,
        original_bytes,
        migrated_bytes,
    }))
}

#[cfg(test)]
fn migrate_with_policy(
    home: &Path,
    bottle_id: &str,
    wine: &Path,
    launcher_path: &Path,
    policy: &MigrationPolicy<'_>,
    idle_timeout: Duration,
) -> Result<Option<RecordRollback>, String> {
    migrate_with_policies(
        home,
        bottle_id,
        wine,
        launcher_path,
        &[policy],
        idle_timeout,
    )
}

fn bound_fingerprint_shape(value: &str) -> bool {
    value
        .strip_prefix("sha256-bound-v2:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn read_record_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let before = fs::symlink_metadata(path)
        .map_err(|e| format!("Could not inspect bottle runtime record: {e}"))?;
    if !before.file_type().is_file() || before.len() > MAX_PREFIX_RECORD_BYTES {
        return Err("Bottle runtime record must be a bounded regular file".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_ANY)
        .open(path)
        .map_err(|e| format!("Could not safely open bottle runtime record: {e}"))?;
    if !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Bottle runtime record changed while opening it".into());
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_PREFIX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PREFIX_RECORD_BYTES
        || !same_snapshot(&before, &file.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Bottle runtime record changed while reading it".into());
    }
    Ok(bytes)
}

fn retain_original_record(path: &Path, original: &[u8]) -> Result<(), String> {
    retain_original_record_with_writer(path, original, |file, bytes| {
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    })
}

fn retain_original_record_with_writer(
    path: &Path,
    original: &[u8],
    write_backup: impl FnOnce(&mut File, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            if read_record_bytes(path)? != original {
                return Err(format!(
                    "Existing Steam runtime backup differs from this bottle's record: {}",
                    path.display()
                ));
            }
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Could not inspect Steam runtime backup: {e}")),
    }
    let parent = path
        .parent()
        .ok_or("Steam runtime backup has no parent directory")?;
    let (parent_guard, parent_snapshot) = open_stable_directory(parent)?;
    let backup_name = path
        .file_name()
        .ok_or("Steam runtime backup has no file name")?;
    let temporary_name = format!(".steam-runtime-backup-{}.tmp", Uuid::new_v4());
    let temporary_path = parent.join(&temporary_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temporary_path)
            .map_err(|e| format!("Could not create Steam runtime backup: {e}"))?;
        write_backup(&mut file, original)
            .map_err(|e| format!("Could not finish Steam runtime backup: {e}"))?;
        if !same_identity(
            &parent_snapshot,
            &fs::symlink_metadata(parent).map_err(|e| e.to_string())?,
        ) {
            return Err("Steam runtime backup directory changed while writing".into());
        }
        let from = CString::new(temporary_name).map_err(|e| e.to_string())?;
        let to = CString::new(backup_name.as_bytes()).map_err(|e| e.to_string())?;
        let moved = unsafe {
            renameatx_np(
                parent_guard.as_raw_fd(),
                from.as_ptr(),
                parent_guard.as_raw_fd(),
                to.as_ptr(),
                RENAME_EXCL | RENAME_NOFOLLOW_ANY,
            )
        };
        if moved != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::AlreadyExists
                && read_record_bytes(path)? == original
            {
                return Ok(());
            }
            return Err(format!("Could not publish Steam runtime backup: {error}"));
        }
        parent_guard
            .sync_all()
            .map_err(|e| format!("Could not sync Steam runtime backup directory: {e}"))
    })();
    // An interrupted write never becomes the durable backup that authorizes
    // replacing the prefix record. The unique staging file is disposable.
    let _ = fs::remove_file(&temporary_path);
    result
}

fn replace_record_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("migration-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW_ANY)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn wait_for_idle_wineserver(
    dir: &Path,
    wine: &Path,
    timeout: Duration,
    steam: bool,
) -> Result<(), String> {
    let wineserver = wine
        .parent()
        .ok_or("Wine path has no parent")?
        .join("wineserver");
    if !is_executable(&wineserver) || !is_independent_runtime(&wineserver) {
        return Err("Cannot check whether the bottle's Wine server is idle".into());
    }
    let prefix = dir.join("prefix");
    let xdg = bottle_xdg::for_bottle(dir)?;
    let mut probe = Command::new(wineserver);
    xdg.apply(&mut probe);
    let mut probe = probe
        .arg("-w")
        .env("WINEPREFIX", prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not check this bottle's Wine server: {e}"))?;
    let (status, timed_out) = wait_child_bounded_status(&mut probe, timeout)?;
    if timed_out {
        return Err(if steam {
            "Close Steam and other Windows apps in this bottle before upgrading its engine".into()
        } else {
            "Close Windows apps in this bottle before upgrading its engine".into()
        });
    }
    if !status.success() {
        return Err("Could not verify that this bottle's Wine server is idle".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        home: PathBuf,
        bottle_id: String,
        wine: PathBuf,
        steam: PathBuf,
        record_path: PathBuf,
        backup_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data");
            fs::create_dir_all(&parent).unwrap();
            let home = parent.join(format!("runtime-migration-{}", Uuid::new_v4()));
            let bottle = create(&home, "Steam").unwrap();
            let root = home.join("Caskrun.app/Contents/Resources/wine-runtime");
            fs::create_dir_all(root.join("bin")).unwrap();
            fs::create_dir_all(root.join("runtime/bin")).unwrap();
            fs::create_dir_all(root.join("lib")).unwrap();
            fs::write(root.join("SOURCES.txt"), b"test package\n").unwrap();
            fs::write(root.join("lib/libfreetype.6.dylib"), b"test freetype").unwrap();
            for name in ["wine", "wineboot", "wineserver"] {
                fs::write(root.join("runtime/bin").join(name), b"test engine").unwrap();
                symlink("_dispatch", root.join("bin").join(name)).unwrap();
            }
            let dispatch = root.join("bin/_dispatch");
            fs::write(
                &dispatch,
                b"#!/bin/sh\ncase \"${0##*/}\" in wineboot) touch \"$WINEPREFIX/system.reg\";; wineserver) [ \"$1\" = -w ] || exit 1;; esac\nexit 0\n",
            )
            .unwrap();
            let mut mode = fs::metadata(&dispatch).unwrap().permissions();
            mode.set_mode(0o755);
            fs::set_permissions(&dispatch, mode).unwrap();
            let wine = root.join("bin/wine");
            ensure_prefix_with_wine(&home, &bottle.id, &wine).unwrap();
            let dir = bottle_dir(&home, &bottle.id).unwrap();
            let prefix = dir.join("prefix");
            let steam = prefix.join("drive_c/Program Files (x86)/Steam/steam.exe");
            fs::create_dir_all(steam.parent().unwrap()).unwrap();
            let mut steam_bytes = vec![0_u8; 100 * 1024];
            steam_bytes[..2].copy_from_slice(b"MZ");
            steam_bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
            steam_bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
            steam_bytes[0x84..0x86].copy_from_slice(&0x014c_u16.to_le_bytes());
            fs::write(&steam, steam_bytes).unwrap();
            fs::write(prefix.join("keep.txt"), b"prefix data to preserve").unwrap();
            add_launcher(&home, &bottle.id, "Steam", &steam).unwrap();
            let mut attempt = steam_install::CatalogAttempt::new(
                &bottle.id,
                "steam",
                "a".repeat(64),
                "https://cdn.fastly.steamstatic.com/client/installer/SteamSetup.exe".into(),
            )
            .unwrap();
            attempt.set_stage(steam_install::CatalogStage::Installed);
            steam_install::save_catalog_attempt(&home, &attempt).unwrap();
            let record_path = dir.join("prefix-record.json");
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
            record.runtime_identity = Some(TLS_WINE_11_IDENTITY.into());
            record.runtime_fingerprint = Some(format!("sha256-bound-v2:{}", "1".repeat(64)));
            save_json(&record_path, &record).unwrap();
            let backup_path = home.join(format!(
                "runtime-migration-backups/{}-tls-to-steam-prefix-record.json",
                bottle.id
            ));
            Self {
                home,
                bottle_id: bottle.id,
                wine,
                steam,
                record_path,
                backup_path,
            }
        }

        fn approved_identity(&self) -> String {
            runtime_identity::bundled_runtime_identity(&self.wine)
                .unwrap()
                .unwrap()
        }

        fn policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: TLS_WINE_11_IDENTITY,
                approved,
                backup_suffix: "tls-to-steam",
                verify_launcher_pe: false,
            }
        }

        fn migrate(&self, policy: &MigrationPolicy<'_>) -> Result<Option<RecordRollback>, String> {
            let _lock = lock_bottle(&self.home, &self.bottle_id)?;
            migrate_with_policy(
                &self.home,
                &self.bottle_id,
                &self.wine,
                &self.steam.canonicalize().unwrap(),
                policy,
                Duration::from_secs(2),
            )
        }

        fn set_predecessor(&self, identity: &str) {
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&self.record_path).unwrap()).unwrap();
            record.runtime_identity = Some(identity.into());
            save_json(&self.record_path, &record).unwrap();
        }

        fn native_policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: STEAM_WINE_11_IDENTITY,
                approved,
                backup_suffix: "steam-to-native",
                verify_launcher_pe: true,
            }
        }

        fn native_backup(&self) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-steam-to-native-prefix-record.json",
                self.bottle_id
            ))
        }

        fn direct_native_policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: TLS_WINE_11_IDENTITY,
                approved,
                backup_suffix: "tls-to-native",
                verify_launcher_pe: true,
            }
        }

        fn direct_native_backup(&self) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-tls-to-native-prefix-record.json",
                self.bottle_id
            ))
        }

        fn skipped_switchable_backup(&self, policy: &MigrationPolicy<'_>) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-{}-prefix-record.json",
                self.bottle_id, policy.backup_suffix
            ))
        }

        fn switchable_policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            MigrationPolicy {
                previous: NATIVE_WINE_11_IDENTITY,
                approved,
                backup_suffix: "native-to-switchable",
                verify_launcher_pe: false,
            }
        }

        fn switchable_backup(&self) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-native-to-switchable-prefix-record.json",
                self.bottle_id
            ))
        }

        fn text_editing_policy<'a>(&self, approved: &'a str) -> MigrationPolicy<'a> {
            test_text_editing_policy(SWITCHABLE_WINE_11_IDENTITY, approved)
        }

        fn text_editing_backup(&self) -> PathBuf {
            self.home.join(format!(
                "runtime-migration-backups/{}-switchable-to-native-text-editing-prefix-record.json",
                self.bottle_id
            ))
        }

        fn migrate_bundled(
            &self,
            policy: &MigrationPolicy<'_>,
        ) -> Result<Option<RecordRollback>, String> {
            let _lock = lock_bottle(&self.home, &self.bottle_id)?;
            migrate_bundled_with_policy(
                &self.home,
                &self.bottle_id,
                &self.wine,
                policy,
                Duration::from_secs(2),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    // A disposable package has its own content hash. Retain every production
    // gate and predecessor while replacing only the approved destination hash.
    fn test_skipped_switchable_policy<'a>(source: &str, approved: &'a str) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_SWITCHABLE_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_SWITCHABLE_POLICY,
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    fn test_text_editing_policy<'a>(source: &str, approved: &'a str) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_NATIVE_TEXT_EDITING_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_NATIVE_TEXT_EDITING_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_NATIVE_TEXT_EDITING_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_NATIVE_TEXT_EDITING_POLICY,
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    fn test_refined_successor_policy<'a>(source: &str, approved: &'a str) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_REFINED_NATIVE_TEXT_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_REFINED_NATIVE_TEXT_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_REFINED_NATIVE_TEXT_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_REFINED_NATIVE_TEXT_POLICY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY => &NATIVE_TEXT_TO_REFINED_NATIVE_TEXT_POLICY,
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    // A disposable package substitutes only the destination content hash;
    // its predecessor, backup name and gates remain the production policy.
    fn test_refined_text_editing_policy(approved: &str) -> MigrationPolicy<'_> {
        test_refined_successor_policy(NATIVE_TEXT_EDITING_WINE_11_IDENTITY, approved)
    }

    fn test_bounded_native_ui_policy<'a>(source: &str, approved: &'a str) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_BOUNDED_NATIVE_UI_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_BOUNDED_NATIVE_UI_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_BOUNDED_NATIVE_UI_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_BOUNDED_NATIVE_UI_POLICY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY => &NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY => {
                &REFINED_NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY
            }
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    fn test_bundled_steam_ui_policy<'a>(source: &str, approved: &'a str) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_BUNDLED_STEAM_UI_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_BUNDLED_STEAM_UI_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_BUNDLED_STEAM_UI_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_BUNDLED_STEAM_UI_POLICY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY => &NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY => {
                &REFINED_NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY
            }
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY => &BOUNDED_NATIVE_UI_TO_BUNDLED_STEAM_UI_POLICY,
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    fn test_single_steam_ui_policy<'a>(source: &str, approved: &'a str) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_SINGLE_STEAM_UI_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_SINGLE_STEAM_UI_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_SINGLE_STEAM_UI_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_SINGLE_STEAM_UI_POLICY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY => &NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY => {
                &REFINED_NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY
            }
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY => &BOUNDED_NATIVE_UI_TO_SINGLE_STEAM_UI_POLICY,
            BUNDLED_STEAM_UI_WINE_11_IDENTITY => &BUNDLED_STEAM_UI_TO_SINGLE_STEAM_UI_POLICY,
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    fn test_recognized_text_popup_policy<'a>(
        source: &str,
        approved: &'a str,
    ) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY => &NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY => {
                &REFINED_NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY
            }
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY => {
                &BOUNDED_NATIVE_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY
            }
            BUNDLED_STEAM_UI_WINE_11_IDENTITY => &BUNDLED_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            SINGLE_STEAM_UI_WINE_11_IDENTITY => &SINGLE_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
            CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY => {
                &CAPTURED_TEXT_CONTEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY
            }
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    fn test_captured_text_context_policy<'a>(
        source: &str,
        approved: &'a str,
    ) -> MigrationPolicy<'a> {
        let policy = match source {
            TLS_WINE_11_IDENTITY => &TLS_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            STEAM_WINE_11_IDENTITY => &STEAM_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            NATIVE_WINE_11_IDENTITY => &NATIVE_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            SWITCHABLE_WINE_11_IDENTITY => &SWITCHABLE_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY => &NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY => {
                &REFINED_NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY
            }
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY => {
                &BOUNDED_NATIVE_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY
            }
            BUNDLED_STEAM_UI_WINE_11_IDENTITY => &BUNDLED_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            SINGLE_STEAM_UI_WINE_11_IDENTITY => &SINGLE_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
            _ => panic!("unexpected predecessor"),
        };
        MigrationPolicy {
            previous: policy.previous,
            approved,
            backup_suffix: policy.backup_suffix,
            verify_launcher_pe: policy.verify_launcher_pe,
        }
    }

    #[test]
    fn failed_backup_write_cleans_staging_file_and_can_retry() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let error =
            retain_original_record_with_writer(&fixture.backup_path, &original, |file, bytes| {
                file.write_all(&bytes[..1]).map_err(|e| e.to_string())?;
                Err("simulated interrupted write".into())
            })
            .unwrap_err();
        assert!(error.contains("simulated interrupted write"));
        assert!(!fixture.backup_path.exists());
        assert_eq!(
            fs::read_dir(fixture.backup_path.parent().unwrap())
                .unwrap()
                .count(),
            0
        );
        retain_original_record(&fixture.backup_path, &original).unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn known_steam_transition_keeps_prefix_and_original_record() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let policy = fixture.policy(&approved);
        let _rollback = fixture.migrate(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        let prefix = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix");
        assert_eq!(
            fs::read(prefix.join("keep.txt")).unwrap(),
            b"prefix data to preserve"
        );
        let migrated: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert_eq!(migrated.runtime_identity.as_deref(), Some(policy.approved));
        ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
        assert!(fixture.migrate(&policy).unwrap().is_none());
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn failed_verification_can_restore_exact_original_record() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.policy(&approved))
            .unwrap()
            .unwrap();
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn unrelated_runtime_and_unapproved_package_are_rejected() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let wrong = MigrationPolicy {
            previous: TLS_WINE_11_IDENTITY,
            approved: "sha256:wrong",
            backup_suffix: "tls-to-steam",
            verify_launcher_pe: false,
        };
        assert!(fixture.migrate(&wrong).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists());

        let mut record: PrefixRecord = serde_json::from_slice(&original).unwrap();
        record.runtime_identity = Some(format!("sha256:{}", "0".repeat(64)));
        save_json(&fixture.record_path, &record).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn busy_bottle_and_unexpected_backup_leave_old_record_in_place() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let approved = fixture.approved_identity();
        assert!(fixture.migrate(&fixture.policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists());

        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        fs::write(&fixture.backup_path, b"unexpected previous backup").unwrap();
        assert!(fixture.migrate(&fixture.policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(
            fs::read(&fixture.backup_path).unwrap(),
            b"unexpected previous backup"
        );
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn only_an_installed_recipe_steam_launcher_can_trigger_migration() {
        let fixture = Fixture::new();
        let mut attempt = steam_install::catalog_attempt(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .unwrap();
        attempt.set_stage(steam_install::CatalogStage::Failed);
        steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn custom_recorded_path_stays_on_strict_runtime_check() {
        let fixture = Fixture::new();
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        record.wine_path = fixture.home.join("custom-wine").display().to_string();
        save_json(&fixture.record_path, &record).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(
            verify_runtime_record(&record, &fixture.wine)
                .unwrap()
                .is_none()
        );
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(&fixture.home).unwrap();
    }

    #[test]
    fn busy_wineserver_prevents_record_change() {
        let fixture = Fixture::new();
        let dispatch = fixture.wine.parent().unwrap().join("_dispatch");
        fs::write(
            dispatch,
            b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) while :; do :; done;; esac\nexit 0\n",
        )
        .unwrap();
        let approved = fixture.approved_identity();
        let original = fs::read(&fixture.record_path).unwrap();
        let error = fixture.migrate(&fixture.policy(&approved)).err().unwrap();
        assert!(error.contains("Close Steam"), "{error}");
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists());
        fs::remove_dir_all(&fixture.home).unwrap();
    }
    #[test]
    fn native_transition_selects_record_predecessor_and_keeps_separate_backups() {
        let fixture = Fixture::new();
        let tls_original = fs::read(&fixture.record_path).unwrap();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        fs::write(&fixture.backup_path, &tls_original).unwrap();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let steam_original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let tls_policy = fixture.policy("sha256:another-approved-steam-package");
        let native_policy = fixture.native_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let rollback = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&tls_policy, &native_policy],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), tls_original);
        assert_eq!(fs::read(fixture.native_backup()).unwrap(), steam_original);
        assert_eq!(
            fs::read(
                bottle_dir(&fixture.home, &fixture.bottle_id)
                    .unwrap()
                    .join("prefix/keep.txt")
            )
            .unwrap(),
            b"prefix data to preserve"
        );
        ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), steam_original);
    }

    #[test]
    fn tls_record_requires_an_explicitly_reviewed_direct_edge() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let tls_policy = fixture.policy("sha256:required-intermediate-steam-package");
        let native_policy = fixture.native_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let result = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&tls_policy, &native_policy],
            Duration::from_secs(2),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.backup_path.exists() && !fixture.native_backup().exists());
    }

    #[test]
    fn direct_native_update_selects_exact_target_and_preserves_tls_backup() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        fs::create_dir_all(fixture.backup_path.parent().unwrap()).unwrap();
        fs::write(&fixture.backup_path, &original).unwrap();
        let approved = fixture.approved_identity();
        let tls_policy = fixture.policy("sha256:other-reviewed-steam-package");
        let steam_policy = fixture.native_policy(&approved);
        let direct_policy = fixture.direct_native_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let before_hashes = runtime_identity::full_content_hash_count();
        let rollback = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&tls_policy, &steam_policy, &direct_policy],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            runtime_identity::full_content_hash_count(),
            before_hashes + 1
        );
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        assert_eq!(fs::read(fixture.direct_native_backup()).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        let migrated: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert_eq!(
            migrated.runtime_identity.as_deref(),
            Some(approved.as_str())
        );
        let prefix = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix");
        assert_eq!(
            fs::read(prefix.join("keep.txt")).unwrap(),
            b"prefix data to preserve"
        );
        ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
    }

    #[test]
    fn direct_edge_does_not_change_original_tls_to_steam_gate() {
        let fixture = Fixture::new();
        // The established transition did not require new PE validation.
        fs::write(&fixture.steam, b"original historical launcher fixture").unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let direct_policy = fixture.direct_native_policy("sha256:other-reviewed-native-package");
        let tls_policy = fixture.policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let rollback = migrate_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &fixture.steam.canonicalize().unwrap(),
            &[&direct_policy, &tls_policy],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(&fixture.backup_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
    }

    #[test]
    fn direct_native_update_refuses_a_different_existing_backup() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let backup = fixture.direct_native_backup();
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        fs::write(&backup, b"different original record").unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(backup).unwrap(), b"different original record");
    }

    #[test]
    fn direct_native_update_refuses_wrong_or_changed_target_and_corrupt_launcher() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy("sha256:unapproved"))
                .is_err()
        );
        let approved = fixture.approved_identity();
        let component = fixture
            .wine
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("lib/libfreetype.6.dylib");
        fs::write(&component, b"changed package bytes").unwrap();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        let approved = fixture.approved_identity();
        fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap();
        let error = fixture
            .migrate(&fixture.direct_native_policy(&approved))
            .err()
            .unwrap();
        assert!(
            error.contains("verify the installed Steam launcher"),
            "{error}"
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
    }

    #[test]
    fn direct_native_update_requires_original_binding_and_installed_catalog_launcher() {
        for invalid in ["binding", "custom-path", "catalog-stage", "launcher"] {
            let fixture = Fixture::new();
            match invalid {
                "binding" | "custom-path" => {
                    let mut record: PrefixRecord =
                        serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
                    if invalid == "binding" {
                        record.runtime_fingerprint = Some("legacy-unbound-record".into());
                    } else {
                        record.wine_path = fixture.home.join("custom-wine").display().to_string();
                    }
                    save_json(&fixture.record_path, &record).unwrap();
                }
                "catalog-stage" => {
                    let mut attempt =
                        steam_install::catalog_attempt(&fixture.home, &fixture.bottle_id)
                            .unwrap()
                            .unwrap();
                    attempt.set_stage(steam_install::CatalogStage::Failed);
                    steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
                }
                "launcher" => save_json(
                    &bottle_dir(&fixture.home, &fixture.bottle_id)
                        .unwrap()
                        .join("launchers.json"),
                    &Vec::<Launcher>::new(),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let original = fs::read(&fixture.record_path).unwrap();
            let approved = fixture.approved_identity();
            assert!(
                fixture
                    .migrate(&fixture.direct_native_policy(&approved))
                    .unwrap()
                    .is_none(),
                "{invalid}"
            );
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert!(!fixture.direct_native_backup().exists());
        }
    }

    #[test]
    fn direct_native_update_refuses_tracked_and_server_busy_states() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::write(
            fixture.wine.parent().unwrap().join("_dispatch"),
            b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) while :; do :; done;; esac\nexit 0\n",
        )
        .unwrap();
        let approved = fixture.approved_identity();
        let error = fixture
            .migrate(&fixture.direct_native_policy(&approved))
            .err()
            .unwrap();
        assert!(error.contains("Close Steam"), "{error}");
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
    }

    #[test]
    fn direct_native_update_requires_zero_owners_without_signalling_untracked_owner() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let held = fixture
            .steam
            .parent()
            .unwrap()
            .join("held-direct-update.log");
        fs::write(&held, b"original fixture").unwrap();
        let mut owner = Owner(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(&held)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(owner.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .is_err()
        );
        assert!(owner.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.direct_native_backup().exists());
        drop(owner);
        assert!(
            fixture
                .migrate(&fixture.direct_native_policy(&approved))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn direct_native_update_rolls_back_exact_tls_record_after_failed_verification() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.direct_native_policy(&approved))
            .unwrap()
            .unwrap();
        let registry = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix/system.reg");
        fs::remove_file(&registry).unwrap();
        fs::create_dir(&registry).unwrap();
        assert!(ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).is_err());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.direct_native_backup()).unwrap(), original);
    }

    #[test]
    fn skipped_switchable_edges_select_exact_source_and_preserve_rollback() {
        for edge in [&STEAM_TO_SWITCHABLE_POLICY, &TLS_TO_SWITCHABLE_POLICY] {
            assert!(
                STEAM_RUNTIME_POLICIES
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        assert_eq!(STEAM_TO_SWITCHABLE_POLICY.previous, STEAM_WINE_11_IDENTITY);
        assert_eq!(TLS_TO_SWITCHABLE_POLICY.previous, TLS_WINE_11_IDENTITY);
        assert_eq!(
            STEAM_TO_SWITCHABLE_POLICY.approved,
            SWITCHABLE_WINE_11_IDENTITY
        );
        assert_eq!(
            TLS_TO_SWITCHABLE_POLICY.approved,
            SWITCHABLE_WINE_11_IDENTITY
        );
        assert_ne!(
            STEAM_TO_SWITCHABLE_POLICY.backup_suffix,
            TLS_TO_SWITCHABLE_POLICY.backup_suffix
        );
        for source in [TLS_WINE_11_IDENTITY, STEAM_WINE_11_IDENTITY] {
            let fixture = Fixture::new();
            fixture.set_predecessor(source);
            let original = fs::read(&fixture.record_path).unwrap();
            let approved = fixture.approved_identity();
            let steam_policy = test_skipped_switchable_policy(STEAM_WINE_11_IDENTITY, &approved);
            let tls_policy = test_skipped_switchable_policy(TLS_WINE_11_IDENTITY, &approved);
            let policies = [
                &TLS_TO_STEAM_POLICY,
                &STEAM_TO_NATIVE_POLICY,
                &TLS_TO_NATIVE_POLICY,
                &steam_policy,
                &tls_policy,
            ];
            let selected = if source == TLS_WINE_11_IDENTITY {
                &tls_policy
            } else {
                &steam_policy
            };
            let other = if source == TLS_WINE_11_IDENTITY {
                &steam_policy
            } else {
                &tls_policy
            };
            let launcher = fixture.steam.canonicalize().unwrap();
            let rollback = {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                migrate_with_policies(
                    &fixture.home,
                    &fixture.bottle_id,
                    &fixture.wine,
                    &launcher,
                    &policies,
                    Duration::from_secs(2),
                )
                .unwrap()
                .unwrap()
            };
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(selected)).unwrap(),
                original
            );
            assert!(!fixture.skipped_switchable_backup(other).exists());
            assert!(!fixture.native_backup().exists());
            assert!(!fixture.direct_native_backup().exists());
            let migrated: PrefixRecord =
                serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
            assert_eq!(
                migrated.runtime_identity.as_deref(),
                Some(approved.as_str())
            );
            assert!(runtime_record_matches(&migrated, &fixture.wine).unwrap());
            let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
            assert_eq!(
                fs::read(dir.join("prefix/keep.txt")).unwrap(),
                b"prefix data to preserve"
            );
            ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).unwrap();
            {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                assert!(
                    migrate_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &launcher,
                        &policies,
                        Duration::from_secs(2),
                    )
                    .unwrap()
                    .is_none()
                );
            }
            rollback.restore().unwrap();
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(selected)).unwrap(),
                original
            );
        }
    }

    #[test]
    fn skipped_switchable_edges_keep_steam_and_runtime_gates() {
        for source in [TLS_WINE_11_IDENTITY, STEAM_WINE_11_IDENTITY] {
            for invalid in [
                "custom-path",
                "unbound",
                "catalog",
                "launcher",
                "pe",
                "package",
            ] {
                let fixture = Fixture::new();
                fixture.set_predecessor(source);
                let approved = fixture.approved_identity();
                let policy = test_skipped_switchable_policy(
                    source,
                    if invalid == "package" {
                        "sha256:unapproved-switchable-package"
                    } else {
                        &approved
                    },
                );
                match invalid {
                    "custom-path" | "unbound" => {
                        let mut record: PrefixRecord =
                            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap())
                                .unwrap();
                        if invalid == "custom-path" {
                            record.wine_path =
                                fixture.home.join("custom-wine").display().to_string();
                        } else {
                            record.runtime_fingerprint = Some("unbound".into());
                        }
                        save_json(&fixture.record_path, &record).unwrap();
                    }
                    "catalog" => {
                        let mut attempt =
                            steam_install::catalog_attempt(&fixture.home, &fixture.bottle_id)
                                .unwrap()
                                .unwrap();
                        attempt.set_stage(steam_install::CatalogStage::Failed);
                        steam_install::save_catalog_attempt(&fixture.home, &attempt).unwrap();
                    }
                    "launcher" => save_json(
                        &bottle_dir(&fixture.home, &fixture.bottle_id)
                            .unwrap()
                            .join("launchers.json"),
                        &Vec::<Launcher>::new(),
                    )
                    .unwrap(),
                    "pe" => fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap(),
                    "package" => {}
                    _ => unreachable!(),
                }
                let original = fs::read(&fixture.record_path).unwrap();
                let result = fixture.migrate(&policy);
                if matches!(invalid, "custom-path" | "unbound" | "catalog" | "launcher") {
                    assert!(result.unwrap().is_none(), "{source} {invalid}");
                } else {
                    assert!(result.is_err(), "{source} {invalid}");
                }
                assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
                assert!(!fixture.skipped_switchable_backup(&policy).exists());
            }
        }
    }

    #[test]
    fn skipped_switchable_edges_refuse_live_tracked_and_untracked_owners() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        for source in [TLS_WINE_11_IDENTITY, STEAM_WINE_11_IDENTITY] {
            let fixture = Fixture::new();
            fixture.set_predecessor(source);
            let approved = fixture.approved_identity();
            let policy = test_skipped_switchable_policy(source, &approved);
            let original = fs::read(&fixture.record_path).unwrap();
            let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
            save_json(
                &dir.join("active-processes.json"),
                &vec![std::process::id()],
            )
            .unwrap();
            assert!(fixture.migrate(&policy).is_err());
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();

            let held = dir.join("prefix/held-skipped-update.log");
            fs::write(&held, b"test owner").unwrap();
            let mut owner = Owner(
                Command::new("/bin/sh")
                    .args([
                        "-c",
                        "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                        "fixture",
                    ])
                    .arg(&held)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let mut ready = String::new();
            std::io::BufReader::new(owner.0.stdout.take().unwrap())
                .read_line(&mut ready)
                .unwrap();
            assert_eq!(ready, "ready\n");
            assert!(fixture.migrate(&policy).is_err());
            assert!(owner.0.try_wait().unwrap().is_none());
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
            drop(owner);
            assert!(fixture.migrate(&policy).unwrap().is_some());
        }
    }

    #[test]
    fn selecting_exact_previous_package_does_not_migrate() {
        let fixture = Fixture::new();
        let previous = fixture.approved_identity();
        fixture.set_predecessor(&previous);
        let original = fs::read(&fixture.record_path).unwrap();
        let policy = MigrationPolicy {
            previous: &previous,
            approved: "sha256:different-successor",
            backup_suffix: "steam-to-native",
            verify_launcher_pe: true,
        };
        assert!(fixture.migrate(&policy).unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
    }

    #[test]
    fn native_transition_rejects_unapproved_and_corrupt_installed_launcher() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        assert!(
            fixture
                .migrate(&fixture.native_policy("sha256:unapproved"))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        let approved = fixture.approved_identity();
        fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap();
        let error = fixture
            .migrate(&fixture.native_policy(&approved))
            .err()
            .unwrap();
        assert!(
            error.contains("verify the installed Steam launcher"),
            "{error}"
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
    }

    #[test]
    fn native_transition_never_executes_recorded_engine() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let saved_engine = fixture
            .home
            .join("old/Caskrun.app/Contents/Resources/wine-runtime/bin/_dispatch");
        fs::create_dir_all(saved_engine.parent().unwrap()).unwrap();
        let marker = saved_engine.with_file_name("recorded-engine-was-executed");
        fs::write(
            &saved_engine,
            b"#!/bin/sh\nprintf executed > \"${0%/*}/recorded-engine-was-executed\"\nexit 1\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&saved_engine).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&saved_engine, permissions).unwrap();
        let mut record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        record.wine_path = saved_engine.display().to_string();
        save_json(&fixture.record_path, &record).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.native_policy(&approved))
            .unwrap()
            .unwrap();
        assert!(!marker.exists());
        assert_eq!(fs::read(fixture.native_backup()).unwrap(), original);
        rollback.restore().unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn native_transition_preserves_record_when_tracked_process_or_backup_is_busy() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let approved = fixture.approved_identity();
        assert!(fixture.migrate(&fixture.native_policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::create_dir_all(fixture.native_backup().parent().unwrap()).unwrap();
        fs::write(fixture.native_backup(), b"unrelated native backup").unwrap();
        assert!(fixture.migrate(&fixture.native_policy(&approved)).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(
            fs::read(fixture.native_backup()).unwrap(),
            b"unrelated native backup"
        );
    }

    #[test]
    fn native_transition_waits_for_exact_untracked_open_handle_without_signalling_it() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let held = fixture.steam.parent().unwrap().join("held-fixture.log");
        fs::write(&held, b"self-authored fixture").unwrap();
        let mut owner = Owner(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(&held)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(owner.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let approved = fixture.approved_identity();
        let result = fixture.migrate(&fixture.native_policy(&approved));
        assert!(result.is_err());
        assert!(owner.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.native_backup().exists());
        drop(owner); // EOF lets our fixture exit naturally.
        assert!(
            fixture
                .migrate(&fixture.native_policy(&approved))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn native_transition_rolls_back_when_ordinary_prefix_verification_fails() {
        let fixture = Fixture::new();
        fixture.set_predecessor(STEAM_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let rollback = fixture
            .migrate(&fixture.native_policy(&approved))
            .unwrap()
            .unwrap();
        let registry = bottle_dir(&fixture.home, &fixture.bottle_id)
            .unwrap()
            .join("prefix/system.reg");
        fs::remove_file(&registry).unwrap();
        fs::create_dir(&registry).unwrap();
        assert!(ensure_prefix_with_wine(&fixture.home, &fixture.bottle_id, &fixture.wine).is_err());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.native_backup()).unwrap(), original);
    }

    #[test]
    fn bundled_transition_needs_no_catalog_or_launcher_and_preserves_prefix() {
        let fixture = Fixture::new();
        fixture.set_predecessor(NATIVE_WINE_11_IDENTITY);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        fs::remove_file(dir.join("launchers.json")).unwrap();
        fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
        rename_bottle(&fixture.home, &fixture.bottle_id, "Custom apps").unwrap();
        let registry = dir.join("prefix/system.reg");
        let registry_before = fs::metadata(&registry).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let policy = fixture.switchable_policy(&approved);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(fs::read(fixture.switchable_backup()).unwrap(), original);
        assert!(same_snapshot(
            &registry_before,
            &fs::metadata(&registry).unwrap()
        ));
        assert_eq!(
            fs::read(dir.join("prefix/keep.txt")).unwrap(),
            b"prefix data to preserve"
        );
        let migrated: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert!(runtime_record_matches(&migrated, &fixture.wine).unwrap());
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.switchable_backup()).unwrap(), original);
        assert!(same_snapshot(
            &registry_before,
            &fs::metadata(&registry).unwrap()
        ));
    }

    #[test]
    fn bundled_transition_keeps_custom_legacy_unknown_and_unbound_records_strict() {
        for invalid in ["custom", "path-only", "unknown", "unbound"] {
            let fixture = Fixture::new();
            fixture.set_predecessor(NATIVE_WINE_11_IDENTITY);
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
            match invalid {
                "custom" => {
                    record.wine_path = fixture.home.join("custom-wine").display().to_string()
                }
                "path-only" => record.runtime_identity = None,
                "unknown" => record.runtime_identity = Some("sha256:unknown-predecessor".into()),
                "unbound" => record.runtime_fingerprint = Some("legacy-fingerprint".into()),
                _ => unreachable!(),
            }
            save_json(&fixture.record_path, &record).unwrap();
            let original = fs::read(&fixture.record_path).unwrap();
            let approved = fixture.approved_identity();
            assert!(
                fixture
                    .migrate_bundled(&fixture.switchable_policy(&approved))
                    .unwrap()
                    .is_none(),
                "{invalid}"
            );
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert!(!fixture.switchable_backup().exists());
        }
    }

    #[test]
    fn bundled_transition_requires_exact_destination_and_app_path() {
        let fixture = Fixture::new();
        fixture.set_predecessor(NATIVE_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        assert!(
            fixture
                .migrate_bundled(&fixture.switchable_policy("sha256:unapproved-successor"))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.switchable_backup().exists());
        let approved = fixture.approved_identity();
        let package = fixture.home.join("Caskrun.app");
        let other = fixture.home.join("Other.app");
        fs::rename(&package, &other).unwrap();
        let wine = other.join("Contents/Resources/wine-runtime/bin/wine");
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        assert!(
            migrate_bundled_with_policy(
                &fixture.home,
                &fixture.bottle_id,
                &wine,
                &fixture.switchable_policy(&approved),
                Duration::from_secs(2),
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.switchable_backup().exists());
    }

    #[test]
    fn bundled_transition_refuses_busy_bottle_and_conflicting_backup() {
        let fixture = Fixture::new();
        fixture.set_predecessor(NATIVE_WINE_11_IDENTITY);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let policy = fixture.switchable_policy(&approved);
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        assert!(fixture.migrate_bundled(&policy).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.switchable_backup().exists());
        save_json(&dir.join("active-processes.json"), &Vec::<u32>::new()).unwrap();
        fs::create_dir_all(fixture.switchable_backup().parent().unwrap()).unwrap();
        fs::write(fixture.switchable_backup(), b"unrelated backup").unwrap();
        assert!(fixture.migrate_bundled(&policy).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(
            fs::read(fixture.switchable_backup()).unwrap(),
            b"unrelated backup"
        );
    }

    #[test]
    fn bundled_transition_does_not_migrate_an_uninitialized_prefix() {
        let fixture = Fixture::new();
        fixture.set_predecessor(NATIVE_WINE_11_IDENTITY);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("prefix/system.reg")).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate_bundled(&fixture.switchable_policy(&approved))
                .unwrap()
                .is_none()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert!(!fixture.switchable_backup().exists());
    }

    #[test]
    fn bundled_transition_leaves_unrecorded_import_to_normal_initialization() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate_bundled(&fixture.switchable_policy(&approved))
                .unwrap()
                .is_none()
        );
        assert!(!fixture.record_path.exists());
        assert!(!fixture.switchable_backup().exists());
        fs::write(&fixture.record_path, b"malformed record").unwrap();
        assert!(
            fixture
                .migrate_bundled(&fixture.switchable_policy(&approved))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), b"malformed record");
    }

    #[test]
    fn bundled_transition_restores_record_if_destination_changes_during_idle_check() {
        let fixture = Fixture::new();
        fixture.set_predecessor(NATIVE_WINE_11_IDENTITY);
        fs::write(
            fixture.wine.parent().unwrap().join("_dispatch"),
            b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) printf changed >> \"${0%/*}/../runtime/bin/wine\";; *) exit 9;; esac\nexit 0\n",
        )
        .unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let approved = fixture.approved_identity();
        let error = fixture
            .migrate_bundled(&fixture.switchable_policy(&approved))
            .err()
            .unwrap();
        assert!(
            error.contains("original runtime record was restored"),
            "{error}"
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.switchable_backup()).unwrap(), original);
    }

    #[derive(Debug, Eq, PartialEq)]
    struct PreservedEntry {
        path: PathBuf,
        contents_or_link: Vec<u8>,
        mode: u32,
        inode: u64,
        modified: (i64, i64),
    }

    // Compare the entire prefix, including relative profile/drive mappings,
    // and every bottle file except the deliberately replaced runtime record.
    // Directory ctime is excluded because reading it is not a data mutation.
    fn bottle_contents(fixture: &Fixture) -> Vec<PreservedEntry> {
        fn walk(root: &Path, dir: &Path, entries: &mut Vec<PreservedEntry>) {
            let mut paths: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            paths.sort();
            for path in paths {
                let relative = path.strip_prefix(root).unwrap();
                if relative == Path::new("prefix-record.json") {
                    continue;
                }
                let metadata = fs::symlink_metadata(&path).unwrap();
                let contents_or_link = if metadata.file_type().is_symlink() {
                    fs::read_link(&path)
                        .unwrap()
                        .as_os_str()
                        .as_bytes()
                        .to_vec()
                } else if metadata.file_type().is_file() {
                    fs::read(&path).unwrap()
                } else {
                    Vec::new()
                };
                entries.push(PreservedEntry {
                    path: relative.to_path_buf(),
                    contents_or_link,
                    mode: metadata.mode(),
                    inode: metadata.ino(),
                    modified: (metadata.mtime(), metadata.mtime_nsec()),
                });
                if metadata.file_type().is_dir() {
                    walk(root, &path, entries);
                }
            }
        }
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        let mut entries = Vec::new();
        walk(&dir, &dir, &mut entries);
        entries
    }

    fn prepare_text_editing_fixture(fixture: &Fixture) {
        fixture.set_predecessor(SWITCHABLE_WINE_11_IDENTITY);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::write(
            dir.join("graphics.json"),
            b"{\"version\":2,\"backend\":\"dxmt\",\"dxmt_frame_rate\":60,\"dxmt_spatial_scale\":150}",
        )
        .unwrap();
        fs::write(dir.join("notes.txt"), b"Fixture bottle settings").unwrap();
        let prefix = dir.join("prefix");
        fs::write(prefix.join("system.reg"), b"WINE REGISTRY Version 2\n").unwrap();
        fs::write(prefix.join("user.reg"), b"Fixture saved registry values\n").unwrap();
        let users = prefix.join("drive_c/users");
        fs::create_dir_all(users.join("Example/AppData/Roaming/ExampleApp")).unwrap();
        fs::create_dir_all(users.join("Shared/Documents")).unwrap();
        fs::write(
            users.join("Example/AppData/Roaming/ExampleApp/settings.ini"),
            b"sound=on\n",
        )
        .unwrap();
        fs::write(
            users.join("Shared/Documents/save.dat"),
            b"Fixture saved game",
        )
        .unwrap();
        symlink("../Shared/Documents", users.join("Example/Documents")).unwrap();
        symlink("Example", users.join("OtherAccount")).unwrap();
        fs::create_dir_all(prefix.join("drive_c/ExampleGame")).unwrap();
        fs::write(
            prefix.join("drive_c/ExampleGame/d3d11.dll"),
            b"Fixture app-local graphics DLL must remain unchanged",
        )
        .unwrap();
        fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        symlink("../drive_c", prefix.join("dosdevices/d:")).unwrap();
    }

    #[test]
    fn text_editing_transition_changes_only_record_and_can_restore_it_exactly() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        // A generic bottle needs no publisher recipe or installed Steam.
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let approved = fixture.approved_identity();
        let policy = fixture.text_editing_policy(&approved);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(bottle_contents(&fixture), contents);
        assert_eq!(fs::read(fixture.text_editing_backup()).unwrap(), original);
        let record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert_eq!(record.runtime_identity.as_deref(), Some(approved.as_str()));
        assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        assert_eq!(bottle_contents(&fixture), contents);
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn text_editing_transition_selects_both_exact_endpoints_among_other_policies() {
        let fixture = Fixture::new();
        fixture.set_predecessor(SWITCHABLE_WINE_11_IDENTITY);
        let approved = fixture.approved_identity();
        let unapproved = fixture.text_editing_policy("sha256:unapproved-successor");
        let reviewed = fixture.text_editing_policy(&approved);
        let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let rollback = migrate_bundled_with_policies(
            &fixture.home,
            &fixture.bottle_id,
            &fixture.wine,
            &[&NATIVE_TO_SWITCHABLE_POLICY, &unapproved, &reviewed],
            Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert!(fixture.text_editing_backup().exists());
        rollback.restore().unwrap();
    }

    #[test]
    fn text_editing_transition_refuses_tracked_apps_without_mutating_bottle_contents() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        save_json(
            &dir.join("active-processes.json"),
            &vec![std::process::id()],
        )
        .unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let approved = fixture.approved_identity();
        assert!(
            fixture
                .migrate_bundled(&fixture.text_editing_policy(&approved))
                .is_err()
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
        assert!(!fixture.text_editing_backup().exists());
    }

    #[test]
    fn text_editing_transition_refuses_untracked_owner_without_signalling_it() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        let held = dir.join("prefix/keep.txt");
        let mut owner = Owner(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                    "fixture",
                ])
                .arg(&held)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        std::io::BufReader::new(owner.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let approved = fixture.approved_identity();
        let policy = fixture.text_editing_policy(&approved);
        assert!(fixture.migrate_bundled(&policy).is_err());
        assert!(owner.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
        assert!(!fixture.text_editing_backup().exists());
        drop(owner); // Our fixture exits naturally when its stdin closes.
        assert!(fixture.migrate_bundled(&policy).unwrap().is_some());
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn text_editing_transition_rejects_unknown_and_unbound_runtime_records() {
        for invalid in ["custom", "path-only", "unknown", "unbound"] {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
            match invalid {
                "custom" => {
                    record.wine_path = fixture.home.join("custom-wine").display().to_string()
                }
                "path-only" => record.runtime_identity = None,
                "unknown" => record.runtime_identity = Some("sha256:unknown-predecessor".into()),
                "unbound" => record.runtime_fingerprint = Some("unbound".into()),
                _ => unreachable!(),
            }
            save_json(&fixture.record_path, &record).unwrap();
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            assert!(
                fixture
                    .migrate_bundled(&fixture.text_editing_policy(&approved))
                    .unwrap()
                    .is_none(),
                "{invalid}"
            );
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
            assert!(!fixture.text_editing_backup().exists());
        }
    }

    #[test]
    fn text_editing_transition_restores_record_if_destination_changes_during_idle_check() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        fs::write(
            fixture.wine.parent().unwrap().join("_dispatch"),
            b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) printf changed >> \"${0%/*}/../runtime/bin/wine\";; *) exit 9;; esac\nexit 0\n",
        )
        .unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let approved = fixture.approved_identity();
        let error = fixture
            .migrate_bundled(&fixture.text_editing_policy(&approved))
            .err()
            .unwrap();
        assert!(
            error.contains("original runtime record was restored"),
            "{error}"
        );
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(fixture.text_editing_backup()).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn text_editing_edges_pin_four_predecessors_and_keep_their_original_gates() {
        let mut suffixes = std::collections::BTreeSet::new();
        for (edge, expected_previous, steam) in [
            (
                &TLS_TO_NATIVE_TEXT_EDITING_POLICY,
                TLS_WINE_11_IDENTITY,
                true,
            ),
            (
                &STEAM_TO_NATIVE_TEXT_EDITING_POLICY,
                STEAM_WINE_11_IDENTITY,
                true,
            ),
            (
                &NATIVE_TO_NATIVE_TEXT_EDITING_POLICY,
                NATIVE_WINE_11_IDENTITY,
                false,
            ),
            (
                &SWITCHABLE_TO_NATIVE_TEXT_EDITING_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                false,
            ),
        ] {
            assert_eq!(edge.previous, expected_previous);
            assert_eq!(edge.approved, NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            assert!(suffixes.insert(edge.backup_suffix));
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        assert!(
            BUNDLED_RUNTIME_POLICIES
                .iter()
                .all(|edge| !edge.verify_launcher_pe)
        );
        assert!(is_reviewed_switchable_runtime(SWITCHABLE_WINE_11_IDENTITY));
        assert!(is_reviewed_switchable_runtime(
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY
        ));
        assert!(!is_reviewed_switchable_runtime(NATIVE_WINE_11_IDENTITY));
    }

    #[test]
    fn text_editing_direct_edges_select_final_target_without_intermediate_record_changes() {
        for source in [
            TLS_WINE_11_IDENTITY,
            STEAM_WINE_11_IDENTITY,
            NATIVE_WINE_11_IDENTITY,
            SWITCHABLE_WINE_11_IDENTITY,
        ] {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(source);
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let steam = matches!(source, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY);
            let original_policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            // Only substitute the disposable package's exact destination;
            // every predecessor, gate and older destination stays production.
            let policies: Vec<_> = original_policies
                .iter()
                .map(|edge| MigrationPolicy {
                    previous: edge.previous,
                    approved: if edge.approved == NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                        &approved
                    } else {
                        edge.approved
                    },
                    backup_suffix: edge.backup_suffix,
                    verify_launcher_pe: edge.verify_launcher_pe,
                })
                .collect();
            let policy_refs: Vec<_> = policies.iter().collect();
            let selected = test_text_editing_policy(source, &approved);
            let rollback = {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                if steam {
                    migrate_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &fixture.steam.canonicalize().unwrap(),
                        &policy_refs,
                        Duration::from_secs(2),
                    )
                } else {
                    migrate_bundled_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &policy_refs,
                        Duration::from_secs(2),
                    )
                }
            }
            .unwrap()
            .unwrap();
            assert_eq!(bottle_contents(&fixture), contents, "{source}");
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(&selected)).unwrap(),
                original
            );
            assert_eq!(
                fs::read_dir(fixture.home.join("runtime-migration-backups"))
                    .unwrap()
                    .count(),
                1,
                "No intermediate runtime record backup should exist"
            );
            let record: PrefixRecord =
                serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
            assert_eq!(record.runtime_identity.as_deref(), Some(approved.as_str()));
            assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
            rollback.restore().unwrap();
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents, "{source}");
        }
    }

    #[test]
    fn new_direct_edges_do_not_reject_old_reviewed_destinations_for_same_predecessor() {
        for source in [
            TLS_WINE_11_IDENTITY,
            STEAM_WINE_11_IDENTITY,
            NATIVE_WINE_11_IDENTITY,
        ] {
            let fixture = Fixture::new();
            fixture.set_predecessor(source);
            let approved = fixture.approved_identity();
            let steam = source != NATIVE_WINE_11_IDENTITY;
            let old = if steam {
                test_skipped_switchable_policy(source, &approved)
            } else {
                fixture.switchable_policy(&approved)
            };
            let new = test_text_editing_policy(source, NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
            // Put the different destination first: exact source + target,
            // not policy ordering, must choose the existing reviewed edge.
            let policies = [&new, &old];
            let rollback = {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                if steam {
                    migrate_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &fixture.steam.canonicalize().unwrap(),
                        &policies,
                        Duration::from_secs(2),
                    )
                } else {
                    migrate_bundled_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &policies,
                        Duration::from_secs(2),
                    )
                }
            }
            .unwrap()
            .unwrap();
            assert!(fixture.skipped_switchable_backup(&old).exists());
            assert!(!fixture.skipped_switchable_backup(&new).exists());
            rollback.restore().unwrap();
        }
    }

    #[test]
    fn older_text_editing_edges_still_require_installed_recipe_and_valid_steam_pe() {
        for source in [TLS_WINE_11_IDENTITY, STEAM_WINE_11_IDENTITY] {
            for invalid in ["catalog", "launcher", "pe", "package"] {
                let fixture = Fixture::new();
                prepare_text_editing_fixture(&fixture);
                fixture.set_predecessor(source);
                let approved = fixture.approved_identity();
                let policy = test_text_editing_policy(
                    source,
                    if invalid == "package" {
                        NATIVE_TEXT_EDITING_WINE_11_IDENTITY
                    } else {
                        &approved
                    },
                );
                let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
                match invalid {
                    "catalog" => fs::remove_file(dir.join("catalog-attempt.json")).unwrap(),
                    "launcher" => {
                        save_json(&dir.join("launchers.json"), &Vec::<Launcher>::new()).unwrap()
                    }
                    "pe" => fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap(),
                    "package" => {}
                    _ => unreachable!(),
                }
                let original = fs::read(&fixture.record_path).unwrap();
                let contents = bottle_contents(&fixture);
                let result = fixture.migrate(&policy);
                if matches!(invalid, "catalog" | "launcher") {
                    assert!(result.unwrap().is_none(), "{source} {invalid}");
                } else {
                    assert!(result.is_err(), "{source} {invalid}");
                }
                assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
                assert_eq!(bottle_contents(&fixture), contents);
                assert!(!fixture.skipped_switchable_backup(&policy).exists());
            }
        }
    }

    #[test]
    fn refined_text_editing_edges_pin_five_predecessors_and_preserve_historical_policies() {
        for (edge, previous, steam) in [
            (
                &TLS_TO_REFINED_NATIVE_TEXT_POLICY,
                TLS_WINE_11_IDENTITY,
                true,
            ),
            (
                &STEAM_TO_REFINED_NATIVE_TEXT_POLICY,
                STEAM_WINE_11_IDENTITY,
                true,
            ),
            (
                &NATIVE_TO_REFINED_NATIVE_TEXT_POLICY,
                NATIVE_WINE_11_IDENTITY,
                false,
            ),
            (
                &SWITCHABLE_TO_REFINED_NATIVE_TEXT_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                false,
            ),
            (
                &NATIVE_TEXT_TO_REFINED_NATIVE_TEXT_POLICY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
            ),
        ] {
            assert_eq!(edge.previous, previous);
            assert_eq!(edge.approved, REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        let all: Vec<_> = STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .filter(|edge| {
                edge.approved != CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
                    && edge.approved != RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
            })
            .copied()
            .collect();
        let mut suffixes = std::collections::BTreeSet::new();
        for edge in &all {
            assert!(suffixes.insert(edge.backup_suffix));
            assert_ne!(edge.previous, SINGLE_STEAM_UI_WINE_11_IDENTITY);
        }
        assert_eq!(
            all.iter()
                .filter(|edge| edge.approved == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY)
                .count(),
            5
        );
        assert_eq!(
            all.iter()
                .filter(|edge| edge.approved == NATIVE_TEXT_EDITING_WINE_11_IDENTITY)
                .count(),
            4,
            "The installed predecessor and its reviewed incoming edges remain available"
        );
        assert!(
            BUNDLED_RUNTIME_POLICIES
                .iter()
                .all(|edge| !edge.verify_launcher_pe)
        );
        assert_ne!(
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY
        );
    }

    #[test]
    fn bounded_native_ui_policies_pin_six_edges_and_keep_deployed_predecessors() {
        for (edge, previous, steam) in [
            (&TLS_TO_BOUNDED_NATIVE_UI_POLICY, TLS_WINE_11_IDENTITY, true),
            (
                &STEAM_TO_BOUNDED_NATIVE_UI_POLICY,
                STEAM_WINE_11_IDENTITY,
                true,
            ),
            (
                &NATIVE_TO_BOUNDED_NATIVE_UI_POLICY,
                NATIVE_WINE_11_IDENTITY,
                false,
            ),
            (
                &SWITCHABLE_TO_BOUNDED_NATIVE_UI_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                false,
            ),
            (
                &NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
            ),
            (
                &REFINED_NATIVE_TEXT_TO_BOUNDED_NATIVE_UI_POLICY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
            ),
        ] {
            assert_eq!(edge.previous, previous);
            assert_eq!(edge.approved, BOUNDED_NATIVE_UI_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        let all: Vec<_> = STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .filter(|edge| {
                edge.approved != CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
                    && edge.approved != RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
            })
            .copied()
            .collect();
        let mut suffixes = std::collections::BTreeSet::new();
        for edge in &all {
            assert!(suffixes.insert(edge.backup_suffix));
            assert_ne!(edge.previous, SINGLE_STEAM_UI_WINE_11_IDENTITY);
        }
        assert_eq!(
            all.iter()
                .filter(|edge| edge.approved == BOUNDED_NATIVE_UI_WINE_11_IDENTITY)
                .count(),
            6
        );
        assert_eq!(
            all.iter()
                .filter(|edge| edge.approved == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY)
                .count(),
            5
        );
        assert_eq!(
            all.iter()
                .filter(|edge| edge.approved == NATIVE_TEXT_EDITING_WINE_11_IDENTITY)
                .count(),
            4
        );
        assert!(
            BUNDLED_RUNTIME_POLICIES
                .iter()
                .all(|edge| !edge.verify_launcher_pe)
        );
        assert_ne!(
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY
        );
    }

    #[test]
    fn bundled_steam_ui_policies_pin_seven_edges_and_keep_all_historical_edges() {
        for (edge, previous, steam) in [
            (&TLS_TO_BUNDLED_STEAM_UI_POLICY, TLS_WINE_11_IDENTITY, true),
            (
                &STEAM_TO_BUNDLED_STEAM_UI_POLICY,
                STEAM_WINE_11_IDENTITY,
                true,
            ),
            (
                &NATIVE_TO_BUNDLED_STEAM_UI_POLICY,
                NATIVE_WINE_11_IDENTITY,
                false,
            ),
            (
                &SWITCHABLE_TO_BUNDLED_STEAM_UI_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                false,
            ),
            (
                &NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
            ),
            (
                &REFINED_NATIVE_TEXT_TO_BUNDLED_STEAM_UI_POLICY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
            ),
            (
                &BOUNDED_NATIVE_UI_TO_BUNDLED_STEAM_UI_POLICY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                false,
            ),
        ] {
            assert_eq!(edge.previous, previous);
            assert_eq!(edge.approved, BUNDLED_STEAM_UI_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        let all: Vec<_> = STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .filter(|edge| {
                edge.approved != CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
                    && edge.approved != RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
            })
            .copied()
            .collect();
        let mut suffixes = std::collections::BTreeSet::new();
        let mut pairs = std::collections::BTreeSet::new();
        for edge in &all {
            assert!(suffixes.insert(edge.backup_suffix));
            assert!(pairs.insert((edge.previous, edge.approved)));
            assert_ne!(edge.previous, SINGLE_STEAM_UI_WINE_11_IDENTITY);
        }
        for (target, count) in [
            (STEAM_WINE_11_IDENTITY, 1),
            (NATIVE_WINE_11_IDENTITY, 2),
            (SWITCHABLE_WINE_11_IDENTITY, 3),
            (NATIVE_TEXT_EDITING_WINE_11_IDENTITY, 4),
            (REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY, 5),
            (BOUNDED_NATIVE_UI_WINE_11_IDENTITY, 6),
            (BUNDLED_STEAM_UI_WINE_11_IDENTITY, 7),
        ] {
            assert_eq!(
                all.iter().filter(|edge| edge.approved == target).count(),
                count
            );
        }
        assert_eq!(
            all.len(),
            36,
            "All 28 historical edges and 8 successor edges remain"
        );
        assert!(
            BUNDLED_RUNTIME_POLICIES
                .iter()
                .all(|edge| !edge.verify_launcher_pe)
        );
    }

    #[test]
    fn single_steam_ui_policies_pin_eight_edges_and_keep_historical_packages() {
        for (edge, previous, steam, suffix) in [
            (
                &TLS_TO_SINGLE_STEAM_UI_POLICY,
                TLS_WINE_11_IDENTITY,
                true,
                "tls-to-single-steam-ui",
            ),
            (
                &STEAM_TO_SINGLE_STEAM_UI_POLICY,
                STEAM_WINE_11_IDENTITY,
                true,
                "steam-to-single-steam-ui",
            ),
            (
                &NATIVE_TO_SINGLE_STEAM_UI_POLICY,
                NATIVE_WINE_11_IDENTITY,
                false,
                "native-to-single-steam-ui",
            ),
            (
                &SWITCHABLE_TO_SINGLE_STEAM_UI_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                false,
                "switchable-to-single-steam-ui",
            ),
            (
                &NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
                "native-text-editing-to-single-steam-ui",
            ),
            (
                &REFINED_NATIVE_TEXT_TO_SINGLE_STEAM_UI_POLICY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                false,
                "refined-native-ui-to-single-steam-ui",
            ),
            (
                &BOUNDED_NATIVE_UI_TO_SINGLE_STEAM_UI_POLICY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                false,
                "bounded-native-ui-to-single-steam-ui",
            ),
            (
                &BUNDLED_STEAM_UI_TO_SINGLE_STEAM_UI_POLICY,
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
                false,
                "bundled-steam-ui-to-single-steam-ui",
            ),
        ] {
            assert_eq!(edge.previous, previous);
            assert_eq!(edge.approved, SINGLE_STEAM_UI_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            assert_eq!(edge.backup_suffix, suffix);
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        let all: Vec<_> = STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .filter(|edge| {
                edge.approved != CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
                    && edge.approved != RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
            })
            .copied()
            .collect();
        let mut suffixes = std::collections::BTreeSet::new();
        let mut pairs = std::collections::BTreeSet::new();
        for edge in &all {
            assert!(suffixes.insert(edge.backup_suffix));
            assert!(pairs.insert((edge.previous, edge.approved)));
            assert_ne!(edge.previous, SINGLE_STEAM_UI_WINE_11_IDENTITY);
        }
        for (target, count) in [
            (STEAM_WINE_11_IDENTITY, 1),
            (NATIVE_WINE_11_IDENTITY, 2),
            (SWITCHABLE_WINE_11_IDENTITY, 3),
            (NATIVE_TEXT_EDITING_WINE_11_IDENTITY, 4),
            (REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY, 5),
            (BOUNDED_NATIVE_UI_WINE_11_IDENTITY, 6),
            (BUNDLED_STEAM_UI_WINE_11_IDENTITY, 7),
            (SINGLE_STEAM_UI_WINE_11_IDENTITY, 8),
        ] {
            assert_eq!(
                all.iter().filter(|edge| edge.approved == target).count(),
                count
            );
        }
        assert_eq!(all.len(), 36);
        assert!(
            BUNDLED_RUNTIME_POLICIES
                .iter()
                .all(|edge| !edge.verify_launcher_pe)
        );
    }

    fn assert_direct_edges_without_chaining(
        sources: &[&str],
        target: &str,
        select_policy: for<'a> fn(&str, &'a str) -> MigrationPolicy<'a>,
    ) {
        for &source in sources {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(source);
            let steam = matches!(source, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY);
            if !steam {
                let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
                fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
                fs::remove_file(dir.join("launchers.json")).unwrap();
                fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
            }
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let production = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            let policies: Vec<_> = production
                .iter()
                .map(|edge| MigrationPolicy {
                    previous: edge.previous,
                    approved: if edge.approved == target {
                        &approved
                    } else {
                        edge.approved
                    },
                    backup_suffix: edge.backup_suffix,
                    verify_launcher_pe: edge.verify_launcher_pe,
                })
                .collect();
            let policy_refs: Vec<_> = policies.iter().collect();
            let selected = select_policy(source, &approved);
            let rollback = {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                if steam {
                    migrate_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &fixture.steam.canonicalize().unwrap(),
                        &policy_refs,
                        Duration::from_secs(2),
                    )
                } else {
                    migrate_bundled_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &policy_refs,
                        Duration::from_secs(2),
                    )
                }
            }
            .unwrap()
            .unwrap();
            assert_eq!(bottle_contents(&fixture), contents, "{source}");
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(&selected)).unwrap(),
                original
            );
            assert_eq!(
                fs::read_dir(fixture.home.join("runtime-migration-backups"))
                    .unwrap()
                    .count(),
                1
            );
            let record: PrefixRecord =
                serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
            assert_eq!(record.runtime_identity.as_deref(), Some(approved.as_str()));
            assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
            rollback.restore().unwrap();
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents, "{source}");
        }
    }

    #[test]
    fn refined_text_editing_direct_edges_select_one_final_target_without_chaining() {
        assert_direct_edges_without_chaining(
            &[
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            ],
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            test_refined_successor_policy,
        );
    }

    #[test]
    fn bounded_native_ui_direct_edges_select_all_six_predecessors_without_chaining() {
        assert_direct_edges_without_chaining(
            &[
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            ],
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
            test_bounded_native_ui_policy,
        );
    }

    #[test]
    fn bundled_steam_ui_direct_edges_select_all_seven_predecessors_without_chaining() {
        assert_direct_edges_without_chaining(
            &[
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
            ],
            BUNDLED_STEAM_UI_WINE_11_IDENTITY,
            test_bundled_steam_ui_policy,
        );
    }

    #[test]
    fn single_steam_ui_direct_edges_select_all_eight_predecessors_without_chaining() {
        assert_direct_edges_without_chaining(
            &[
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
            ],
            SINGLE_STEAM_UI_WINE_11_IDENTITY,
            test_single_steam_ui_policy,
        );
    }

    #[test]
    fn single_steam_ui_edges_do_not_misroute_any_historical_destination() {
        for old in STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .filter(|edge| {
                edge.approved != SINGLE_STEAM_UI_WINE_11_IDENTITY
                    && edge.approved != CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
                    && edge.approved != RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
            })
        {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(old.previous);
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let historical = MigrationPolicy {
                previous: old.previous,
                approved: &approved,
                backup_suffix: old.backup_suffix,
                verify_launcher_pe: old.verify_launcher_pe,
            };
            let successor =
                test_single_steam_ui_policy(old.previous, SINGLE_STEAM_UI_WINE_11_IDENTITY);
            let policies = [&successor, &historical];
            let rollback = {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                if matches!(old.previous, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY) {
                    migrate_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &fixture.steam.canonicalize().unwrap(),
                        &policies,
                        Duration::from_secs(2),
                    )
                } else {
                    migrate_bundled_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &policies,
                        Duration::from_secs(2),
                    )
                }
            }
            .unwrap()
            .unwrap();
            assert_eq!(bottle_contents(&fixture), contents);
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(&historical)).unwrap(),
                original
            );
            assert!(!fixture.skipped_switchable_backup(&successor).exists());
            assert_eq!(
                fs::read_dir(fixture.home.join("runtime-migration-backups"))
                    .unwrap()
                    .count(),
                1
            );
            rollback.restore().unwrap();
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
        }
    }

    #[test]
    fn native_ui_edges_do_not_misroute_old_destinations_for_same_predecessor() {
        for destination in [
            SWITCHABLE_WINE_11_IDENTITY,
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
        ] {
            for source in [
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            ] {
                if source == destination
                    || (destination == SWITCHABLE_WINE_11_IDENTITY
                        && matches!(
                            source,
                            NATIVE_TEXT_EDITING_WINE_11_IDENTITY
                                | REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY
                        ))
                    || (destination == NATIVE_TEXT_EDITING_WINE_11_IDENTITY
                        && source == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY)
                {
                    continue;
                }
                let fixture = Fixture::new();
                prepare_text_editing_fixture(&fixture);
                fixture.set_predecessor(source);
                let original = fs::read(&fixture.record_path).unwrap();
                let contents = bottle_contents(&fixture);
                let approved = fixture.approved_identity();
                let steam = matches!(source, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY);
                let old = if destination == BOUNDED_NATIVE_UI_WINE_11_IDENTITY {
                    test_bounded_native_ui_policy(source, &approved)
                } else if destination == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                    test_refined_successor_policy(source, &approved)
                } else if destination == NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                    test_text_editing_policy(source, &approved)
                } else if steam {
                    test_skipped_switchable_policy(source, &approved)
                } else {
                    fixture.switchable_policy(&approved)
                };
                let new = test_bundled_steam_ui_policy(source, BUNDLED_STEAM_UI_WINE_11_IDENTITY);
                // The different final destination appears first. Only the
                // exact destination attestation may select the older edge.
                let policies = [&new, &old];
                let rollback = {
                    let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                    if steam {
                        migrate_with_policies(
                            &fixture.home,
                            &fixture.bottle_id,
                            &fixture.wine,
                            &fixture.steam.canonicalize().unwrap(),
                            &policies,
                            Duration::from_secs(2),
                        )
                    } else {
                        migrate_bundled_with_policies(
                            &fixture.home,
                            &fixture.bottle_id,
                            &fixture.wine,
                            &policies,
                            Duration::from_secs(2),
                        )
                    }
                }
                .unwrap()
                .unwrap();
                assert_eq!(
                    bottle_contents(&fixture),
                    contents,
                    "{source} {destination}"
                );
                assert_eq!(
                    fs::read(fixture.skipped_switchable_backup(&old)).unwrap(),
                    original
                );
                assert!(!fixture.skipped_switchable_backup(&new).exists());
                assert_eq!(
                    fs::read_dir(fixture.home.join("runtime-migration-backups"))
                        .unwrap()
                        .count(),
                    1
                );
                rollback.restore().unwrap();
                assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
                assert_eq!(
                    bottle_contents(&fixture),
                    contents,
                    "{source} {destination}"
                );
            }
        }
    }

    #[test]
    fn older_refined_text_editing_edges_require_steam_recipe_pe_and_exact_package() {
        for target in [
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
            BUNDLED_STEAM_UI_WINE_11_IDENTITY,
            SINGLE_STEAM_UI_WINE_11_IDENTITY,
            CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
        ] {
            for source in [TLS_WINE_11_IDENTITY, STEAM_WINE_11_IDENTITY] {
                for invalid in ["catalog", "launcher", "pe", "package"] {
                    let fixture = Fixture::new();
                    prepare_text_editing_fixture(&fixture);
                    fixture.set_predecessor(source);
                    let approved = fixture.approved_identity();
                    let destination = if invalid == "package" {
                        target
                    } else {
                        &approved
                    };
                    let policy = if target == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                        test_refined_successor_policy(source, destination)
                    } else if target == BOUNDED_NATIVE_UI_WINE_11_IDENTITY {
                        test_bounded_native_ui_policy(source, destination)
                    } else if target == BUNDLED_STEAM_UI_WINE_11_IDENTITY {
                        test_bundled_steam_ui_policy(source, destination)
                    } else if target == SINGLE_STEAM_UI_WINE_11_IDENTITY {
                        test_single_steam_ui_policy(source, destination)
                    } else {
                        test_captured_text_context_policy(source, destination)
                    };
                    let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
                    match invalid {
                        "catalog" => fs::remove_file(dir.join("catalog-attempt.json")).unwrap(),
                        "launcher" => {
                            save_json(&dir.join("launchers.json"), &Vec::<Launcher>::new()).unwrap()
                        }
                        "pe" => fs::write(&fixture.steam, vec![b'x'; 100 * 1024]).unwrap(),
                        "package" => {}
                        _ => unreachable!(),
                    }
                    let original = fs::read(&fixture.record_path).unwrap();
                    let contents = bottle_contents(&fixture);
                    let result = fixture.migrate(&policy);
                    if matches!(invalid, "catalog" | "launcher") {
                        assert!(result.unwrap().is_none(), "{source} {invalid}");
                    } else {
                        assert!(result.is_err(), "{source} {invalid}");
                    }
                    assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
                    assert_eq!(bottle_contents(&fixture), contents);
                    assert!(!fixture.skipped_switchable_backup(&policy).exists());
                }
            }
        }
    }

    #[test]
    fn bounded_native_ui_transition_preserves_two_earlier_backups_and_restores_deployed_record() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        fs::remove_file(dir.join("launchers.json")).unwrap();
        fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
        let approved = fixture.approved_identity();
        fixture.set_predecessor(SWITCHABLE_WINE_11_IDENTITY);
        let first_record = fs::read(&fixture.record_path).unwrap();
        let first_backup = fixture.text_editing_backup();
        fs::create_dir_all(first_backup.parent().unwrap()).unwrap();
        retain_original_record(&first_backup, &first_record).unwrap();
        fixture.set_predecessor(NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
        let second_record = fs::read(&fixture.record_path).unwrap();
        let second_backup =
            fixture.skipped_switchable_backup(&test_refined_text_editing_policy(&approved));
        retain_original_record(&second_backup, &second_record).unwrap();
        fixture.set_predecessor(REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let policy =
            test_bounded_native_ui_policy(REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY, &approved);
        let current_backup = fixture.skipped_switchable_backup(&policy);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&first_backup).unwrap(), first_record);
        assert_eq!(fs::read(&second_backup).unwrap(), second_record);
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(
            fs::read_dir(fixture.home.join("runtime-migration-backups"))
                .unwrap()
                .count(),
            3
        );
        assert_eq!(bottle_contents(&fixture), contents);
        let record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&first_backup).unwrap(), first_record);
        assert_eq!(fs::read(&second_backup).unwrap(), second_record);
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn bundled_steam_ui_transition_keeps_three_earlier_backups_and_restores_deployed_record() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        fs::remove_file(dir.join("launchers.json")).unwrap();
        fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
        let approved = fixture.approved_identity();
        let mut earlier = Vec::new();
        for (source, policy) in [
            (
                SWITCHABLE_WINE_11_IDENTITY,
                test_text_editing_policy(SWITCHABLE_WINE_11_IDENTITY, &approved),
            ),
            (
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                test_refined_text_editing_policy(&approved),
            ),
            (
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                test_bounded_native_ui_policy(
                    REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                    &approved,
                ),
            ),
        ] {
            fixture.set_predecessor(source);
            let record = fs::read(&fixture.record_path).unwrap();
            let backup = fixture.skipped_switchable_backup(&policy);
            fs::create_dir_all(backup.parent().unwrap()).unwrap();
            retain_original_record(&backup, &record).unwrap();
            let metadata = fs::metadata(&backup).unwrap();
            earlier.push(PreservedEntry {
                path: backup,
                contents_or_link: record,
                mode: metadata.mode(),
                inode: metadata.ino(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
            });
        }
        fixture.set_predecessor(BOUNDED_NATIVE_UI_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let policy = test_bundled_steam_ui_policy(BOUNDED_NATIVE_UI_WINE_11_IDENTITY, &approved);
        let current_backup = fixture.skipped_switchable_backup(&policy);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(
            fs::read_dir(fixture.home.join("runtime-migration-backups"))
                .unwrap()
                .count(),
            4
        );
        assert_eq!(bottle_contents(&fixture), contents);
        let record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
        for backup in earlier {
            let metadata = fs::metadata(&backup.path).unwrap();
            assert_eq!(fs::read(&backup.path).unwrap(), backup.contents_or_link);
            assert_eq!(metadata.mode(), backup.mode);
            assert_eq!(metadata.ino(), backup.inode);
            assert_eq!((metadata.mtime(), metadata.mtime_nsec()), backup.modified);
        }
    }

    #[test]
    fn single_steam_ui_transition_keeps_four_earlier_backups_and_restores_deployed_record() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        fs::remove_file(dir.join("launchers.json")).unwrap();
        fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
        let approved = fixture.approved_identity();
        let mut earlier = Vec::new();
        for (source, policy) in [
            (
                SWITCHABLE_WINE_11_IDENTITY,
                test_text_editing_policy(SWITCHABLE_WINE_11_IDENTITY, &approved),
            ),
            (
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                test_refined_text_editing_policy(&approved),
            ),
            (
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                test_bounded_native_ui_policy(
                    REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                    &approved,
                ),
            ),
            (
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                test_bundled_steam_ui_policy(BOUNDED_NATIVE_UI_WINE_11_IDENTITY, &approved),
            ),
        ] {
            fixture.set_predecessor(source);
            let record = fs::read(&fixture.record_path).unwrap();
            let backup = fixture.skipped_switchable_backup(&policy);
            fs::create_dir_all(backup.parent().unwrap()).unwrap();
            retain_original_record(&backup, &record).unwrap();
            let metadata = fs::metadata(&backup).unwrap();
            earlier.push(PreservedEntry {
                path: backup,
                contents_or_link: record,
                mode: metadata.mode(),
                inode: metadata.ino(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
            });
        }
        fixture.set_predecessor(BUNDLED_STEAM_UI_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let policy = test_single_steam_ui_policy(BUNDLED_STEAM_UI_WINE_11_IDENTITY, &approved);
        let current_backup = fixture.skipped_switchable_backup(&policy);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(
            fs::read_dir(fixture.home.join("runtime-migration-backups"))
                .unwrap()
                .count(),
            5
        );
        assert_eq!(bottle_contents(&fixture), contents);
        let record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
        for backup in earlier {
            let metadata = fs::metadata(&backup.path).unwrap();
            assert_eq!(fs::read(&backup.path).unwrap(), backup.contents_or_link);
            assert_eq!(metadata.mode(), backup.mode);
            assert_eq!(metadata.ino(), backup.inode);
            assert_eq!((metadata.mtime(), metadata.mtime_nsec()), backup.modified);
        }
    }

    #[test]
    fn bundled_steam_ui_transition_waits_for_the_bottle_lock_without_early_changes() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        fixture.set_predecessor(BOUNDED_NATIVE_UI_WINE_11_IDENTITY);
        let approved = fixture.approved_identity();
        let policy = test_bundled_steam_ui_policy(BOUNDED_NATIVE_UI_WINE_11_IDENTITY, &approved);
        let lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                ready_tx.send(()).unwrap();
                done_tx.send(fixture.migrate_bundled(&policy)).unwrap();
            });
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(matches!(
                done_rx.recv_timeout(Duration::from_millis(100)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
            drop(lock);
            let rollback = done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .unwrap();
            worker.join().unwrap();
            rollback.restore().unwrap();
        });
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn single_steam_ui_transition_waits_for_the_bottle_lock_without_early_changes() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        fixture.set_predecessor(BUNDLED_STEAM_UI_WINE_11_IDENTITY);
        let approved = fixture.approved_identity();
        let policy = test_single_steam_ui_policy(BUNDLED_STEAM_UI_WINE_11_IDENTITY, &approved);
        let lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                ready_tx.send(()).unwrap();
                done_tx.send(fixture.migrate_bundled(&policy)).unwrap();
            });
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(matches!(
                done_rx.recv_timeout(Duration::from_millis(100)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
            drop(lock);
            let rollback = done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .unwrap();
            worker.join().unwrap();
            rollback.restore().unwrap();
        });
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn refined_text_editing_transition_preserves_bottle_and_earlier_record_backup() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        fs::remove_file(dir.join("launchers.json")).unwrap();
        fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
        let earlier_record = fs::read(&fixture.record_path).unwrap();
        fs::create_dir_all(fixture.text_editing_backup().parent().unwrap()).unwrap();
        retain_original_record(&fixture.text_editing_backup(), &earlier_record).unwrap();
        fixture.set_predecessor(NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let approved = fixture.approved_identity();
        let policy = test_refined_text_editing_policy(&approved);
        let backup = fixture.skipped_switchable_backup(&policy);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&backup).unwrap(), original);
        assert_eq!(
            fs::read(fixture.text_editing_backup()).unwrap(),
            earlier_record
        );
        assert_eq!(bottle_contents(&fixture), contents);
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&backup).unwrap(), original);
        assert_eq!(
            fs::read(fixture.text_editing_backup()).unwrap(),
            earlier_record
        );
        assert_eq!(bottle_contents(&fixture), contents);
    }

    #[test]
    fn refined_text_editing_transition_refuses_live_tracked_and_untracked_owners() {
        use std::io::BufRead;
        struct Owner(Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        for previous in [
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
            BUNDLED_STEAM_UI_WINE_11_IDENTITY,
            SINGLE_STEAM_UI_WINE_11_IDENTITY,
        ] {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(previous);
            let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
            let active = dir.join("active-processes.json");
            save_json(&active, &vec![std::process::id()]).unwrap();
            let original = fs::read(&fixture.record_path).unwrap();
            let tracked_contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let policy = if previous == NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                test_refined_text_editing_policy(&approved)
            } else if previous == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                test_bounded_native_ui_policy(previous, &approved)
            } else if previous == BOUNDED_NATIVE_UI_WINE_11_IDENTITY {
                test_bundled_steam_ui_policy(previous, &approved)
            } else if previous == BUNDLED_STEAM_UI_WINE_11_IDENTITY {
                test_single_steam_ui_policy(previous, &approved)
            } else if previous == SINGLE_STEAM_UI_WINE_11_IDENTITY {
                test_captured_text_context_policy(previous, &approved)
            } else {
                test_recognized_text_popup_policy(previous, &approved)
            };
            assert!(fixture.migrate_bundled(&policy).is_err());
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), tracked_contents);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
            fs::remove_file(&active).unwrap();
            let held = dir.join("prefix/keep.txt");
            let mut owner = Owner(
                Command::new("/bin/sh")
                    .args([
                        "-c",
                        "exec 3<\"$1\"; printf 'ready\\n'; read line; exit 0",
                        "fixture",
                    ])
                    .arg(&held)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let mut ready = String::new();
            std::io::BufReader::new(owner.0.stdout.take().unwrap())
                .read_line(&mut ready)
                .unwrap();
            assert_eq!(ready, "ready\n");
            let contents = bottle_contents(&fixture);
            assert!(fixture.migrate_bundled(&policy).is_err());
            assert!(owner.0.try_wait().unwrap().is_none());
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
            drop(owner);
            assert!(fixture.migrate_bundled(&policy).unwrap().is_some());
            assert_eq!(bottle_contents(&fixture), contents);
        }
    }

    #[test]
    fn refined_text_editing_transition_refuses_unapproved_destination_without_changes() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        fixture.set_predecessor(NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let policy = test_refined_text_editing_policy("sha256:unapproved-refined-package");
        assert!(fixture.migrate_bundled(&policy).is_err());
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
        assert!(!fixture.skipped_switchable_backup(&policy).exists());
    }

    #[test]
    fn refined_text_editing_transition_keeps_custom_legacy_and_unbound_records_strict() {
        for invalid in ["custom", "path-only", "unknown", "unbound"] {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(NATIVE_TEXT_EDITING_WINE_11_IDENTITY);
            let mut record: PrefixRecord =
                serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
            match invalid {
                "custom" => {
                    record.wine_path = fixture.home.join("custom-wine").display().to_string()
                }
                "path-only" => record.runtime_identity = None,
                "unknown" => record.runtime_identity = Some("sha256:unknown-predecessor".into()),
                "unbound" => record.runtime_fingerprint = Some("unbound".into()),
                _ => unreachable!(),
            }
            save_json(&fixture.record_path, &record).unwrap();
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let policy = test_refined_text_editing_policy(&approved);
            assert!(
                fixture.migrate_bundled(&policy).unwrap().is_none(),
                "{invalid}"
            );
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
        }
    }

    #[test]
    fn refined_text_editing_transition_rolls_back_if_package_changes_during_idle_check() {
        for previous in [
            NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
            BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
            BUNDLED_STEAM_UI_WINE_11_IDENTITY,
            SINGLE_STEAM_UI_WINE_11_IDENTITY,
        ] {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(previous);
            fs::write(
                fixture.wine.parent().unwrap().join("_dispatch"),
                b"#!/bin/sh\ncase \"${0##*/}\" in wineserver) printf changed >> \"${0%/*}/../runtime/bin/wine\";; *) exit 9;; esac\nexit 0\n",
            )
            .unwrap();
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let policy = if previous == NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                test_refined_text_editing_policy(&approved)
            } else if previous == REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY {
                test_bounded_native_ui_policy(previous, &approved)
            } else if previous == BOUNDED_NATIVE_UI_WINE_11_IDENTITY {
                test_bundled_steam_ui_policy(previous, &approved)
            } else if previous == BUNDLED_STEAM_UI_WINE_11_IDENTITY {
                test_single_steam_ui_policy(previous, &approved)
            } else if previous == SINGLE_STEAM_UI_WINE_11_IDENTITY {
                test_captured_text_context_policy(previous, &approved)
            } else {
                test_recognized_text_popup_policy(previous, &approved)
            };
            let error = fixture.migrate_bundled(&policy).err().unwrap();
            assert!(
                error.contains("original runtime record was restored"),
                "{error}"
            );
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(&policy)).unwrap(),
                original
            );
            assert_eq!(bottle_contents(&fixture), contents);
        }
    }
    #[test]
    fn captured_text_context_policies_pin_nine_edges_and_keep_historical_packages() {
        for (edge, previous, suffix) in [
            (
                &TLS_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                TLS_WINE_11_IDENTITY,
                "tls-to-captured-text-context",
            ),
            (
                &STEAM_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                STEAM_WINE_11_IDENTITY,
                "steam-to-captured-text-context",
            ),
            (
                &NATIVE_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                NATIVE_WINE_11_IDENTITY,
                "native-to-captured-text-context",
            ),
            (
                &SWITCHABLE_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                "switchable-to-captured-text-context",
            ),
            (
                &NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                "native-text-editing-to-captured-text-context",
            ),
            (
                &REFINED_NATIVE_TEXT_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                "refined-native-ui-to-captured-text-context",
            ),
            (
                &BOUNDED_NATIVE_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                "bounded-native-ui-to-captured-text-context",
            ),
            (
                &BUNDLED_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
                "bundled-steam-ui-to-captured-text-context",
            ),
            (
                &SINGLE_STEAM_UI_TO_CAPTURED_TEXT_CONTEXT_POLICY,
                SINGLE_STEAM_UI_WINE_11_IDENTITY,
                "single-steam-ui-to-captured-text-context",
            ),
        ] {
            assert_eq!(edge.previous, previous);
            assert_eq!(edge.approved, CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY);
            assert_eq!(edge.backup_suffix, suffix);
            let steam = matches!(previous, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        let all: Vec<_> = STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .copied()
            .collect();
        let mut suffixes = std::collections::BTreeSet::new();
        let mut pairs = std::collections::BTreeSet::new();
        for edge in &all {
            assert!(suffixes.insert(edge.backup_suffix));
            assert!(pairs.insert((edge.previous, edge.approved)));
            assert_ne!(edge.previous, RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY);
        }
        for (target, count) in [
            (STEAM_WINE_11_IDENTITY, 1),
            (NATIVE_WINE_11_IDENTITY, 2),
            (SWITCHABLE_WINE_11_IDENTITY, 3),
            (NATIVE_TEXT_EDITING_WINE_11_IDENTITY, 4),
            (REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY, 5),
            (BOUNDED_NATIVE_UI_WINE_11_IDENTITY, 6),
            (BUNDLED_STEAM_UI_WINE_11_IDENTITY, 7),
            (SINGLE_STEAM_UI_WINE_11_IDENTITY, 8),
            (CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY, 9),
            (RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY, 10),
        ] {
            assert_eq!(
                all.iter().filter(|edge| edge.approved == target).count(),
                count
            );
        }
        assert_eq!(
            all.len(),
            55,
            "All 36 historical edges, 9 captured-popup edges and 10 successor edges remain"
        );
        assert!(
            BUNDLED_RUNTIME_POLICIES
                .iter()
                .all(|edge| !edge.verify_launcher_pe)
        );
    }

    #[test]
    fn recognized_text_popup_policies_pin_ten_edges_and_keep_historical_packages() {
        for (edge, previous, suffix) in [
            (
                &TLS_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                TLS_WINE_11_IDENTITY,
                "tls-to-recognized-text-popup",
            ),
            (
                &STEAM_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                STEAM_WINE_11_IDENTITY,
                "steam-to-recognized-text-popup",
            ),
            (
                &NATIVE_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                NATIVE_WINE_11_IDENTITY,
                "native-to-recognized-text-popup",
            ),
            (
                &SWITCHABLE_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                SWITCHABLE_WINE_11_IDENTITY,
                "switchable-to-recognized-text-popup",
            ),
            (
                &NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                "native-text-editing-to-recognized-text-popup",
            ),
            (
                &REFINED_NATIVE_TEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                "refined-native-ui-to-recognized-text-popup",
            ),
            (
                &BOUNDED_NATIVE_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                "bounded-native-ui-to-recognized-text-popup",
            ),
            (
                &BUNDLED_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
                "bundled-steam-ui-to-recognized-text-popup",
            ),
            (
                &SINGLE_STEAM_UI_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                SINGLE_STEAM_UI_WINE_11_IDENTITY,
                "single-steam-ui-to-recognized-text-popup",
            ),
            (
                &CAPTURED_TEXT_CONTEXT_TO_RECOGNIZED_TEXT_POPUP_POLICY,
                CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
                "captured-text-context-to-recognized-text-popup",
            ),
        ] {
            assert_eq!(edge.previous, previous);
            assert_eq!(edge.approved, RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY);
            assert_eq!(edge.backup_suffix, suffix);
            let steam = matches!(previous, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY);
            assert_eq!(edge.verify_launcher_pe, steam);
            let policies = if steam {
                &STEAM_RUNTIME_POLICIES[..]
            } else {
                &BUNDLED_RUNTIME_POLICIES[..]
            };
            assert!(
                policies
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, edge))
            );
        }
        assert!(is_reviewed_switchable_runtime(
            RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
        ));
    }

    #[test]
    fn recognized_text_popup_direct_edges_select_all_ten_predecessors_without_chaining() {
        assert_direct_edges_without_chaining(
            &[
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
                SINGLE_STEAM_UI_WINE_11_IDENTITY,
                CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
            ],
            RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY,
            test_recognized_text_popup_policy,
        );
    }

    #[test]
    fn captured_text_context_direct_edges_select_all_nine_predecessors_without_chaining() {
        assert_direct_edges_without_chaining(
            &[
                TLS_WINE_11_IDENTITY,
                STEAM_WINE_11_IDENTITY,
                NATIVE_WINE_11_IDENTITY,
                SWITCHABLE_WINE_11_IDENTITY,
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
                SINGLE_STEAM_UI_WINE_11_IDENTITY,
            ],
            CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
            test_captured_text_context_policy,
        );
    }

    #[test]
    fn captured_text_context_edges_do_not_misroute_any_historical_destination() {
        for old in STEAM_RUNTIME_POLICIES
            .iter()
            .chain(BUNDLED_RUNTIME_POLICIES.iter())
            .filter(|edge| {
                edge.approved != CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY
                    && edge.approved != RECOGNIZED_TEXT_POPUP_WINE_11_IDENTITY
            })
        {
            let fixture = Fixture::new();
            prepare_text_editing_fixture(&fixture);
            fixture.set_predecessor(old.previous);
            let original = fs::read(&fixture.record_path).unwrap();
            let contents = bottle_contents(&fixture);
            let approved = fixture.approved_identity();
            let historical = MigrationPolicy {
                previous: old.previous,
                approved: &approved,
                backup_suffix: old.backup_suffix,
                verify_launcher_pe: old.verify_launcher_pe,
            };
            let successor = test_captured_text_context_policy(
                old.previous,
                CAPTURED_TEXT_CONTEXT_WINE_11_IDENTITY,
            );
            let policies = [&successor, &historical];
            let rollback = {
                let _lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
                if matches!(old.previous, TLS_WINE_11_IDENTITY | STEAM_WINE_11_IDENTITY) {
                    migrate_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &fixture.steam.canonicalize().unwrap(),
                        &policies,
                        Duration::from_secs(2),
                    )
                } else {
                    migrate_bundled_with_policies(
                        &fixture.home,
                        &fixture.bottle_id,
                        &fixture.wine,
                        &policies,
                        Duration::from_secs(2),
                    )
                }
            }
            .unwrap()
            .unwrap();
            assert_eq!(bottle_contents(&fixture), contents);
            assert_eq!(
                fs::read(fixture.skipped_switchable_backup(&historical)).unwrap(),
                original
            );
            assert!(!fixture.skipped_switchable_backup(&successor).exists());
            assert_eq!(
                fs::read_dir(fixture.home.join("runtime-migration-backups"))
                    .unwrap()
                    .count(),
                1
            );
            rollback.restore().unwrap();
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
        }
    }

    #[test]
    fn captured_text_context_transition_keeps_five_earlier_backups_and_restores_deployed_record() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        let dir = bottle_dir(&fixture.home, &fixture.bottle_id).unwrap();
        fs::remove_file(dir.join("catalog-attempt.json")).unwrap();
        fs::remove_file(dir.join("launchers.json")).unwrap();
        fs::remove_dir_all(fixture.steam.parent().unwrap()).unwrap();
        let approved = fixture.approved_identity();
        let mut earlier = Vec::new();
        for (source, policy) in [
            (
                SWITCHABLE_WINE_11_IDENTITY,
                test_text_editing_policy(SWITCHABLE_WINE_11_IDENTITY, &approved),
            ),
            (
                NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                test_refined_text_editing_policy(&approved),
            ),
            (
                REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                test_bounded_native_ui_policy(
                    REFINED_NATIVE_TEXT_EDITING_WINE_11_IDENTITY,
                    &approved,
                ),
            ),
            (
                BOUNDED_NATIVE_UI_WINE_11_IDENTITY,
                test_bundled_steam_ui_policy(BOUNDED_NATIVE_UI_WINE_11_IDENTITY, &approved),
            ),
            (
                BUNDLED_STEAM_UI_WINE_11_IDENTITY,
                test_single_steam_ui_policy(BUNDLED_STEAM_UI_WINE_11_IDENTITY, &approved),
            ),
        ] {
            fixture.set_predecessor(source);
            let record = fs::read(&fixture.record_path).unwrap();
            let backup = fixture.skipped_switchable_backup(&policy);
            fs::create_dir_all(backup.parent().unwrap()).unwrap();
            retain_original_record(&backup, &record).unwrap();
            let metadata = fs::metadata(&backup).unwrap();
            earlier.push(PreservedEntry {
                path: backup,
                contents_or_link: record,
                mode: metadata.mode(),
                inode: metadata.ino(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
            });
        }
        fixture.set_predecessor(SINGLE_STEAM_UI_WINE_11_IDENTITY);
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let policy = test_captured_text_context_policy(SINGLE_STEAM_UI_WINE_11_IDENTITY, &approved);
        let current_backup = fixture.skipped_switchable_backup(&policy);
        let rollback = fixture.migrate_bundled(&policy).unwrap().unwrap();
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(
            fs::read_dir(fixture.home.join("runtime-migration-backups"))
                .unwrap()
                .count(),
            6
        );
        assert_eq!(bottle_contents(&fixture), contents);
        let record: PrefixRecord =
            serde_json::from_slice(&fs::read(&fixture.record_path).unwrap()).unwrap();
        assert!(runtime_record_matches(&record, &fixture.wine).unwrap());
        assert!(fixture.migrate_bundled(&policy).unwrap().is_none());
        rollback.restore().unwrap();
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(fs::read(&current_backup).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
        for backup in earlier {
            let metadata = fs::metadata(&backup.path).unwrap();
            assert_eq!(fs::read(&backup.path).unwrap(), backup.contents_or_link);
            assert_eq!(metadata.mode(), backup.mode);
            assert_eq!(metadata.ino(), backup.inode);
            assert_eq!((metadata.mtime(), metadata.mtime_nsec()), backup.modified);
        }
    }

    #[test]
    fn captured_text_context_transition_waits_for_the_bottle_lock_without_early_changes() {
        let fixture = Fixture::new();
        prepare_text_editing_fixture(&fixture);
        fixture.set_predecessor(SINGLE_STEAM_UI_WINE_11_IDENTITY);
        let approved = fixture.approved_identity();
        let policy = test_captured_text_context_policy(SINGLE_STEAM_UI_WINE_11_IDENTITY, &approved);
        let lock = lock_bottle(&fixture.home, &fixture.bottle_id).unwrap();
        let original = fs::read(&fixture.record_path).unwrap();
        let contents = bottle_contents(&fixture);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                ready_tx.send(()).unwrap();
                done_tx.send(fixture.migrate_bundled(&policy)).unwrap();
            });
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(matches!(
                done_rx.recv_timeout(Duration::from_millis(100)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
            assert_eq!(bottle_contents(&fixture), contents);
            assert!(!fixture.skipped_switchable_backup(&policy).exists());
            drop(lock);
            let rollback = done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .unwrap();
            worker.join().unwrap();
            rollback.restore().unwrap();
        });
        assert_eq!(fs::read(&fixture.record_path).unwrap(), original);
        assert_eq!(bottle_contents(&fixture), contents);
    }
}
