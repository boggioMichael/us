//! Numbers read on screen: "HP 1240/1500", "EXP 45.20%", "TIME 0:07",
//! "Lv. 230", "SCORE 2 - 1", "29 min 32 sec", "GOLD 150", "x3".
//!
//! Values are read as text first (Maplesyrup's lesson: a bar's fill is only
//! corroboration), so the parser is careful: digits with separators, OCR's
//! usual confusions (O for 0, l for 1) inside numbers, and a label taken
//! from the words just before the number.

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// `87/100`.
    Ratio { value: f64, max: f64 },
    /// `45.20%`.
    Percent(f64),
    /// A time in seconds: `0:07`, `1:02:03`, `29 min 32 sec`.
    Time(f64),
    /// `Lv. 230`.
    Level(f64),
    /// `2 - 1` (a score between two sides).
    Pair(f64, f64),
    /// A plain number.
    Count(f64),
}

impl Reading {
    /// The main number.
    pub fn value(&self) -> f64 {
        match self {
            Reading::Ratio { value, .. } => *value,
            Reading::Percent(p) => *p,
            Reading::Time(s) => *s,
            Reading::Level(l) => *l,
            Reading::Pair(a, _) => *a,
            Reading::Count(c) => *c,
        }
    }

    /// As a fraction of its maximum, when there is one.
    pub fn fraction(&self) -> Option<f64> {
        match self {
            Reading::Ratio { value, max } if *max > 0.0 => Some((value / max).clamp(0.0, 1.5)),
            Reading::Percent(p) => Some(p / 100.0),
            _ => None,
        }
    }

    pub fn max(&self) -> Option<f64> {
        match self {
            Reading::Ratio { max, .. } => Some(*max),
            Reading::Percent(_) => Some(100.0),
            _ => None,
        }
    }
}

/// A reading and the words before it (lower case), if any.
#[derive(Debug, Clone, PartialEq)]
pub struct Labelled {
    pub label: Option<String>,
    pub reading: Reading,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// OCR confusions inside numbers: `1O0` is `100`, `l2` is `12`.
fn fix_digits(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let digit = |j: Option<usize>| j.and_then(|j| chars.get(j)).is_some_and(|c| c.is_ascii_digit());
    let letter = |j: Option<usize>| j.and_then(|j| chars.get(j)).is_some_and(|c| c.is_alphabetic());
    for (i, c) in chars.iter().enumerate() {
        let (before, after) = (i.checked_sub(1), Some(i + 1));
        let inside_number = (digit(before) && (digit(after) || !letter(after))) || (digit(after) && !letter(before));
        out.push(match c {
            'O' | 'o' | 'D' if inside_number => '0',
            'l' | 'I' | '|' if inside_number => '1',
            'S' if inside_number => '5',
            // "HPl1162": a stroke between a word and a number is a space OCR misread.
            'l' | 'I' | '|' if letter(before) && digit(after) => ' ',
            _ => *c,
        });
    }
    out
}

fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    // "1,234,567" and "1.234.567" are thousands; "45.20" is a decimal.
    let grouped = |sep: char| {
        let parts: Vec<&str> = s.split(sep).collect();
        parts.len() > 1 && parts[1..].iter().all(|p| p.len() == 3) && !parts[0].is_empty() && parts[0].len() <= 3
    };
    let cleaned: String = if grouped(',') {
        s.replace(',', "")
    } else if grouped('.') && s.matches('.').count() > 1 {
        s.replace('.', "")
    } else {
        s.replace(',', ".")
    };
    cleaned.parse().ok()
}

/// The last one or two words before position `at`, as a label.
fn label_before(text: &str, at: usize) -> Option<String> {
    let before = &text[..at];
    let words: Vec<String> = before
        .split(|c: char| !c.is_alphanumeric() && c != '$' && c != '♥')
        .filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit()))
        .map(|w| w.to_lowercase())
        .collect();
    match words.len() {
        0 => None,
        1 => Some(words[0].clone()),
        n => {
            let two = format!("{} {}", words[n - 2], words[n - 1]);
            // Keep two words only for the common two-word labels.
            if ["time left", "hit points", "time remaining", "high score", "best time"].contains(&two.as_str()) {
                Some(two)
            } else {
                Some(words[n - 1].clone())
            }
        }
    }
}

/// Every reading in a line of text, left to right.
pub fn parse(text: &str) -> Vec<Labelled> {
    static RATIO: OnceLock<Regex> = OnceLock::new();
    static PERCENT: OnceLock<Regex> = OnceLock::new();
    static CLOCK: OnceLock<Regex> = OnceLock::new();
    static MINSEC: OnceLock<Regex> = OnceLock::new();
    static LEVEL: OnceLock<Regex> = OnceLock::new();
    static PAIR: OnceLock<Regex> = OnceLock::new();
    static COUNT: OnceLock<Regex> = OnceLock::new();
    let text = fix_digits(text);
    let mut out: Vec<(usize, usize, Labelled)> = Vec::new();
    let taken = |out: &Vec<(usize, usize, Labelled)>, s: usize, e: usize| out.iter().any(|(a, b, _)| s < *b && e > *a);
    let push = |out: &mut Vec<(usize, usize, Labelled)>, s: usize, e: usize, r: Reading, text: &str| {
        out.push((s, e, Labelled { label: label_before(text, s), reading: r }));
    };
    for c in re(&MINSEC, r"(?i)(\d{1,3})\s*(?:min|m)\s*(\d{1,2})\s*(?:sec|s)\b").captures_iter(&text) {
        let m = c.get(0).unwrap();
        let v = c[1].parse::<f64>().unwrap_or(0.0) * 60.0 + c[2].parse::<f64>().unwrap_or(0.0);
        push(&mut out, m.start(), m.end(), Reading::Time(v), &text);
    }
    for c in re(&RATIO, r"(\d[\d,\.]*)\s*/\s*(\d[\d,\.]*)").captures_iter(&text) {
        let m = c.get(0).unwrap();
        if let (Some(v), Some(mx)) = (number(&c[1]), number(&c[2]))
            && mx > 0.0
            && !taken(&out, m.start(), m.end())
        {
            push(&mut out, m.start(), m.end(), Reading::Ratio { value: v, max: mx }, &text);
        }
    }
    for c in re(&PERCENT, r"(\d[\d,\.]*)\s*%").captures_iter(&text) {
        let m = c.get(0).unwrap();
        if let Some(v) = number(&c[1])
            && !taken(&out, m.start(), m.end())
        {
            push(&mut out, m.start(), m.end(), Reading::Percent(v), &text);
        }
    }
    for c in re(&CLOCK, r"\b(\d{1,2}):(\d{2})(?::(\d{2}))?\b").captures_iter(&text) {
        let m = c.get(0).unwrap();
        if taken(&out, m.start(), m.end()) {
            continue;
        }
        let a: f64 = c[1].parse().unwrap_or(0.0);
        let b: f64 = c[2].parse().unwrap_or(0.0);
        let v = match c.get(3) {
            Some(s) => a * 3600.0 + b * 60.0 + s.as_str().parse::<f64>().unwrap_or(0.0),
            None => a * 60.0 + b,
        };
        push(&mut out, m.start(), m.end(), Reading::Time(v), &text);
    }
    for c in re(&LEVEL, r"(?i)\b(?:lv|lvl|level)\.?\s*(\d{1,4})\b").captures_iter(&text) {
        let m = c.get(0).unwrap();
        if !taken(&out, m.start(), m.end()) {
            let v: f64 = c[1].parse().unwrap_or(0.0);
            out.push((m.start(), m.end(), Labelled { label: Some("level".into()), reading: Reading::Level(v) }));
        }
    }
    for c in re(&PAIR, r"\b(\d{1,3})\s*[-–:]\s*(\d{1,3})\b").captures_iter(&text) {
        let m = c.get(0).unwrap();
        if !taken(&out, m.start(), m.end()) {
            let (a, b): (f64, f64) = (c[1].parse().unwrap_or(0.0), c[2].parse().unwrap_or(0.0));
            push(&mut out, m.start(), m.end(), Reading::Pair(a, b), &text);
        }
    }
    for c in re(&COUNT, r"(?:^|[^\w.])[x×]?(\d[\d,]*(?:\.\d+)?)").captures_iter(&text) {
        let g = c.get(1).unwrap();
        if taken(&out, g.start(), g.end()) {
            continue;
        }
        if let Some(v) = number(g.as_str()) {
            push(&mut out, g.start(), g.end(), Reading::Count(v), &text);
        }
    }
    out.sort_by_key(|(s, _, _)| *s);
    out.into_iter().map(|(_, _, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Labelled {
        let v = parse(text);
        assert_eq!(v.len(), 1, "{text}: {v:?}");
        v.into_iter().next().unwrap()
    }

    #[test]
    fn hud_texts_parse() {
        let l = one("HP 1240/1500");
        assert_eq!(l.label.as_deref(), Some("hp"));
        assert_eq!(l.reading, Reading::Ratio { value: 1240.0, max: 1500.0 });
        assert_eq!(one("71867 / 71867").reading, Reading::Ratio { value: 71867.0, max: 71867.0 });
        let l = one("EXP 45.20%");
        assert_eq!((l.label.as_deref(), l.reading), (Some("exp"), Reading::Percent(45.2)));
        assert_eq!(one("TIME 0:07").reading, Reading::Time(7.0));
        assert_eq!(one("Lv. 230").reading, Reading::Level(230.0));
        assert_eq!(one("SCORE 2 - 1").reading, Reading::Pair(2.0, 1.0));
        assert_eq!(one("29 min 32 sec").reading, Reading::Time(29.0 * 60.0 + 32.0));
        let l = one("GOLD 150");
        assert_eq!((l.label.as_deref(), l.reading), (Some("gold"), Reading::Count(150.0)));
        assert_eq!(one("AMMO 2O").reading, Reading::Count(20.0));
        assert_eq!(one("22,496,313,951").reading, Reading::Count(22_496_313_951.0));
        let l = one("HPl1162/1480)");
        assert_eq!((l.label.as_deref(), l.reading), (Some("hp"), Reading::Ratio { value: 1162.0, max: 1480.0 }));
        assert_eq!(one("Defeat slimes (4/10)").reading, Reading::Ratio { value: 4.0, max: 10.0 });
    }

    #[test]
    fn several_readings_in_a_line() {
        let v = parse("22,496,313,951 [26.888%]");
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!(v[1].reading, Reading::Percent(26.888));
        assert!(parse("Mossy Hills").is_empty());
    }
}
