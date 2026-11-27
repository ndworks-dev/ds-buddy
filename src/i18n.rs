//! The app speaks Brazilian Portuguese or English. By default it follows the
//! Windows display language (Portuguese there, English anywhere else); the
//! Settings page can pin either one. Slint translates the window's text from
//! ui/translations; this module tells it which language to use and covers the
//! text built in Rust, so the window and its messages never disagree.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings;

/// Overrides the saved choice for one run, for screenshots and testing:
/// `DS_BUDDY_LANG=en` or `DS_BUDDY_LANG=pt-BR`.
const OVERRIDE: &str = "DS_BUDDY_LANG";
const SETTING: &str = "Language";

/// Picks the Portuguese or the English text, then formats it like `format!`.
macro_rules! tr {
    ($en:literal, $pt:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::portuguese() { format!($pt $(, $arg)*) } else { format!($en $(, $arg)*) }
    };
}

/// What the user chose. The order matches the options on the Settings page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Automatic,
    Portuguese,
    English,
}

impl Language {
    pub fn index(self) -> i32 {
        self as i32
    }

    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Portuguese,
            2 => Self::English,
            _ => Self::Automatic,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Automatic => "",
            Self::Portuguese => "pt-BR",
            Self::English => "en",
        }
    }

    fn from_code(code: &str) -> Self {
        if code.is_empty() {
            Self::Automatic
        } else if is_portuguese(code) {
            Self::Portuguese
        } else {
            Self::English
        }
    }
}

static PORTUGUESE: AtomicBool = AtomicBool::new(false);

/// Applies the saved choice, or the override, and returns it. Must run after
/// the window is created, since Slint's language belongs to the window.
pub fn init() -> Language {
    let language =
        std::env::var(OVERRIDE).ok().or_else(|| settings::load(SETTING)).map_or(Language::Automatic, |code| Language::from_code(&code));
    apply(language);
    language
}

/// Switches the language right away and remembers it.
pub fn set(language: Language) {
    apply(language);
    let _ = settings::save(SETTING, language.code());
}

fn apply(language: Language) {
    let portuguese = match language {
        Language::Automatic => sys_locale::get_locale().is_some_and(|l| is_portuguese(&l)),
        Language::Portuguese => true,
        Language::English => false,
    };
    PORTUGUESE.store(portuguese, Ordering::Relaxed);
    // An empty name selects the untranslated text, which is English.
    let _ = slint::select_bundled_translation(if portuguese { "pt-BR" } else { "" });
}

pub fn portuguese() -> bool {
    PORTUGUESE.load(Ordering::Relaxed)
}

/// The same choice for text that needs no formatting.
pub fn pick(en: &'static str, pt: &'static str) -> &'static str {
    if portuguese() { pt } else { en }
}

/// Any Portuguese locale. Slint would match pt-BR the same way: by the exact
/// name, or by the part before `-`, `_` or `@`.
fn is_portuguese(locale: &str) -> bool {
    locale.split(['-', '_', '@']).next().is_some_and(|l| l.eq_ignore_ascii_case("pt"))
}

#[cfg(test)]
mod tests {
    use super::{Language, is_portuguese};

    #[test]
    fn reads_saved_choices() {
        assert_eq!(Language::from_code(""), Language::Automatic);
        assert_eq!(Language::from_code("pt-BR"), Language::Portuguese);
        assert_eq!(Language::from_code("en"), Language::English);
        assert_eq!(Language::from_code("fr"), Language::English);
        for language in [Language::Automatic, Language::Portuguese, Language::English] {
            assert_eq!(Language::from_code(language.code()), language);
            assert_eq!(Language::from_index(language.index()), language);
        }
    }

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
