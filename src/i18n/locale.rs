use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguagePreference {
    #[default]
    System,
    English,
    Russian,
}

impl LanguagePreference {
    pub(super) fn resolve(self) -> Locale {
        match self {
            Self::System => Locale::system(),
            Self::English => Locale::English,
            Self::Russian => Locale::Russian,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Locale {
    English,
    Russian,
}

impl Locale {
    pub fn system() -> Self {
        let locale = sys_locale::get_locale();
        match locale.as_deref().and_then(|tag| tag.split('-').next()) {
            Some(language) if language.eq_ignore_ascii_case("ru") => Self::Russian,
            _ => Self::English,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::Russian => "ru",
        }
    }
}
