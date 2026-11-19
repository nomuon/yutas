//! Mirrors what the IME is doing with the user's romaji keystrokes, so that we
//! know how many characters the uncommitted composition holds (and which kana).
//! The IME stays the source of truth: the engine re-reads the committed text
//! from the document and only uses this mirror to find and verify it.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Stands in for a character we count but do not predict (digits, symbols,
/// literal letters): its width/shape depends on IME settings.
pub const SYM: char = '\u{FFFC}';

struct Table {
    map: HashMap<String, &'static str>,
    prefixes: HashSet<String>,
}

static TABLE: LazyLock<Table> = LazyLock::new(build_table);

fn build_table() -> Table {
    const VOWELS: [char; 5] = ['a', 'i', 'u', 'e', 'o'];
    // consonant prefix -> kana for a, i, u, e, o
    const ROWS: &[(&str, [&str; 5])] = &[
        ("", ["あ", "い", "う", "え", "お"]),
        ("k", ["か", "き", "く", "け", "こ"]),
        ("s", ["さ", "し", "す", "せ", "そ"]),
        ("t", ["た", "ち", "つ", "て", "と"]),
        ("n", ["な", "に", "ぬ", "ね", "の"]),
        ("h", ["は", "ひ", "ふ", "へ", "ほ"]),
        ("m", ["ま", "み", "む", "め", "も"]),
        ("y", ["や", "い", "ゆ", "いぇ", "よ"]),
        ("r", ["ら", "り", "る", "れ", "ろ"]),
        ("w", ["わ", "うぃ", "う", "うぇ", "を"]),
        ("g", ["が", "ぎ", "ぐ", "げ", "ご"]),
        ("z", ["ざ", "じ", "ず", "ぜ", "ぞ"]),
        ("d", ["だ", "ぢ", "づ", "で", "ど"]),
        ("b", ["ば", "び", "ぶ", "べ", "ぼ"]),
        ("p", ["ぱ", "ぴ", "ぷ", "ぺ", "ぽ"]),
        ("c", ["か", "し", "く", "せ", "こ"]),
        ("f", ["ふぁ", "ふぃ", "ふ", "ふぇ", "ふぉ"]),
        ("j", ["じゃ", "じ", "じゅ", "じぇ", "じょ"]),
        ("v", ["ゔぁ", "ゔぃ", "ゔ", "ゔぇ", "ゔぉ"]),
        ("q", ["くぁ", "くぃ", "く", "くぇ", "くぉ"]),
        ("x", ["ぁ", "ぃ", "ぅ", "ぇ", "ぉ"]),
        ("l", ["ぁ", "ぃ", "ぅ", "ぇ", "ぉ"]),
        ("ky", ["きゃ", "きぃ", "きゅ", "きぇ", "きょ"]),
        ("sy", ["しゃ", "しぃ", "しゅ", "しぇ", "しょ"]),
        ("sh", ["しゃ", "し", "しゅ", "しぇ", "しょ"]),
        ("ty", ["ちゃ", "ちぃ", "ちゅ", "ちぇ", "ちょ"]),
        ("ch", ["ちゃ", "ち", "ちゅ", "ちぇ", "ちょ"]),
        ("cy", ["ちゃ", "ちぃ", "ちゅ", "ちぇ", "ちょ"]),
        ("ny", ["にゃ", "にぃ", "にゅ", "にぇ", "にょ"]),
        ("hy", ["ひゃ", "ひぃ", "ひゅ", "ひぇ", "ひょ"]),
        ("my", ["みゃ", "みぃ", "みゅ", "みぇ", "みょ"]),
        ("ry", ["りゃ", "りぃ", "りゅ", "りぇ", "りょ"]),
        ("gy", ["ぎゃ", "ぎぃ", "ぎゅ", "ぎぇ", "ぎょ"]),
        ("zy", ["じゃ", "じぃ", "じゅ", "じぇ", "じょ"]),
        ("jy", ["じゃ", "じぃ", "じゅ", "じぇ", "じょ"]),
        ("dy", ["ぢゃ", "ぢぃ", "ぢゅ", "ぢぇ", "ぢょ"]),
        ("by", ["びゃ", "びぃ", "びゅ", "びぇ", "びょ"]),
        ("py", ["ぴゃ", "ぴぃ", "ぴゅ", "ぴぇ", "ぴょ"]),
        ("fy", ["ふゃ", "ふぃ", "ふゅ", "ふぇ", "ふょ"]),
        ("ts", ["つぁ", "つぃ", "つ", "つぇ", "つぉ"]),
        ("th", ["てゃ", "てぃ", "てゅ", "てぇ", "てょ"]),
        ("dh", ["でゃ", "でぃ", "でゅ", "でぇ", "でょ"]),
        ("tw", ["とぁ", "とぃ", "とぅ", "とぇ", "とぉ"]),
        ("dw", ["どぁ", "どぃ", "どぅ", "どぇ", "どぉ"]),
        ("wh", ["うぁ", "うぃ", "う", "うぇ", "うぉ"]),
        ("kw", ["くぁ", "くぃ", "くぅ", "くぇ", "くぉ"]),
        ("gw", ["ぐぁ", "ぐぃ", "ぐぅ", "ぐぇ", "ぐぉ"]),
        ("xy", ["ゃ", "ぃ", "ゅ", "ぇ", "ょ"]),
        ("ly", ["ゃ", "ぃ", "ゅ", "ぇ", "ょ"]),
    ];
    const EXTRA: &[(&str, &str)] = &[
        ("nn", "ん"),
        ("xn", "ん"),
        ("xtu", "っ"),
        ("ltu", "っ"),
        ("xtsu", "っ"),
        ("ltsu", "っ"),
        ("xwa", "ゎ"),
        ("lwa", "ゎ"),
        ("xka", "ヵ"),
        ("lka", "ヵ"),
        ("xke", "ヶ"),
        ("lke", "ヶ"),
    ];

    let mut map = HashMap::new();
    for (prefix, kana) in ROWS {
        for (v, k) in VOWELS.iter().zip(kana.iter()) {
            map.insert(format!("{prefix}{v}"), *k);
        }
    }
    for (k, v) in EXTRA {
        map.insert((*k).to_string(), *v);
    }
    let mut prefixes = HashSet::new();
    for key in map.keys() {
        for end in 1..key.len() {
            prefixes.insert(key[..end].to_string());
        }
    }
    Table { map, prefixes }
}

#[derive(Default)]
pub struct Composer {
    out: Vec<char>,
    pending: String,
    /// Shift+letter switches the IME to literal alphanumerics until commit.
    alpha: bool,
}

impl Composer {
    pub fn is_empty(&self) -> bool {
        self.out.is_empty() && self.pending.is_empty()
    }

    pub fn clear(&mut self) {
        self.out.clear();
        self.pending.clear();
        self.alpha = false;
    }

    pub fn push_letter(&mut self, c: char, shifted: bool) {
        if shifted {
            self.alpha = true;
        }
        if self.alpha {
            self.flush_pending();
            self.out.push(SYM);
            return;
        }
        self.pending.push(c);
        self.resolve();
    }

    pub fn push_symbol(&mut self) {
        self.flush_pending();
        self.out.push(SYM);
    }

    pub fn backspace(&mut self) {
        if self.pending.pop().is_none() {
            self.out.pop();
        }
        if self.is_empty() {
            self.alpha = false;
        }
    }

    /// The composition as the IME would commit it right now.
    pub fn finalized(&self) -> Vec<char> {
        let mut out = self.out.clone();
        out.extend(self.pending.chars().map(leftover));
        out
    }

    fn flush_pending(&mut self) {
        let rest: Vec<char> = self.pending.chars().map(leftover).collect();
        self.out.extend(rest);
        self.pending.clear();
    }

    fn resolve(&mut self) {
        while !self.pending.is_empty() {
            if let Some(kana) = TABLE.map.get(&self.pending) {
                self.out.extend(kana.chars());
                self.pending.clear();
                return;
            }
            if TABLE.prefixes.contains(&self.pending) {
                return;
            }
            // Not a romaji sequence: the first letter resolves on its own.
            let mut chars = self.pending.chars();
            let first = chars.next().unwrap();
            let second = chars.clone().next();
            let rest: String = chars.collect();
            self.out.push(if first == 'n' {
                'ん'
            } else if second == Some(first) {
                'っ'
            } else {
                SYM
            });
            self.pending = rest;
        }
    }
}

/// A letter still waiting for a vowel when the composition is committed.
fn leftover(c: char) -> char {
    if c == 'n' {
        'ん'
    } else {
        SYM
    }
}

fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{3096}').contains(&c)
}

/// Does the text read back from the document match what we tracked?
/// Length must agree, and every kana we predicted must be that kana.
pub fn matches(expected: &[char], actual: &str) -> bool {
    let actual: Vec<char> = actual.chars().collect();
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual.iter())
            .all(|(e, a)| !is_hiragana(*e) || e == a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kana(input: &str) -> String {
        let mut c = Composer::default();
        for ch in input.chars() {
            if ch.is_ascii_lowercase() {
                c.push_letter(ch, false);
            } else {
                c.push_symbol();
            }
        }
        c.finalized().into_iter().collect()
    }

    #[test]
    fn basic_words() {
        assert_eq!(kana("ashita"), "あした");
        assert_eq!(kana("kyouha"), "きょうは");
        assert_eq!(kana("gakkou"), "がっこう");
        assert_eq!(kana("konnnitiha"), "こんにちは");
        assert_eq!(kana("kantan"), "かんたん");
        assert_eq!(kana("henkou"), "へんこう");
        assert_eq!(kana("shimasu"), "します");
        assert_eq!(kana("tyotto"), "ちょっと");
        assert_eq!(kana("fairu"), "ふぁいる");
    }

    #[test]
    fn symbols_count_as_one() {
        assert_eq!(kana("15ji"), format!("{SYM}{SYM}じ"));
        assert_eq!(kana("un,"), format!("うん{SYM}"));
    }

    #[test]
    fn backspace_removes_pending_first() {
        let mut c = Composer::default();
        for ch in "kak".chars() {
            c.push_letter(ch, false);
        }
        c.backspace();
        assert_eq!(c.finalized(), vec!['か']);
        c.backspace();
        assert!(c.is_empty());
    }

    #[test]
    fn match_ignores_symbol_shape() {
        let expected: Vec<char> = kana("15jini").chars().collect();
        assert!(matches(&expected, "１５じに"));
        assert!(matches(&expected, "15じに"));
        assert!(!matches(&expected, "15時に"));
        assert!(!matches(&expected, "5じに"));
    }
}
