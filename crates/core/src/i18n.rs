//! Display language for text the core generates (advice, usage names, errors).
//! The CLI stays in Chinese; the desktop app passes the user's choice.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    #[default]
    Zh,
    En,
}

impl Lang {
    /// `zh`, `zh-CN`, `zh_Hans_CN` → Chinese; every other locale gets English.
    #[must_use]
    pub fn from_locale(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Self::Zh
        } else {
            Self::En
        }
    }

    #[must_use]
    pub const fn pick(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }
}

/// `tr!(lang, "发生 {} 次", "{} times", count)`: formats whichever template matches `lang`.
#[macro_export]
macro_rules! tr {
    ($lang:expr, $zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $lang {
            $crate::i18n::Lang::Zh => format!($zh $(, $arg)*),
            $crate::i18n::Lang::En => format!($en $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::Lang;

    #[test]
    fn chinese_locales_resolve_to_chinese() {
        for tag in ["zh", "zh-CN", "zh_Hans_CN", "ZH-tw"] {
            assert_eq!(Lang::from_locale(tag), Lang::Zh);
        }
        for tag in ["en-US", "ja-JP", ""] {
            assert_eq!(Lang::from_locale(tag), Lang::En);
        }
    }

    #[test]
    fn tr_formats_the_matching_template() {
        let count = 3;
        assert_eq!(
            tr!(Lang::Zh, "发生 {count} 次", "{count} times"),
            "发生 3 次"
        );
        assert_eq!(tr!(Lang::En, "发生 {} 次", "{} times", count), "3 times");
    }
}
