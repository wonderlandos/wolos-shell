//! Spotlight ranking (port of Iara.Core.Search.Spotlight.Rank). Titles are lowercased once at index time.

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
        }
    }

    /// Also found when a query of [`WORDS_FROM`]+ letters starts one of `words` (rank 2).
    pub fn with_words(mut self, words: &str) -> Self {
        self.words = format!(" {}", words.to_lowercase());
        self
    }

    fn rank(&self, q: &str) -> u8 {
        let r = rank(&self.lower, q);
        if r < 2 && q.len() >= WORDS_FROM && self.words.contains(&format!(" {q}")) {
            return 2;
        }
        r
    }
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

/// Spotlight's calculator: `2+2`, `(3.5 * 4) / 2`, `2^10`, `15%`. None unless the query is
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
