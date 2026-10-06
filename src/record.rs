//! The one record shape every source adapter returns, plus the helpers that
//! turn a source's JSON into it: pointer lookups that never panic, HTML
//! stripped to a line of text, and DOIs normalized so deduplication can match
//! them across sources. Adapters fill what they have and leave the rest `None`;
//! they never invent a value to make a record look complete.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Descriptions are a teaser, not the abstract: the landing page has the rest.
const SUMMARY_CHARS: usize = 320;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dataset {
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Downloads, votes or stars, whatever the source counts. Comparable only
    /// within one source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popularity: Option<u64>,
    /// Other identifiers for the same dataset (a concept DOI, a mirror URL),
    /// used only to merge duplicates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl Dataset {
    pub fn new(title: &str, url: &str) -> Self {
        Self {
            title: clean(title),
            url: url.trim().to_owned(),
            ..Self::default()
        }
    }

    /// A record without a title or a link cannot be shown or opened, so the
    /// adapters drop it instead of passing noise on.
    ///
    /// The link is also the one field printed without passing through
    /// [`clean`], so one carrying whitespace or a control character is
    /// treated as no link at all.
    pub fn valid(self) -> Option<Self> {
        let link_is_plain =
            self.url.chars().all(|c| !c.is_control() && !c.is_whitespace());
        (!self.title.is_empty()
            && self.url.starts_with("http")
            && link_is_plain)
            .then_some(self)
    }

    pub fn describe(mut self, text: Option<String>) -> Self {
        self.description = text.and_then(|t| summary(&t));
        self
    }

    pub fn doi_from(mut self, raw: Option<String>) -> Self {
        self.doi = raw.and_then(|r| doi(&r));
        self
    }
}

/// HTML tags removed (a `<` opens one only before a letter, `/` or `!`, so
/// "aged <5" survives), the common entities decoded, whitespace collapsed,
/// and every control character (ESC, CSI, OSC, C1) replaced by a space, so a
/// record from a remote source can never drive the terminal it is printed
/// on.
pub fn clean(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut in_tag = false;
    let mut chars = without_controls(text).peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' if !in_tag
                && chars.peek().is_some_and(|n| {
                    n.is_alphabetic() || matches!(n, '/' | '!')
                }) =>
            {
                in_tag = true;
            }
            '>' if in_tag => {
                in_tag = false;
                plain.push(' ');
            }
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    decode_entities(&plain).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTML entities decoded in one pass, so `&amp;lt;` stays the text `&lt;`.
/// Unknown names and numeric codes for control characters stay as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once('&') {
        out.push_str(head);
        let decoded = tail
            .split_once(';')
            .filter(|(name, _)| name.len() <= 8)
            .and_then(|(name, after)| Some((entity(name)?, after)));
        if let Some((c, after)) = decoded {
            out.push(c);
            rest = after;
        } else {
            out.push('&');
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    let named = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "rarr" => '\u{2192}',
        _ => {
            let code = name.strip_prefix('#')?;
            let number = match code.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            return char::from_u32(number).filter(|c| !c.is_control());
        }
    };
    Some(named)
}

/// [`clean`], cut to a teaser on a char boundary. `None` when nothing is left.
pub fn summary(text: &str) -> Option<String> {
    shortened(text, SUMMARY_CHARS)
}

/// [`clean`], cut to at most `chars` on a word, with `...` marking a cut.
pub fn shortened(text: &str, chars: usize) -> Option<String> {
    let plain = clean(text);
    if plain.is_empty() {
        return None;
    }
    if plain.chars().count() <= chars {
        return Some(plain);
    }
    let cut: String = plain.chars().take(chars).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    Some(format!("{}...", cut.trim_end_matches([',', '.', ';', ':'])))
}

/// A bare lowercase DOI (`10.5281/zenodo.1`) from any of the forms sources
/// use: `doi:`, `https://doi.org/`, `http://dx.doi.org/`, or bare. A DOI
/// ends at the first space, so a trailing note like "(version 2)" is dropped.
pub fn doi(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase();
    let start = lower.find("10.")?;
    let candidate = lower
        .get(start..)?
        .split_whitespace()
        .next()?
        .trim_end_matches(['/', '.']);
    let (prefix, suffix) = candidate.split_once('/')?;
    let registrant = prefix.strip_prefix("10.")?;
    let plausible = registrant.len() >= 4
        && registrant.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !suffix.is_empty();
    plausible.then(|| candidate.to_owned())
}

/// The string at a JSON pointer, trimmed, with control characters replaced
/// by spaces (see [`clean`]); numbers are rendered as text. `None` for
/// absent, null, empty and non-scalar values.
pub fn text(value: &Value, pointer: &str) -> Option<String> {
    match value.pointer(pointer)? {
        Value::String(s) => {
            let safe: String = without_controls(s).collect();
            let trimmed = safe.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The first of several pointers that holds text.
pub fn first_text(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|p| text(value, p))
}

/// A non-negative integer at a pointer, accepting numeric strings and
/// truncating floats, which sources use interchangeably.
pub fn number(value: &Value, pointer: &str) -> Option<u64> {
    let (whole, float) = match value.pointer(pointer)? {
        Value::Number(n) => (n.as_u64(), n.as_f64()),
        Value::String(s) => (s.trim().parse().ok(), s.trim().parse().ok()),
        _ => return None,
    };
    whole.or_else(|| {
        float.filter(|f: &f64| f.is_finite() && *f >= 0.0).map(float_to_u64)
    })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "callers pass finite non-negative values; truncation is the intent"
)]
fn float_to_u64(f: f64) -> u64 {
    f as u64
}

/// The array at a pointer, or an empty slice when it is absent or not an
/// array, so adapters can iterate without a branch.
pub fn items<'a>(value: &'a Value, pointer: &str) -> &'a [Value] {
    value.pointer(pointer).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// A string from a value that is either a plain string or a language map
/// (`{"en": "...", "de": "..."}`), preferring English.
pub fn localized(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.as_str()),
        Value::Object(map) => map
            .get("en")
            .and_then(Value::as_str)
            .or_else(|| map.values().find_map(Value::as_str)),
        _ => None,
    }
    .map(|s| without_controls(s).collect::<String>().trim().to_owned())
    .filter(|s| !s.is_empty())
}

/// Every control character (ESC, CSI, OSC, C1) replaced by a space.
fn without_controls(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().map(|c| if c.is_control() { ' ' } else { c })
}

/// Unix seconds or milliseconds as an ISO date (`2024-01-31`).
pub fn date_from_epoch(epoch: u64) -> Option<String> {
    let seconds =
        if epoch > 100_000_000_000 { epoch.checked_div(1000)? } else { epoch };
    let days = i64::try_from(seconds.checked_div(86_400)?).ok()?;
    let (y, m, d) = civil_from_days(days);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Howard Hinnant's days-to-civil algorithm, in checked arithmetic.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = doe
        .saturating_sub(doe / 1460)
        .saturating_add(doe / 36_524)
        .saturating_sub(doe / 146_096)
        / 365;
    let doy = doe.saturating_sub(
        yoe.saturating_mul(365)
            .saturating_add(yoe / 4)
            .saturating_sub(yoe / 100),
    );
    let mp = doy.saturating_mul(5).saturating_add(2) / 153;
    let d = doy
        .saturating_sub(mp.saturating_mul(153).saturating_add(2) / 5)
        .saturating_add(1);
    let m = if mp < 10 { mp.saturating_add(3) } else { mp.saturating_sub(9) };
    let y = yoe
        .saturating_add(era.saturating_mul(400))
        .saturating_add(i64::from(m <= 2));
    (y, m, d)
}

/// The date part of an ISO timestamp, or a compact `20220124` spelled out,
/// so every source prints dates alike.
pub fn day(timestamp: Option<String>) -> Option<String> {
    let t = timestamp?;
    let head: String = t.chars().take(10).collect();
    let looks_iso = head.len() == 10
        && head.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() }
        });
    if looks_iso {
        return Some(head);
    }
    match (t.get(0..4), t.get(4..6), t.get(6..8)) {
        (Some(y), Some(m), Some(d))
            if t.len() == 8 && t.chars().all(|c| c.is_ascii_digit()) =>
        {
            Some(format!("{y}-{m}-{d}"))
        }
        _ => Some(t),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    proptest! {
        #[test]
        fn cleaned_text_is_printable_and_tidy(raw in any::<String>()) {
            let shown = clean(&raw);
            prop_assert!(!shown.chars().any(char::is_control), "{shown:?}");
            prop_assert_eq!(shown.trim(), shown.as_str());
            prop_assert!(!shown.contains("  "), "{shown:?}");
        }

        #[test]
        fn text_without_markup_only_has_its_spacing_tidied(
            raw in "[a-zA-Z0-9 .,;:()%'\"-]{0,80}"
        ) {
            let tidy = raw.split_whitespace().collect::<Vec<_>>().join(" ");
            prop_assert_eq!(clean(&raw), tidy);
        }

        #[test]
        fn a_normalized_doi_normalizes_to_itself(
            raw in "[a-z :/]{0,8}10\\.[0-9.]{3,8}/[a-zA-Z0-9 ./:()-]{0,20}"
        ) {
            if let Some(bare) = doi(&raw) {
                prop_assert!(!bare.contains(char::is_whitespace), "{bare:?}");
                prop_assert_eq!(doi(&bare), Some(bare.clone()));
            }
        }

        #[test]
        fn every_doi_spelling_gives_the_bare_form(
            registrant in "[0-9]{4,6}",
            suffix in "[a-z0-9_-][a-z0-9._/-]{0,20}[a-z0-9_-]",
            resolver in prop::sample::select(vec![
                "", "doi:", "https://doi.org/", "http://dx.doi.org/", " DOI: ",
            ]),
        ) {
            let bare = format!("10.{registrant}/{suffix}");
            let spelled = format!("{resolver}{}", bare.to_uppercase());
            prop_assert_eq!(doi(&spelled), Some(bare));
        }
    }

    #[test]
    fn clean_strips_markup_and_collapses_space() {
        assert_eq!(
            clean("<p>Rain &amp; snow</p>\n\n<b>daily</b>"),
            "Rain & snow daily"
        );
    }

    #[test]
    fn summary_cuts_on_a_word_and_marks_the_cut() {
        // 53 "data, " fill 318 chars, so the 320-char cut lands inside the
        // 54th word, right after a comma.
        let long = "data, ".repeat(100);
        let expected = format!("{}...", vec!["data"; 53].join(", "));
        assert_eq!(summary(&long), Some(expected));
        assert_eq!(summary("short, as is.").as_deref(), Some("short, as is."));
        assert_eq!(summary("<p> </p>"), None);
    }

    #[test]
    fn doi_accepts_every_spelling_and_rejects_lookalikes() {
        for raw in [
            "10.5281/ZENODO.1",
            "doi:10.5281/zenodo.1",
            "https://doi.org/10.5281/zenodo.1",
            "http://dx.doi.org/10.5281/zenodo.1/",
            "https://doi.org/10.5281/zenodo.1 (version 2)",
        ] {
            assert_eq!(doi(raw).as_deref(), Some("10.5281/zenodo.1"), "{raw}");
        }
        assert_eq!(doi("version 10.2 of the data"), None);
        assert_eq!(doi("10.12/x"), None);
    }

    #[test]
    fn pointers_tolerate_missing_and_mixed_types() {
        let v = json!({"a": {"b": " x ", "n": 3, "s": "42", "f": 7.9, "fs": "1197.0"}});
        assert_eq!(text(&v, "/a/b").as_deref(), Some("x"));
        assert_eq!(text(&v, "/a/n").as_deref(), Some("3"));
        assert_eq!(text(&v, "/a/zz"), None);
        assert_eq!(number(&v, "/a/s"), Some(42));
        assert_eq!(number(&v, "/a/f"), Some(7));
        assert_eq!(number(&v, "/a/fs"), Some(1197));
        assert_eq!(items(&v, "/a").len(), 0);
    }

    #[test]
    fn localized_prefers_english() {
        let map = json!({"de": "Wasser", "en": "Water"});
        assert_eq!(localized(Some(&map)).as_deref(), Some("Water"));
        assert_eq!(
            localized(Some(&json!({"fr": "Eau"}))).as_deref(),
            Some("Eau")
        );
    }

    #[test]
    fn epochs_render_as_iso_days() {
        assert_eq!(date_from_epoch(0).as_deref(), Some("1970-01-01"));
        assert_eq!(
            date_from_epoch(1_785_951_652_000).as_deref(),
            Some("2026-08-05")
        );
        assert_eq!(
            date_from_epoch(951_782_400).as_deref(),
            Some("2000-02-29")
        );
    }

    #[test]
    fn named_and_numeric_entities_decode_but_never_to_controls() {
        assert_eq!(
            clean("England&rsquo;s &apos;map&apos; &#8211; &#x2014; &mdash;"),
            "England\u{2019}s 'map' \u{2013} \u{2014} \u{2014}"
        );
        assert_eq!(clean("&amp;lt; stays &lt;"), "&lt; stays <");
        assert_eq!(clean("R&D &unknown; &#xZZ;"), "R&D &unknown; &#xZZ;");
        assert!(!clean("&#27;[2J&#x9b;").chars().any(char::is_control));
    }

    #[test]
    fn a_literal_less_than_sign_is_text_not_a_tag() {
        assert_eq!(clean("Children aged <5 years"), "Children aged <5 years");
        assert_eq!(clean("PM2.5 < 10 and > 2"), "PM2.5 < 10 and > 2");
        assert_eq!(clean("a<b>bold</b> c<!-- x -->d"), "a bold c d");
    }

    #[test]
    fn day_keeps_only_the_date_of_iso_stamps() {
        assert_eq!(
            day(Some("2024-01-04T12:09:45Z".into())).as_deref(),
            Some("2024-01-04")
        );
        assert_eq!(
            day(Some("Feb 17, 2026".into())).as_deref(),
            Some("Feb 17, 2026")
        );
        assert_eq!(
            day(Some("20100708".into())).as_deref(),
            Some("2010-07-08")
        );
        assert_eq!(day(Some("201007".into())).as_deref(), Some("201007"));
    }

    #[test]
    fn remote_text_cannot_carry_terminal_escapes() {
        let hostile = "Title\u{1b}]8;;https://evil\u{7}x\u{1b}[2J\u{9b}31m";
        assert!(!clean(hostile).chars().any(char::is_control));
        let v = json!({"t": hostile});
        assert!(!text(&v, "/t").unwrap().chars().any(char::is_control));
        for map in
            [json!(hostile), json!({"en": hostile}), json!({"de": hostile})]
        {
            let shown = localized(Some(&map)).unwrap();
            assert!(!shown.chars().any(char::is_control), "{shown:?}");
        }
        assert!(
            Dataset::new("t", "https://x.org/\u{1b}[2J").valid().is_none()
        );
        assert!(Dataset::new("t", "https://x.org/a b").valid().is_none());
    }

    #[test]
    fn records_without_title_or_link_are_dropped() {
        assert!(Dataset::new("t", "https://x").valid().is_some());
        assert!(Dataset::new("", "https://x").valid().is_none());
        assert!(Dataset::new("t", "ftp://x").valid().is_none());
    }
}
