//! Local search over a downloaded catalog, for the sources that publish a
//! complete list but no search endpoint: UCI, OpenML, SDMX dataflows, STAC
//! collections, the Earth Engine and AWS registries, and the like.
//!
//! Every query term must appear in the title or the description; a term in
//! the title counts three times as much, and the whole query as a phrase in
//! the title wins outright. Matching is on lowercase alphanumeric tokens, so
//! "CO2" finds "co2-emissions" and "sea surface" finds "Sea-Surface". The
//! scorer is deliberately simple: these catalogs hold hundreds to tens of
//! thousands of short entries, not documents.

use crate::record::Dataset;

/// Words too common in dataset metadata to say anything about relevance.
const STOPWORDS: [&str; 12] = [
    "a", "an", "and", "the", "of", "in", "for", "on", "to", "with", "data",
    "dataset",
];

/// The query's meaningful words, lowercase. Falls back to every word when
/// the query is nothing but stopwords ("the data").
pub fn terms(query: &str) -> Vec<String> {
    let all: Vec<String> =
        words(query).split_whitespace().map(str::to_owned).collect();
    let meaningful: Vec<String> = all
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    if meaningful.is_empty() { all } else { meaningful }
}

/// Each term with a space in front. Matched against [`words`], which also
/// starts with a space, a needle occurs exactly when some word starts with
/// its term.
pub fn needles(terms: &[String]) -> Vec<String> {
    terms.iter().map(|t| format!(" {t}")).collect()
}

/// How many of `needles` appear as a word prefix in `text`.
pub fn matched(text: &str, needles: &[String]) -> usize {
    let words = words(text);
    needles.iter().filter(|n| words.contains(n.as_str())).count()
}

pub fn search(entries: &[Dataset], query: &str, limit: usize) -> Vec<Dataset> {
    let terms = terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let needles = needles(&terms);
    let phrase = terms.join(" ");
    let (mut title, mut body) = (String::new(), String::new());
    let mut scored: Vec<(u32, &Dataset, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            words_into(&entry.title, &mut title);
            let score = score(entry, &title, &mut body, &needles, &phrase)?;
            Some((score, entry, i))
        })
        .collect();
    let order = |(a, x, i): &(u32, &Dataset, usize),
                 (b, y, j): &(u32, &Dataset, usize)| {
        b.cmp(a)
            .then_with(|| x.title.len().cmp(&y.title.len()))
            .then_with(|| i.cmp(j))
    };
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit, order);
        scored.truncate(limit);
    }
    scored.sort_unstable_by(order);
    scored.into_iter().map(|(_, entry, _)| entry.clone()).collect()
}

/// The entry's score, or `None` when some term is in neither its title nor
/// its description. `title` holds the title's [`words`]; the description's
/// go into `body` only once a term is missing from the title.
fn score(
    entry: &Dataset,
    title: &str,
    body: &mut String,
    needles: &[String],
    phrase: &str,
) -> Option<u32> {
    let mut body_read = false;
    let mut total: u32 = 0;
    for needle in needles {
        let points = if title.contains(needle.as_str()) {
            3
        } else {
            if !body_read {
                words_into(entry.description.as_deref().unwrap_or(""), body);
                body_read = true;
            }
            if !body.contains(needle.as_str()) {
                return None;
            }
            1
        };
        total = total.saturating_add(points);
    }
    if title.contains(phrase) {
        total = total.saturating_add(10);
    }
    Some(total)
}

/// `text` lowercased, split on everything that is not a letter or a digit,
/// and joined back with a space before every word, so "CO2-emissions"
/// becomes " co2 emissions".
fn words(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(1));
    words_into(text, &mut out);
    out
}

fn words_into(text: &str, out: &mut String) {
    out.clear();
    if text.is_ascii() {
        for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            if !word.is_empty() {
                out.push(' ');
                out.push_str(word);
            }
        }
        out.make_ascii_lowercase();
    } else {
        for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if !word.is_empty() {
                out.push(' ');
                out.push_str(word);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, description: &str) -> Dataset {
        Dataset::new(title, "https://example.org")
            .describe(Some(description.to_owned()))
    }

    #[test]
    fn every_term_must_match_somewhere() {
        let entries = [
            entry("Sea surface temperature", "monthly grids"),
            entry("Sea level", "tide gauges"),
        ];
        let hits = search(&entries, "sea temperature", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Sea surface temperature");
    }

    #[test]
    fn title_matches_outrank_description_matches() {
        let entries = [
            entry("Global grids", "rainfall estimates"),
            entry("Rainfall estimates", "global grids"),
        ];
        let hits = search(&entries, "rainfall", 10);
        assert_eq!(hits[0].title, "Rainfall estimates");
    }

    #[test]
    fn terms_match_token_prefixes_not_substrings() {
        let entries = [entry("Terrain model", ""), entry("Rainfall", "")];
        let hits = search(&entries, "rain", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Rainfall");
    }

    #[test]
    fn punctuation_and_case_do_not_matter() {
        let entries = [entry("CO2-emissions per capita", "")];
        assert_eq!(search(&entries, "co2 Emissions", 10).len(), 1);
        assert_eq!(search(&entries, "   ", 10).len(), 0);
    }

    #[test]
    fn a_query_of_only_stopwords_still_searches_its_words() {
        assert_eq!(terms("the data"), ["the", "data"]);
        assert_eq!(terms("the ocean data"), ["ocean"]);
    }

    #[test]
    fn the_whole_query_as_a_title_phrase_wins() {
        let entries = [
            entry("Temperature at the sea surface", ""),
            entry("Sea surface temperature, global monthly grids", ""),
        ];
        let hits = search(&entries, "sea surface temperature", 10);
        assert_eq!(
            hits[0].title,
            "Sea surface temperature, global monthly grids"
        );
    }
}
