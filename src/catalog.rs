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
    let all = tokens(query);
    let meaningful: Vec<String> = all
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    if meaningful.is_empty() { all } else { meaningful }
}

/// How many of `terms` appear as a token prefix in `text`.
pub fn matched(text: &str, terms: &[String]) -> usize {
    let haystack = tokens(text).join(" ");
    terms.iter().filter(|t| contains_token_prefix(&haystack, t)).count()
}

pub fn search(entries: &[Dataset], query: &str, limit: usize) -> Vec<Dataset> {
    let terms = terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let phrase = terms.join(" ");
    let mut scored: Vec<(u32, &Dataset)> = entries
        .iter()
        .filter_map(|entry| score(entry, &terms, &phrase).map(|s| (s, entry)))
        .collect();
    scored.sort_by(|(a, x), (b, y)| {
        b.cmp(a).then_with(|| x.title.len().cmp(&y.title.len()))
    });
    scored.into_iter().take(limit).map(|(_, entry)| entry.clone()).collect()
}

fn score(entry: &Dataset, terms: &[String], phrase: &str) -> Option<u32> {
    let title = tokens(&entry.title).join(" ");
    let body = entry
        .description
        .as_deref()
        .map(|d| tokens(d).join(" "))
        .unwrap_or_default();
    let mut total: u32 = 0;
    for term in terms {
        let in_title = contains_token_prefix(&title, term);
        let in_body = contains_token_prefix(&body, term);
        if !in_title && !in_body {
            return None;
        }
        total = total.saturating_add(if in_title { 3 } else { 1 });
    }
    if title.contains(phrase) {
        total = total.saturating_add(10);
    }
    Some(total)
}

/// Whether some token in `haystack` starts with `term`, so "temp" matches
/// "temperature" but "rain" does not match "terrain".
fn contains_token_prefix(haystack: &str, term: &str) -> bool {
    haystack.split(' ').any(|token| token.starts_with(term))
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
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
