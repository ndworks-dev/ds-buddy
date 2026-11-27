//! The app speaks Brazilian Portuguese on a Portuguese system and English
//! everywhere else. Slint translates the window's text from ui/translations
//! and picks the language from the system locale by itself; this module
//! covers the text built in Rust and decides the same way, so the window and
//! its messages never disagree.

use std::sync::OnceLock;

/// Overrides the system language, for screenshots and testing:
/// `DS_BUDDY_LANG=en` or `DS_BUDDY_LANG=pt-BR`.
const OVERRIDE: &str = "DS_BUDDY_LANG";

/// Picks the Portuguese or the English text, then formats it like `format!`.
macro_rules! tr {
    ($en:literal, $pt:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::portuguese() { format!($pt $(, $arg)*) } else { format!($en $(, $arg)*) }
    };
}

pub fn portuguese() -> bool {
    static PORTUGUESE: OnceLock<bool> = OnceLock::new();
    *PORTUGUESE.get_or_init(|| is_portuguese(&std::env::var(OVERRIDE).ok().or_else(sys_locale::get_locale).unwrap_or_default()))
}

/// The same choice for text that needs no formatting.
pub fn pick(en: &'static str, pt: &'static str) -> &'static str {
    if portuguese() { pt } else { en }
}

/// Points the window at the overridden language, if any. Must run after the
/// window is created.
pub fn apply_override() {
    if std::env::var_os(OVERRIDE).is_some() {
        // An empty name selects the untranslated text, which is English.
        let _ = slint::select_bundled_translation(if portuguese() { "pt-BR" } else { "" });
    }
}

/// Slint matches a bundled language by its exact name or, failing that, by
/// the part before `-`, `_` or `@`. With only pt-BR bundled, that is any
/// Portuguese locale.
fn is_portuguese(locale: &str) -> bool {
    locale.split(['-', '_', '@']).next().is_some_and(|l| l.eq_ignore_ascii_case("pt"))
}

#[cfg(test)]
mod tests {
    use super::is_portuguese;

    /// The strings inside `marker("…")`, for text without escaped quotes.
    fn quoted_after<'a>(text: &'a str, marker: &str) -> Vec<&'a str> {
        text.split(marker).skip(1).filter_map(|rest| rest.split_once('"').map(|(s, _)| s)).collect()
    }

    #[test]
    fn every_window_text_has_a_portuguese_translation() {
        let po = include_str!("../ui/translations/pt-BR/LC_MESSAGES/ds-buddy.po");
        let translated = quoted_after(po, "msgid \"");
        for slint in [include_str!("../ui/app.slint"), include_str!("../ui/widgets.slint")] {
            for text in quoted_after(slint, "@tr(\"") {
                assert!(translated.contains(&text), "no pt-BR entry for {text:?}");
            }
        }
    }

    #[test]
    fn matches_any_portuguese_locale() {
        for locale in ["pt-BR", "pt_BR", "pt-PT", "pt", "PT-br", "pt_BR@latin"] {
            assert!(is_portuguese(locale), "{locale}");
        }
        for locale in ["en-US", "es-ES", "", "ptx", "it"] {
            assert!(!is_portuguese(locale), "{locale}");
        }
    }
}
