//! Setup assistant model (ADR 0004): steps, choices, account draft. Pure; UI in `ui::setup`.
use shared::login::NewAccount;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Language,
    Keyboard,
    Wifi,
    Account,
    Appearance,
    Done,
}

pub const STEPS: [Step; 7] = [
    Step::Welcome,
    Step::Language,
    Step::Keyboard,
    Step::Wifi,
    Step::Account,
    Step::Appearance,
    Step::Done,
];

impl Step {
    pub fn title(self) -> &'static str {
        match self {
            Step::Welcome => "Welcome",
            Step::Language => "Language & Region",
            Step::Keyboard => "Keyboard",
            Step::Wifi => "Wi-Fi",
            Step::Account => "Create Your Account",
            Step::Appearance => "Appearance",
            Step::Done => "You're all set",
        }
    }

    /// Stack page name.
    pub fn id(self) -> &'static str {
        match self {
            Step::Welcome => "welcome",
            Step::Language => "language",
            Step::Keyboard => "keyboard",
            Step::Wifi => "wifi",
            Step::Account => "account",
            Step::Appearance => "appearance",
            Step::Done => "done",
        }
    }

    fn index(self) -> usize {
        STEPS.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn next(self) -> Option<Step> {
        STEPS.get(self.index() + 1).copied()
    }

    pub fn prev(self) -> Option<Step> {
        self.index().checked_sub(1).map(|i| STEPS[i])
    }
}

pub struct Language {
    pub name: &'static str,
    pub region: &'static str,
    pub locale: &'static str,
    pub keymap: &'static str,
}

const fn lang(
    name: &'static str,
    region: &'static str,
    locale: &'static str,
    keymap: &'static str,
) -> Language {
    Language {
        name,
        region,
        locale,
        keymap,
    }
}

pub const LANGUAGES: [Language; 8] = [
    lang("English", "United States", "en_US.UTF-8", "us"),
    lang("English", "United Kingdom", "en_GB.UTF-8", "gb"),
    lang("Português", "Brasil", "pt_BR.UTF-8", "br"),
    lang("Português", "Portugal", "pt_PT.UTF-8", "pt"),
    lang("Español", "España", "es_ES.UTF-8", "es"),
    lang("Français", "France", "fr_FR.UTF-8", "fr"),
    lang("Deutsch", "Deutschland", "de_DE.UTF-8", "de"),
    lang("Italiano", "Italia", "it_IT.UTF-8", "it"),
];

pub struct Keyboard {
    pub name: &'static str,
    /// xkb layout
    pub layout: &'static str,
}

pub const KEYBOARDS: [Keyboard; 8] = [
    Keyboard {
        name: "U.S.",
        layout: "us",
    },
    Keyboard {
        name: "British",
        layout: "gb",
    },
    Keyboard {
        name: "Brazilian (ABNT2)",
        layout: "br",
    },
    Keyboard {
        name: "Portuguese",
        layout: "pt",
    },
    Keyboard {
        name: "Spanish",
        layout: "es",
    },
    Keyboard {
        name: "French (AZERTY)",
        layout: "fr",
    },
    Keyboard {
        name: "German (QWERTZ)",
        layout: "de",
    },
    Keyboard {
        name: "Italian",
        layout: "it",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    Auto,
}

impl Appearance {
    pub fn wire(self) -> &'static str {
        match self {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
            Appearance::Auto => "auto",
        }
    }
}

/// Account name macOS-style: lowercase ASCII letters and digits of the full name, accents folded,
/// starting with a letter, at most 32.
pub fn account_name(full: &str) -> String {
    let fold = |c: char| -> Option<char> {
        let c = c.to_lowercase().next()?;
        let base = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            c => c,
        };
        (base.is_ascii_lowercase() || base.is_ascii_digit()).then_some(base)
    };
    full.chars()
        .filter_map(fold)
        .skip_while(|c| c.is_ascii_digit())
        .take(32)
        .collect()
}

/// Everything the assistant collects.
#[derive(Clone, Debug, Default)]
pub struct Draft {
    pub language: usize,
    pub keyboard: usize,
    pub full_name: String,
    pub name: String,
    name_edited: bool,
    pub password: String,
    pub verify: String,
    pub hint: String,
    pub appearance: Appearance,
}

impl Draft {
    /// Picking a language also picks its keyboard.
    pub fn set_language(&mut self, i: usize) {
        self.language = i.min(LANGUAGES.len() - 1);
        let km = LANGUAGES[self.language].keymap;
        self.keyboard = KEYBOARDS.iter().position(|k| k.layout == km).unwrap_or(0);
    }

    /// Derives the account name until the user edits it.
    pub fn set_full_name(&mut self, full: &str) {
        self.full_name = full.to_string();
        if !self.name_edited {
            self.name = account_name(full);
        }
    }

    pub fn set_name(&mut self, name: &str) {
        self.name_edited = name != account_name(&self.full_name);
        self.name = name.to_string();
    }

    /// First thing to fix on the account card (one sentence), or None to continue.
    pub fn account_problem(&self) -> Option<&'static str> {
        if self.full_name.trim().is_empty() {
            return Some("Enter your full name.");
        }
        if self.password.is_empty() {
            return Some("Choose a password.");
        }
        if self.password != self.verify {
            return Some("Passwords don't match.");
        }
        if !self.hint.is_empty() && self.hint == self.password {
            return Some("The hint can't be the password.");
        }
        match wol_core::settings::users::validate_account(&self.account()) {
            Ok(()) => None,
            Err("name") => Some("Use lowercase letters and digits for the account name."),
            Err(_) => Some("Check the highlighted fields."),
        }
    }

    pub fn account(&self) -> NewAccount {
        NewAccount {
            name: self.name.clone(),
            full_name: self.full_name.trim().to_string(),
            hint: self.hint.clone(),
            locale: LANGUAGES[self.language].locale.into(),
            keymap: KEYBOARDS[self.keyboard].layout.into(),
            appearance: self.appearance.wire().into(),
            password: self.password.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_name_from_full_name() {
        for (full, want) in [
            ("Alice Liddell", "aliceliddell"),
            ("  José  Álvarez-Núñez ", "josealvareznunez"),
            ("Ünïcödé", "unicode"),
            ("42 Wonderland", "wonderland"),
            ("", ""),
            ("李", ""),
        ] {
            assert_eq!(account_name(full), want, "{full}");
        }
        assert_eq!(account_name(&"a".repeat(40)).len(), 32);
    }

    #[test]
    fn steps_in_order() {
        assert_eq!(
            STEPS.map(Step::title),
            [
                "Welcome",
                "Language & Region",
                "Keyboard",
                "Wi-Fi",
                "Create Your Account",
                "Appearance",
                "You're all set",
            ]
        );
        assert_eq!(Step::Welcome.next(), Some(Step::Language));
        assert_eq!(Step::Done.next(), None);
        assert_eq!(Step::Welcome.prev(), None);
        assert_eq!(Step::Keyboard.prev(), Some(Step::Language));
    }

    #[test]
    fn language_picks_keyboard() {
        let pt = LANGUAGES
            .iter()
            .position(|l| l.locale == "pt_BR.UTF-8")
            .unwrap();
        let mut d = Draft::default();
        d.set_language(pt);
        assert_eq!(KEYBOARDS[d.keyboard].layout, "br");
        assert_eq!(LANGUAGES[0].locale, "en_US.UTF-8");
        assert!(
            LANGUAGES
                .iter()
                .all(|l| KEYBOARDS.iter().any(|k| k.layout == l.keymap))
        );
    }

    #[test]
    fn account_draft() {
        let mut d = Draft::default();
        assert_eq!(d.account_problem(), Some("Enter your full name."));
        d.set_full_name("Alice Liddell");
        assert_eq!(d.name, "aliceliddell");
        d.set_name("alice");
        d.set_full_name("Alice P. Liddell");
        assert_eq!(d.name, "alice", "edited account name stays");
        assert_eq!(d.account_problem(), Some("Choose a password."));
        d.password = "curiouser".into();
        d.verify = "curiouserr".into();
        assert_eq!(d.account_problem(), Some("Passwords don't match."));
        d.verify = "curiouser".into();
        d.hint = "curiouser".into();
        assert_eq!(d.account_problem(), Some("The hint can't be the password."));
        d.hint = "white rabbit".into();
        assert_eq!(d.account_problem(), None);
        d.appearance = Appearance::Auto;
        let a = d.account();
        assert_eq!(
            (
                a.name.as_str(),
                a.full_name.as_str(),
                a.locale.as_str(),
                a.keymap.as_str(),
                a.appearance.as_str()
            ),
            ("alice", "Alice P. Liddell", "en_US.UTF-8", "us", "auto")
        );
        assert_eq!(wol_core::settings::users::validate_account(&a), Ok(()));
    }
}
