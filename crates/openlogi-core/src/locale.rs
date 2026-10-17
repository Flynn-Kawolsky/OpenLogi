//! Locale negotiation: which shipped catalog a BCP-47 code resolves to.
//!
//! Shared by every process that renders localized text — the settings app
//! picks the locale from its stored [`AppSettings`](crate::config::AppSettings),
//! the agent from the same setting for its menu-bar tray, and the overlay
//! helper from the code the agent hands it per invocation. Keeping one
//! implementation is what stops the processes from disagreeing about, say,
//! whether `zh-Hant-HK` is Hong Kong or Taiwan.
//!
//! The catalogs themselves stay in `crates/openlogi-ui/locales/` (each binary
//! expands its own `rust_i18n::i18n!` over that directory by relative path);
//! this module is only the negotiation over their codes, so it can live in the
//! platform-free core where the GUI-less agent can reach it.
//!
//! Setting the locale is global: `rust_i18n` stores it in a process-wide atomic
//! that gpui-component reads too, so in the settings app
//! [`crate::locale::activate`] re-localizes the framework's own widget strings
//! alongside ours.

use fluent_langneg::{LanguageIdentifier, NegotiationStrategy, negotiate_languages};

/// Locales the GUI ships, as `(code, native name)`. The codes match the
/// `locales/*.toml` filenames, and both also match gpui-component's bundled
/// `ui.yml`, so choosing one localizes the framework's own widgets too.
///
/// This build ships English and Traditional Chinese (Taiwan) only; every other
/// language resolves to English (see `match_supported`).
/// Order here is the order shown in the Settings picker (after "Follow system").
pub const SUPPORTED: &[(&str, &str)] = &[("en", "English"), ("zh-TW", "正體中文（臺灣）")];

/// Resolve the locale to apply, preferring an explicit stored `setting`, then
/// the system locale, and finally `"en"`. An unrecognized stored code is
/// treated as "follow system" rather than failing.
fn resolve(setting: Option<&str>) -> &'static str {
    setting
        .and_then(match_supported)
        .or_else(|| {
            sys_locale::get_locale()
                .as_deref()
                .and_then(match_supported)
        })
        .unwrap_or("en")
}

/// Collapse an arbitrary BCP-47 locale onto one of [`SUPPORTED`], or `None`,
/// by matching its primary subtag.
///
/// Chinese is decided by [`chinese_locale`] and never reaches the lookup:
/// plain negotiation would happily hand `zh-CN` the `zh-TW` catalog by
/// language alone, serving a Simplified-Chinese reader Traditional text.
fn match_supported(code: &str) -> Option<&'static str> {
    let requested = code.replace('_', "-").parse::<LanguageIdentifier>().ok()?;
    if requested.language.as_str() == "zh" {
        return chinese_locale(&requested);
    }
    lookup_supported(&requested)
}

/// Only Taiwan's Traditional Chinese has a catalog: a `tw` region, or a bare
/// `Hant` script with no region, → `zh-TW`. An explicit `Hans` script always
/// wins (so `zh-Hans-TW` stays Simplified), and Simplified, Hong Kong / Macau,
/// and region-less `zh` have no catalog, so they resolve to `None` — English.
fn chinese_locale(requested: &LanguageIdentifier) -> Option<&'static str> {
    let script = requested.script.as_ref().map(ToString::to_string);
    let region = requested.region.as_ref().map(ToString::to_string);
    match (script.as_deref(), region.as_deref()) {
        (Some("Hans"), _) => None,
        (_, Some("TW")) | (Some("Hant"), None) => Some("zh-TW"),
        _ => None,
    }
}

fn lookup_supported(requested: &LanguageIdentifier) -> Option<&'static str> {
    let available = supported_langids();
    let matched = negotiate_languages(
        std::slice::from_ref(requested),
        &available,
        None,
        NegotiationStrategy::Lookup,
    )
    .into_iter()
    .next()?;
    let matched = matched.to_string();
    SUPPORTED
        .iter()
        .find_map(|(code, _)| (*code == matched).then_some(*code))
}

fn supported_langids() -> Vec<LanguageIdentifier> {
    SUPPORTED
        .iter()
        .filter_map(|(code, _)| code.parse().ok())
        .collect()
}

/// Switch the process-global locale to the resolution of `language`
/// (`None` = follow system). The single resolve→`set_locale` surface for every
/// caller — app startup, the live Settings switch, and each overlay
/// invocation — so the resolution policy can't drift between them.
///
/// The caller is responsible for refreshing any open window afterwards; views
/// already rendered do not re-read the locale on their own.
pub fn activate(language: Option<&str>) {
    rust_i18n::set_locale(resolve(language));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_locale_variants() {
        assert_eq!(match_supported("zh-Hant-TW"), Some("zh-TW"));
        assert_eq!(match_supported("zh-TW"), Some("zh-TW"));
        assert_eq!(match_supported("zh_TW"), Some("zh-TW"));
        assert_eq!(match_supported("zh-Hant"), Some("zh-TW"));
        assert_eq!(match_supported("en"), Some("en"));
        assert_eq!(match_supported("en-US"), Some("en"));
        assert_eq!(match_supported("en_GB"), Some("en"));
        assert_eq!(match_supported("klingon"), None);
    }

    #[test]
    fn languages_without_a_catalog_fall_back_to_english() {
        // Chinese variants with no catalog must not borrow `zh-TW`'s by
        // language alone.
        for code in [
            "zh",
            "zh-CN",
            "zh-Hans",
            "zh-Hans-CN",
            "zh-Hans-TW",
            "zh-HK",
            "zh-Hant-HK",
            "zh-MO",
        ] {
            assert_eq!(match_supported(code), None, "{code}");
        }
        // Catalogs this build no longer ships.
        for code in ["de", "fr-FR", "ja", "pt-BR", "nb-NO", "cs_CZ"] {
            assert_eq!(match_supported(code), None, "{code}");
        }
        assert_eq!(resolve(Some("zh-CN")), resolve(None));
    }

    #[test]
    fn explicit_setting_wins_over_system() {
        assert_eq!(resolve(Some("zh-TW")), "zh-TW");
        assert_eq!(resolve(Some("en")), "en");
        // An unknown stored code falls through to system/`en`, never panics.
        assert!(
            SUPPORTED
                .iter()
                .any(|(c, _)| *c == resolve(Some("klingon")))
        );
    }
}
