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

pub const LANGUAGES: [Language; 7] = [
    lang("English", "United States", "en_US.UTF-8", "us"),
    lang("English", "United Kingdom", "en_GB.UTF-8", "gb"),
    lang("Português", "Brasil", "pt_BR.UTF-8", "br"),
    lang("Português", "Portugal", "pt_PT.UTF-8", "pt"),
    lang("Español", "España", "es_ES.UTF-8", "es"),
    lang("Français", "France", "fr_FR.UTF-8", "fr"),
    lang("Deutsch", "Deutschland", "de_DE.UTF-8", "de"),
];

/// (xkb layout, name): the same list Settings > Keyboard offers.
pub const KEYBOARDS: [(&str, &str); 8] = wol_core::prefs::LAYOUTS;

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

/// Account card fields (problems are shown under the one they concern).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    FullName,
    Name,
    Password,
    Verify,
    Hint,
}

/// Avatar text: initials, "?" before any input.
pub fn initials_or_placeholder(full: &str) -> String {
    let i = crate::login::initials(full);
    if i.is_empty() { "?".into() } else { i }
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
        self.keyboard = KEYBOARDS.iter().position(|k| k.0 == km).unwrap_or(0);
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

    /// First thing to fix on the account card: the field and one sentence; None to continue.
    pub fn account_problem(&self) -> Option<(Field, &'static str)> {
        if self.full_name.trim().is_empty() {
            return Some((Field::FullName, "Enter your full name."));
        }
        let rules = wol_core::settings::users::validate_account(&self.account());
        if rules == Err("name") || self.name.is_empty() {
            return Some((
                Field::Name,
                "Use lowercase letters and digits, starting with a letter.",
            ));
        }
        if self.password.is_empty() {
            return Some((Field::Password, "Choose a password."));
        }
        if self.password != self.verify {
            return Some((Field::Verify, "Passwords don't match."));
        }
        if !self.hint.is_empty() && self.hint == self.password {
            return Some((Field::Hint, "The hint can't be the password."));
        }
        match rules {
            Ok(()) => None,
            Err("hint") => Some((Field::Hint, "Keep the hint short, without colons.")),
            Err("full name") => Some((Field::FullName, "Remove colons from the name.")),
            Err(_) => Some((Field::Password, "Choose a different password.")),
        }
    }

    pub fn account(&self) -> NewAccount {
        NewAccount {
            name: self.name.clone(),
            full_name: self.full_name.trim().to_string(),
            hint: self.hint.clone(),
            locale: LANGUAGES[self.language].locale.into(),
            keymap: KEYBOARDS[self.keyboard].0.into(),
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
        assert_eq!(KEYBOARDS[d.keyboard].0, "br");
        assert_eq!(LANGUAGES[0].locale, "en_US.UTF-8");
        assert!(
            LANGUAGES
                .iter()
                .all(|l| KEYBOARDS.iter().any(|k| k.0 == l.keymap))
        );
    }

    #[test]
    fn account_draft() {
        use Field::*;
        let mut d = Draft::default();
        assert_eq!(
            d.account_problem(),
            Some((FullName, "Enter your full name."))
        );
        d.set_full_name("Alice Liddell");
        assert_eq!(d.name, "aliceliddell");
        d.set_name("alice");
        d.set_full_name("Alice P. Liddell");
        assert_eq!(d.name, "alice", "edited account name stays");
        d.set_name("Alice!");
        assert_eq!(d.account_problem().map(|p| p.0), Some(Name));
        d.set_name("alice");
        assert_eq!(d.account_problem(), Some((Password, "Choose a password.")));
        d.password = "curiouser".into();
        d.verify = "curiouserr".into();
        assert_eq!(
            d.account_problem(),
            Some((Verify, "Passwords don't match."))
        );
        d.verify = "curiouser".into();
        d.hint = "curiouser".into();
        assert_eq!(
            d.account_problem(),
            Some((Hint, "The hint can't be the password."))
        );
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
        assert_eq!(initials_or_placeholder(""), "?");
        assert_eq!(initials_or_placeholder("alice liddell"), "AL");
    }
}
