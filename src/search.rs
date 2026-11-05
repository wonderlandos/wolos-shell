// SPDX-License-Identifier: GPL-2.0-or-later
//! Search ranking (port from IaraOS). Titles are lowercased once at index time.

/// Shortest query that may match as a subsequence (`stg` finds Settings; `ls` finds nothing by it).
pub const SUBSEQUENCE_FROM: usize = 3;

/// 4 prefix, 3 word start, 2 substring, 1 subsequence, 0 none. Both inputs already lowercase.
pub fn rank(title: &str, q: &str) -> u8 {
    if q.is_empty() {
        return 0;
    }
    if title.starts_with(q) {
        return 4;
    }
    if title.split([' ', '-', '.', '_']).any(|w| w.starts_with(q)) {
        return 3;
    }
    if title.contains(q) {
        return 2;
    }
    // A subsequence match is for a typed-out abbreviation: two letters match half the index
    // ("ls" found Hardware Locality, Modules, Displays, Lock Screen, ...; QA-LN-5).
    if q.chars().count() < SUBSEQUENCE_FROM {
        return 0;
    }
    let mut qc = q.chars().peekable();
    for c in title.chars() {
        if qc.peek() == Some(&c) {
            qc.next();
        }
    }
    if qc.peek().is_none() { 1 } else { 0 }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub title: String,
    pub lower: String,
    pub kind: &'static str,
    pub icon: String,
    pub exec: Vec<String>,
    /// Lowercase search words, each preceded by a space (`" theme dark"`); see [`Item::with_words`].
    pub words: String,
    /// The app's desktop id without `.desktop` for an installed app, "" for anything else: what
    /// the row hands the Dock (drag it there, or Keep in Dock; `apps::keep_in_dock`).
    pub app: String,
    /// Ranks taken off a hit: a Settings row (a thing on a page) ranks below an app, an action
    /// or the page itself that match the same way ([`Item::demoted`]).
    pub demote: u8,
}

impl Item {
    pub fn new(title: &str, kind: &'static str, icon: &str, exec: Vec<String>) -> Self {
        Item {
            title: title.into(),
            lower: title.to_lowercase(),
            kind,
            icon: icon.into(),
            exec,
            words: String::new(),
            app: String::new(),
            demote: 0,
        }
    }

    /// A hit on this item ranks one lower than the same hit on a first-class item: 'screenshot'
    /// finds the Take Screenshot action before the Screenshot row of Control Center's page.
    pub fn demoted(mut self) -> Self {
        self.demote = 1;
        self
    }

    /// An installed app's row: the Dock can keep it by `id` ([`Item::app`]).
    pub fn with_app(mut self, id: &str) -> Self {
        self.app = id.into();
        self
    }

    /// Also found when a query of [`WORDS_FROM`]+ letters starts one of `words` (rank 2). A word with a
    /// hyphen is also found without it: `Wi-Fi` answers `wifi` too, so the words keep the glossary spelling.
    pub fn with_words(mut self, words: &str) -> Self {
        let lower = words.to_lowercase();
        self.words = format!(" {lower}");
        for w in lower.split_whitespace().filter(|w| w.contains('-')) {
            self.words.push(' ');
            self.words.push_str(&w.replace('-', ""));
        }
        self
    }

    fn rank(&self, q: &str) -> u8 {
        match self.rank_hit(q) {
            0 => 0,
            r => r.saturating_sub(self.demote).max(1),
        }
    }

    fn rank_hit(&self, q: &str) -> u8 {
        let r = self.rank_one(q);
        if r >= 2 {
            return r;
        }
        // Several words, in any order ("dark mode", "mode dark", "time zone"): every word must
        // match on its own, as a prefix, a word start or a substring, of the title or the search
        // words (QA-LN-4: only the whole text was tried, as one prefix).
        let words: Vec<&str> = q.split_whitespace().collect();
        if words.len() > 1 {
            // "privacy settings": on a Settings result "settings" only names the place (QA-SEARCH-1).
            let settings = self.kind == crate::ui::searchbar::SETTINGS;
            let mut real = words
                .iter()
                .filter(|w| !(settings && is_settings_filler(w)));
            if real.clone().next().is_some() && real.all(|w| self.rank_one(w) >= 2) {
                return 2;
            }
        }
        r
    }

    fn rank_one(&self, q: &str) -> u8 {
        let r = rank(&self.lower, q);
        if r < 2 && q.len() >= WORDS_FROM && self.words.contains(&format!(" {q}")) {
            return 2;
        }
        r
    }
}

/// Words a person adds to say where to look, not what to find: "privacy settings", "ajustes de
/// som". On a Settings result they match nothing and cost nothing (when another word remains).
pub const SETTINGS_FILLER: [&str; 12] = [
    "settings",
    "setting",
    "preferences",
    "preference",
    "prefs",
    "options",
    "ajustes",
    "ajuste",
    "configurações",
    "configuracoes",
    "preferências",
    "preferencias",
];

/// Whether `w` (lowercase) is one of [`SETTINGS_FILLER`].
pub fn is_settings_filler(w: &str) -> bool {
    SETTINGS_FILLER.contains(&w)
}

/// Shortest query matched against search words (shorter ones match half of Settings).
pub const WORDS_FROM: usize = 3;

/// Best `max` hits: rank desc, then shorter title, then title.
pub fn search<'a>(items: &'a [Item], query: &str, max: usize) -> Vec<&'a Item> {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(u8, &Item)> = items
        .iter()
        .map(|i| (i.rank(&q), i))
        .filter(|h| h.0 > 0)
        .collect();
    hits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.title.len().cmp(&b.1.title.len()))
            .then(a.1.title.cmp(&b.1.title))
    });
    hits.into_iter().take(max).map(|h| h.1).collect()
}

/// Search's calculator: `2+2`, `(3.5 * 4) / 2`, `2^10`, `15%`. None unless the query is
/// arithmetic with at least one operator. The answer is shown rounded to 10 significant places.
pub fn calc(query: &str) -> Option<String> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty()
        || q.len() > 200
        || !q.iter().any(|c| "+-*/^%x×÷".contains(*c))
        || !q
            .iter()
            .all(|c| c.is_ascii_digit() || "+-*/^%().,x×÷".contains(*c))
    {
        return None;
    }
    let mut p = Calc { s: &q, i: 0 };
    let v = p.sum()?;
    if p.i != q.len() || !v.is_finite() {
        return None;
    }
    let r = (v * 1e10).round() / 1e10;
    let text = if r.fract() == 0.0 && r.abs() < 1e15 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    };
    // "5" for "5": a bare number is not a calculation.
    (text != q.iter().collect::<String>()).then_some(text)
}

struct Calc<'a> {
    s: &'a [char],
    i: usize,
}

impl Calc<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(c @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            v = if c == '+' { v + r } else { v - r };
        }
        Some(v)
    }
    fn product(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(c @ ('*' | '/' | 'x' | '×' | '÷')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = if matches!(c, '/' | '÷') {
                if r == 0.0 {
                    return None;
                }
                v / r
            } else {
                v * r
            };
        }
        Some(v)
    }
    fn power(&mut self) -> Option<f64> {
        let b = self.unary()?;
        if self.peek() == Some('^') {
            self.i += 1;
            return Some(b.powf(self.power()?));
        }
        Some(b)
    }
    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            '-' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            '+' => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Option<f64> {
        let mut v = if self.peek()? == '(' {
            self.i += 1;
            let v = self.sum()?;
            (self.peek()? == ')').then_some(())?;
            self.i += 1;
            v
        } else {
            let start = self.i;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',')
            {
                self.i += 1;
            }
            let n: String = self.s[start..self.i]
                .iter()
                .map(|c| if *c == ',' { '.' } else { *c })
                .collect();
            n.parse().ok()?
        };
        if self.peek() == Some('%') {
            self.i += 1;
            v /= 100.0;
        }
        Some(v)
    }
}

/// Slower providers (the file index) answer after the local ones; rows from an answer that
/// came in after `timeout_ms` are left out so the list does not shift under the pointer.
/// `slow` holds (milliseconds after the keystroke it arrived, rows). Local rows stay first;
/// a slow row duplicating one already listed (same title and kind) is dropped.
pub fn merge_slow(
    local: Vec<Item>,
    slow: Vec<(u32, Vec<Item>)>,
    timeout_ms: u32,
    rows: usize,
) -> Vec<Item> {
    let mut out = local;
    out.truncate(rows);
    let mut late: Vec<_> = slow.into_iter().filter(|(t, _)| *t <= timeout_ms).collect();
    late.sort_by_key(|(t, _)| *t);
    for (_, items) in late {
        for it in items {
            if out.len() >= rows {
                return out;
            }
            if !out.iter().any(|o| o.title == it.title && o.kind == it.kind) {
                out.push(it);
            }
        }
    }
    out
}

/// May a slow answer that arrived `elapsed_ms` after the keystroke still be shown?
pub fn slow_in_time(elapsed_ms: u32, timeout_ms: u32) -> bool {
    elapsed_ms <= timeout_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(t: &str) -> Item {
        Item::new(t, "File", "x", vec![])
    }

    #[test]
    fn slow_providers_merge_only_before_the_timeout() {
        let local = vec![it("a"), it("b")];
        let slow = vec![(500, vec![it("late")]), (100, vec![it("c"), it("a")])];
        let got: Vec<_> = merge_slow(local.clone(), slow, 400, 4)
            .into_iter()
            .map(|i| i.title)
            .collect();
        assert_eq!(
            got,
            ["a", "b", "c"],
            "local first, duplicates and late rows out"
        );
        let capped = merge_slow(local, vec![(1, vec![it("c"), it("d")])], 400, 3);
        assert_eq!(capped.len(), 3);
        assert!(slow_in_time(400, 400) && !slow_in_time(401, 400));
        assert!(merge_slow(vec![], vec![], 400, 4).is_empty());
    }

    #[test]
    fn calculator_answers_arithmetic_only() {
        for (q, want) in [
            ("2+2", Some("4")),
            ("(3.5 * 4) / 2", Some("7")),
            ("2^10", Some("1024")),
            ("1/3", Some("0.3333333333")),
            ("15% * 200", Some("30")),
            ("-3 + 1", Some("-2")),
            ("1,5*2", Some("3")),
            ("10 x 3", Some("30")),
            ("1/0", None),
            ("42", None),
            ("terminal", None),
            ("2+", None),
            ("(2+3", None),
            ("", None),
        ] {
            assert_eq!(calc(q).as_deref(), want, "{q}");
        }
    }

    /// QA-LN-4: 'dark mode', 'mode dark' and 'time zone' found nothing while 'dark' found Appearance.
    #[test]
    fn multiword_any_order() {
        let items = [
            Item::new("Appearance", "", "", vec![]).with_words("theme dark light mode"),
            Item::new("Date & Time", "", "", vec![]).with_words("clock timezone zone"),
            Item::new("Network", "", "", vec![]),
        ];
        let first = |q| search(&items, q, 1).first().map(|i| i.title.as_str());
        assert_eq!(first("dark mode"), Some("Appearance"));
        assert_eq!(first("mode dark"), Some("Appearance"));
        assert_eq!(first("time zone"), Some("Date & Time"));
        assert_eq!(
            first("dark zone"),
            None,
            "every word has to match the same item"
        );
        assert_eq!(first("dark xyz"), None);
    }

    /// QA-LN-5: a one- or two-letter query is a prefix, a word start or a substring, never a
    /// subsequence ('ls' listed lstopo, Modules, Displays, Lock Screen, ...); three letters still
    /// find 'Settings' by 'stg'.
    #[test]
    fn short_query_no_subsequence() {
        assert_eq!(
            rank("hardware locality lstopo", "ls"),
            3,
            "a word start is still a hit"
        );
        assert_eq!(rank("modules", "ls"), 0);
        assert_eq!(rank("lock screen", "ls"), 0);
        assert_eq!(rank("displays", "ds"), 0);
        assert_eq!(rank("settings", "stg"), 1);
        let items = [
            "Modules",
            "Displays",
            "Lock Screen",
            "Login Items",
            "Effects & Looks",
            "Users & Groups",
        ]
        .map(|t| Item::new(t, "", "", vec![]));
        assert!(search(&items, "ls", 8).is_empty());
        assert!(
            !search(&items, "sgs", 8).is_empty(),
            "three letters keep the abbreviation"
        );
    }

    #[test]
    fn ranks() {
        for (t, q, w) in [
            ("terminal", "ter", 4),
            ("gnome terminal", "ter", 3),
            ("my-files", "fil", 3),
            ("bitter", "tte", 2),
            ("settings", "stg", 1),
            ("files", "xyz", 0),
            ("files", "", 0),
        ] {
            assert_eq!(rank(t, q), w, "{t} {q}");
        }
    }

    #[test]
    fn orders_by_rank_then_length() {
        let items: Vec<Item> = ["Text Editor", "Terminal", "Settings", "Tetris", "Files"]
            .iter()
            .map(|t| Item::new(t, "app", "", vec![]))
            .collect();
        let got: Vec<&str> = search(&items, "Te", 10)
            .iter()
            .map(|i| i.title.as_str())
            .collect();
        assert_eq!(got, ["Tetris", "Terminal", "Text Editor"]);
        assert_eq!(search(&items, "te", 1).len(), 1);
        assert!(search(&items, "  ", 10).is_empty());
    }

    /// A Settings pane is found by its search words too ("dark" -> Appearance), from three
    /// letters on and from a word start, below any title match.
    #[test]
    fn search_words_find_an_item_below_titles() {
        let items = [
            Item::new("Appearance", "", "", vec![]).with_words("theme dark light"),
            Item::new("Desktop & Dock", "", "", vec![]).with_words("dock menu bar"),
            Item::new("Darkroom", "", "", vec![]),
        ];
        let first = |q| search(&items, q, 1).first().map(|i| i.title.as_str());
        assert_eq!(first("dark"), Some("Darkroom"));
        assert_eq!(search(&items, "dark", 5).len(), 2);
        assert_eq!(first("theme"), Some("Appearance"));
        assert_eq!(first("Menu Bar"), Some("Desktop & Dock"));
        assert_eq!(first("enu"), None, "word starts only");
        assert_eq!(first("th"), None, "too short for words");
    }

    /// A word spelled with a hyphen in the glossary (`Wi-Fi`) is found with and without it.
    #[test]
    fn a_hyphenated_search_word_is_found_without_its_hyphen() {
        let items = [Item::new("Network", "", "", vec![]).with_words("Wi-Fi wireless")];
        let first = |q| search(&items, q, 1).first().map(|i| i.title.as_str());
        assert_eq!(first("wifi"), Some("Network"));
        assert_eq!(first("wi-fi"), Some("Network"));
        assert_eq!(first("wireless"), Some("Network"));
        assert_eq!(first("ifi"), None, "word starts only");
    }
}
