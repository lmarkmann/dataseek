//! The filters `search` applies to the merged list (ADR 0020). They read a
//! merged hit as it is shown: its license and date are the first ones a
//! source stated, its formats every source's. They hold alike for every
//! source whatever its own API can filter.
//! A dataset that does not state a field never passes a filter on it: a
//! license filter keeps only datasets that name a matching license.

use crate::cli::Filters;
use crate::dedup::Hit;
use crate::license;
use crate::record::file_format;

/// The filters, whose values the parsers below normalized as the records'
/// are, so a license asked for as `cc by 4.0` matches one recorded as
/// `CC-BY-4.0`.
#[derive(Debug, Default)]
pub struct Wanted {
    pub licenses: Vec<String>,
    /// [`file_format`] names.
    pub formats: Vec<String>,
    /// A full ISO day.
    pub since: Option<String>,
    pub min_sources: usize,
}

impl Wanted {
    pub fn new(filters: &Filters) -> Self {
        Self {
            licenses: filters.license.clone(),
            formats: filters.format.clone(),
            since: filters.updated_since.clone(),
            min_sources: filters.min_sources.map_or(0, usize::from),
        }
    }

    /// Whether any filter is set.
    pub fn any(&self) -> bool {
        !self.licenses.is_empty()
            || !self.formats.is_empty()
            || self.since.is_some()
            || self.min_sources > 1
    }

    pub fn admits(&self, hit: &Hit) -> bool {
        let d = &hit.dataset;
        let licensed = self.licenses.is_empty()
            || d.license.as_deref().is_some_and(|l| {
                self.licenses.iter().any(|w| license::matches(l, w))
            });
        let offered = self.formats.is_empty()
            || d.formats.iter().any(|f| self.formats.contains(f));
        let recent = self.since.as_deref().is_none_or(|since| {
            d.updated.as_deref().is_some_and(|day| on_or_after(day, since))
        });
        licensed && offered && recent && hit.sources.len() >= self.min_sources
    }
}

/// `--license` as the records spell it. A word that names no license, such
/// as `unknown`, is refused rather than dropped, so the search never runs
/// unfiltered without saying so.
pub fn license(raw: &str) -> Result<String, String> {
    license::normalized(raw).ok_or_else(|| {
        format!("`{raw}` names no license; write an SPDX id such as CC-BY-4.0")
    })
}

/// `--format` as the records spell it, refused when it names no format.
pub fn format(raw: &str) -> Result<String, String> {
    file_format(raw).ok_or_else(|| {
        format!("`{raw}` names no file format; write one such as csv")
    })
}

/// What [`day`] accepts, as a JSON Schema `pattern`.
pub const DAY_PATTERN: &str = r"^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$";

/// `--updated-since` as a full day: `2024` is `2024-01-01`, `2024-07` is
/// `2024-07-01`.
pub fn day(raw: &str) -> Result<String, String> {
    let day = iso_prefix(raw.trim())
        .filter(|prefix| prefix.len() == raw.trim().len())
        .filter(|prefix| on_the_calendar(prefix))
        .ok_or_else(|| {
            format!("`{raw}` is not a day; write YYYY, YYYY-MM or YYYY-MM-DD")
        })?;
    Ok(match day.len() {
        4 => format!("{day}-01-01"),
        7 => format!("{day}-01"),
        _ => day.to_owned(),
    })
}

/// Whether a `YYYY-MM-DD` date names a day the month has: no 30 February.
/// A year or a month alone always does.
fn on_the_calendar(date: &str) -> bool {
    let part = |range: std::ops::Range<usize>| {
        date.get(range).and_then(|s| s.parse::<u32>().ok())
    };
    let (Some(year), Some(month), Some(day)) =
        (part(0..4), part(5..7), part(8..10))
    else {
        return true;
    };
    let leap = year.is_multiple_of(4)
        && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let last = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= last
}

/// Whether a recorded date is on or after `since`, a full day, compared to
/// the precision the record gives: a dataset dated only `2024` counts as
/// updated after `2024-06-01`, since it may well have been.
fn on_or_after(recorded: &str, since: &str) -> bool {
    iso_prefix(recorded).is_some_and(|date| {
        since.get(..date.len()).is_some_and(|wanted| date >= wanted)
    })
}

/// The leading `YYYY`, `YYYY-MM` or `YYYY-MM-DD` of a date, month and day
/// in range.
fn iso_prefix(text: &str) -> Option<&str> {
    let digits = |range: std::ops::Range<usize>| {
        text.get(range).filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
    };
    let in_range = |range, low, high| {
        digits(range)
            .and_then(|s| s.parse::<u8>().ok())
            .is_some_and(|n| (low..=high).contains(&n))
    };
    digits(0..4)?;
    let dash = |at: usize| text.get(at..at.saturating_add(1)) == Some("-");
    let length = if dash(4) && in_range(5..7, 1, 12) {
        if dash(7) && in_range(8..10, 1, 31) { 10 } else { 7 }
    } else {
        4
    };
    text.get(..length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Dataset;

    fn hit(
        license: Option<&str>,
        updated: Option<&str>,
        sources: usize,
    ) -> Hit {
        let mut dataset = Dataset::new("Rain", "https://x.org/rain");
        dataset.license = license.map(String::from);
        dataset.updated = updated.map(String::from);
        Hit {
            dataset,
            sources: ["zenodo", "datacite", "openaire"]
                .into_iter()
                .take(sources)
                .collect(),
            #[cfg(feature = "internals")]
            ranks: vec![0; sources],
            score: 1.0,
        }
    }

    fn wanted(license: &[&str], since: Option<&str>, min: u16) -> Wanted {
        Wanted::new(&Filters {
            license: license
                .iter()
                .map(|l| super::license(l).unwrap())
                .collect(),
            format: Vec::new(),
            updated_since: since.map(|s| day(s).unwrap()),
            min_sources: Some(min),
        })
    }

    #[test]
    fn days_are_full_iso_days_or_refused() {
        assert_eq!(day("2024").as_deref(), Ok("2024-01-01"));
        assert_eq!(day("2024-07").as_deref(), Ok("2024-07-01"));
        assert_eq!(day("2024-07-30").as_deref(), Ok("2024-07-30"));
        assert_eq!(day("2024-02-29").as_deref(), Ok("2024-02-29"));
        for bad in [
            "24",
            "2024-13",
            "2024-7-1",
            "July 2024",
            "2024-07-30T00",
            "2024-02-30",
            "2023-02-29",
            "2023-04-31",
            "1900-02-29",
        ] {
            assert!(day(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_value_that_names_nothing_is_refused() {
        assert_eq!(license("cc by 4.0").as_deref(), Ok("CC-BY-4.0"));
        assert!(license("unknown").is_err());
        assert!(license(" ").is_err());
        assert_eq!(format(".CSV").as_deref(), Ok("CSV"));
        assert!(format("not provided").is_err());
    }

    #[test]
    fn a_date_counts_at_the_precision_it_is_recorded_with() {
        assert!(on_or_after("2024-07-30", "2024-07-30"));
        assert!(!on_or_after("2024-07-29", "2024-07-30"));
        assert!(on_or_after("2024", "2024-06-01"));
        assert!(on_or_after("2024-06", "2024-06-15"));
        assert!(!on_or_after("2023-12-31T23:59:59Z", "2024-01-01"));
        assert!(!on_or_after("Feb 17, 2026", "2020-01-01"));
    }

    #[test]
    fn a_license_filter_matches_ids_and_families_and_drops_the_unstated() {
        let by = wanted(&["cc by"], None, 1);
        assert!(by.admits(&hit(Some("CC-BY-4.0"), None, 1)));
        assert!(by.admits(&hit(Some("CC-BY-3.0-AU"), None, 1)));
        assert!(!by.admits(&hit(Some("CC-BY-SA-4.0"), None, 1)));
        assert!(!by.admits(&hit(None, None, 1)));
        let exact = wanted(&["CC-BY-4.0", "cc0"], None, 1);
        assert!(exact.admits(&hit(Some("CC0-1.0"), None, 1)));
        assert!(!exact.admits(&hit(Some("CC-BY-3.0"), None, 1)));
    }

    #[test]
    fn every_filter_must_hold() {
        let all = wanted(&["cc-by"], Some("2024"), 2);
        assert!(all.any());
        assert!(all.admits(&hit(Some("CC-BY-4.0"), Some("2025-01-02"), 2)));
        assert!(!all.admits(&hit(Some("CC-BY-4.0"), Some("2023-01-02"), 2)));
        assert!(!all.admits(&hit(Some("CC-BY-4.0"), Some("2025-01-02"), 1)));
        assert!(!all.admits(&hit(Some("CC-BY-4.0"), None, 3)));
        assert!(!wanted(&[], None, 1).any());
        let csv = Wanted::new(&Filters {
            format: vec![format("text/csv").unwrap()],
            ..Filters::default()
        });
        let mut offered = hit(None, None, 1);
        offered.dataset.formats = vec!["PDF".into(), "CSV".into()];
        assert!(csv.admits(&offered));
        assert!(!csv.admits(&hit(None, None, 1)));
        assert!(wanted(&[], None, 1).admits(&hit(None, None, 1)));
    }
}
