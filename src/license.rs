//! Licenses as SPDX ids, so one license reads the same from every source
//! and `--license` can match it. Sources spell a license many ways: a
//! Creative Commons deed URL, a CKAN slug (`cc-by`, `odc-odbl`, `uk-ogl`), a
//! dcat-ap.de or EU vocabulary URI, or a name ("Attribution 4.0
//! International (CC BY 4.0)"); six queries over every source in October
//! 2026 returned 180 spellings, about 30 of them for CC BY 4.0. What is
//! recognized becomes its SPDX id. A Creative Commons license named without
//! a version keeps its family alone (`CC-BY`), and an Open Government
//! Licence named without one is the UK's current version 3.0. Anything else
//! is shown as the source wrote it, and placeholders such as "other" or
//! "notspecified", which say nothing about the terms, are dropped.

use crate::record::clean;

/// Slugs that name no license.
const PLACEHOLDERS: &[&str] = &[
    "other",
    "unknown",
    "none",
    "notspecified",
    "not-specified",
    "unspecified",
    "unsupported",
    "no-license",
    "other-specified-in-description",
];

/// Names and slugs of licenses whose SPDX id no rule below derives.
const NAMES: &[(&str, &str)] = &[
    ("mit", "MIT"),
    ("mit-license", "MIT"),
    ("apache-2.0", "Apache-2.0"),
    ("apache-2", "Apache-2.0"),
    ("apache2", "Apache-2.0"),
    ("apache-license-2.0", "Apache-2.0"),
    ("apache-license-version-2.0", "Apache-2.0"),
    ("bsd-2-clause", "BSD-2-Clause"),
    ("bsd-2-clause-simplified-license", "BSD-2-Clause"),
    ("bsd-3-clause", "BSD-3-Clause"),
    ("bsd-3-clause-new-revised-license", "BSD-3-Clause"),
    ("mpl-2.0", "MPL-2.0"),
    ("mozilla-public-license-2.0", "MPL-2.0"),
    ("epl-1.0", "EPL-1.0"),
    ("eclipse-public-license-1.0", "EPL-1.0"),
    ("epl-2.0", "EPL-2.0"),
    ("eclipse-public-license-2.0", "EPL-2.0"),
    ("afl-3.0", "AFL-3.0"),
    ("academic-free-license-afl-3.0", "AFL-3.0"),
    ("artistic-2.0", "Artistic-2.0"),
    ("artistic-license-2.0", "Artistic-2.0"),
    ("unlicense", "Unlicense"),
    ("the-unlicense", "Unlicense"),
    ("isc", "ISC"),
    ("cdla-permissive-1.0", "CDLA-Permissive-1.0"),
    (
        "community-data-license-agreement-permissive-version-1.0",
        "CDLA-Permissive-1.0",
    ),
    ("cdla-permissive-2.0", "CDLA-Permissive-2.0"),
    (
        "community-data-license-agreement-permissive-version-2.0",
        "CDLA-Permissive-2.0",
    ),
    ("cdla-sharing-1.0", "CDLA-Sharing-1.0"),
    (
        "community-data-license-agreement-sharing-version-1.0",
        "CDLA-Sharing-1.0",
    ),
    ("odbl", "ODbL-1.0"),
    ("odbl-1.0", "ODbL-1.0"),
    ("odc-odbl", "ODbL-1.0"),
    ("open-database-license", "ODbL-1.0"),
    ("open-database-license-odbl", "ODbL-1.0"),
    ("open-database-license-odc-odbl", "ODbL-1.0"),
    ("odc-by", "ODC-By-1.0"),
    ("odc-by-1.0", "ODC-By-1.0"),
    ("odby", "ODC-By-1.0"),
    ("open-data-commons-attribution-license", "ODC-By-1.0"),
    ("pddl", "PDDL-1.0"),
    ("pddl-1.0", "PDDL-1.0"),
    ("odc-pddl", "PDDL-1.0"),
    ("odcpddl", "PDDL-1.0"),
    ("open-data-commons-public-domain-dedication-and-license", "PDDL-1.0"),
    ("odc-public-domain-dedication-and-licence-pddl", "PDDL-1.0"),
    ("dl-de-by-2.0", "DL-DE-BY-2.0"),
    ("dl-by-de-2.0", "DL-DE-BY-2.0"),
    ("dl-de-zero-2.0", "DL-DE-ZERO-2.0"),
    ("dl-zero-de-2.0", "DL-DE-ZERO-2.0"),
    ("etalab-2.0", "etalab-2.0"),
    ("licence-ouverte-2.0", "etalab-2.0"),
];

/// The license as shown and matched: its SPDX id when recognized, the
/// source's own text otherwise, `None` for a placeholder.
pub fn normalized(raw: &str) -> Option<String> {
    let shown = clean(raw);
    let shown = shown.trim_matches(['[', ']', ' ']);
    if shown.is_empty() || PLACEHOLDERS.contains(&slug(shown).as_str()) {
        return None;
    }
    Some(spdx(shown).unwrap_or_else(|| shown.to_owned()))
}

/// The SPDX id a license name, slug or URI stands for.
pub fn spdx(raw: &str) -> Option<String> {
    match raw.trim().split_once("://") {
        Some((_, address)) => from_uri(address),
        None => from_name(&slug(raw)),
    }
}

/// An id without its version: `CC-BY-4.0` is `CC-BY`, `CC0-1.0` is `CC0`,
/// `GPL-3.0-only` is `GPL`. An id with no version is its own family.
pub fn family(id: &str) -> &str {
    id.match_indices('-')
        .find(|(i, _)| {
            id.get(i.saturating_add(1)..).is_some_and(|rest| {
                rest.starts_with(|c: char| c.is_ascii_digit())
            })
        })
        .and_then(|(i, _)| id.get(..i))
        .unwrap_or(id)
}

/// Whether a license is what `--license` asked for: the same id, or the
/// family it belongs to, ignoring case.
pub fn matches(license: &str, wanted: &str) -> bool {
    license.eq_ignore_ascii_case(wanted)
        || family(license).eq_ignore_ascii_case(wanted)
}

/// Lowercase words joined by `-`, every other character a separator, and
/// an `_` between digits a dot, as the EU vocabulary writes `CC_BY_4_0`.
fn slug(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut slug = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let digit_at = |j: Option<usize>| {
            j.and_then(|j| chars.get(j)).is_some_and(char::is_ascii_digit)
        };
        let between_digits = c == '_'
            && digit_at(i.checked_sub(1))
            && digit_at(i.checked_add(1));
        if c.is_alphanumeric() || between_digits || c == '.' {
            slug.push(if between_digits { '.' } else { c });
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.to_lowercase().trim_matches(['-', '.']).to_owned()
}

fn from_uri(address: &str) -> Option<String> {
    let address = address.strip_prefix("www.").unwrap_or(address);
    let (host, path) = address.split_once('/')?;
    let path = path.trim_end_matches('/');
    match host {
        "spdx.org" => {
            let id = path.strip_prefix("licenses/")?;
            let id = id.trim_end_matches(".html").trim_end_matches(".json");
            (!id.is_empty())
                .then(|| from_name(&slug(id)).unwrap_or_else(|| id.to_owned()))
        }
        "creativecommons.org" => creative_commons_deed(path),
        "opendatacommons.org" | "opendefinition.org" => {
            let name = path.strip_prefix("licenses/")?.split('/').next()?;
            from_name(if name == "by" { "odc-by" } else { name })
        }
        "nationalarchives.gov.uk" => {
            let version =
                path.strip_prefix("doc/open-government-licence/version/")?;
            matches!(version, "1" | "2" | "3")
                .then(|| format!("OGL-UK-{version}.0"))
        }
        "reference.data.gov.uk" => (path == "id/open-government-licence")
            .then(|| "OGL-UK-3.0".to_owned()),
        "dcat-ap.de" => from_name(&slug(path.strip_prefix("def/licenses/")?)),
        "publications.europa.eu" => {
            from_name(&slug(path.strip_prefix("resource/authority/licence/")?))
        }
        "etalab.gouv.fr" => path
            .starts_with("licence-ouverte")
            .then(|| "etalab-2.0".to_owned()),
        _ => None,
    }
}

/// A Creative Commons deed or legal code: `licenses/by-sa/4.0/`,
/// `licenses/by/3.0/at/deed.de` (a national port, `CC-BY-3.0-AT`) or
/// `publicdomain/zero/1.0/legalcode`.
fn creative_commons_deed(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let (kind, code, version) = (parts.next()?, parts.next()?, parts.next()?);
    let version = cc_version(version)?;
    let port = parts.next().and_then(cc_port);
    match (kind, code) {
        ("publicdomain", "zero") => Some(format!("CC0-{version}")),
        (
            "licenses",
            "by" | "by-sa" | "by-nd" | "by-nc" | "by-nc-sa" | "by-nc-nd",
        ) => {
            let id = format!("CC-{}-{version}", code.to_ascii_uppercase());
            Some(match port {
                Some(port) => format!("{id}-{port}"),
                None => id,
            })
        }
        _ => None,
    }
}

fn from_name(slug: &str) -> Option<String> {
    if let Some((_, id)) = NAMES.iter().find(|(name, _)| *name == slug) {
        return Some((*id).to_owned());
    }
    let words: Vec<&str> = slug.split('-').collect();
    creative_commons(&words).or_else(|| gnu(&words)).or_else(|| ogl(&words))
}

/// A Creative Commons license from the words of its name or slug, in SPDX
/// order (`CC-BY-NC-SA`), with its version when one is named and a
/// national port after it.
fn creative_commons(words: &[&str]) -> Option<String> {
    let has = |word: &str| words.contains(&word);
    let named = has("cc")
        || has("cc0")
        || (has("creative") && has("commons"))
        || words.first() == Some(&"attribution");
    if !named {
        return None;
    }
    if has("cc0")
        || has("zero")
        || words == ["cc", "0"]
        || (has("public") && has("domain") && has("dedication"))
    {
        return Some("CC0-1.0".to_owned());
    }
    let noncommercial =
        has("nc") || has("noncommercial") || (has("non") && has("commercial"));
    let no_derivatives = has("nd")
        || has("noderivatives")
        || has("noderivs")
        || (has("no") && has("derivatives"));
    let share_alike =
        has("sa") || has("sharealike") || (has("share") && has("alike"));
    if !(has("by")
        || has("attribution")
        || noncommercial
        || no_derivatives
        || share_alike)
    {
        return None;
    }
    let mut id = String::from("CC-BY");
    if noncommercial {
        id.push_str("-NC");
    }
    if no_derivatives {
        id.push_str("-ND");
    } else if share_alike {
        id.push_str("-SA");
    }
    if let Some(version) = words.iter().find_map(|w| cc_version(w)) {
        id = format!("{id}-{version}");
        if let Some(port) = words.iter().find_map(|w| cc_port(w)) {
            id = format!("{id}-{port}");
        }
    }
    Some(id)
}

fn cc_version(word: &str) -> Option<&'static str> {
    match word {
        "1" | "1.0" => Some("1.0"),
        "2" | "2.0" => Some("2.0"),
        "2.5" => Some("2.5"),
        "3" | "3.0" => Some("3.0"),
        "4" | "4.0" => Some("4.0"),
        _ => None,
    }
}

fn cc_port(word: &str) -> Option<&'static str> {
    match word {
        "at" | "austria" => Some("AT"),
        "au" | "australia" => Some("AU"),
        "de" | "germany" => Some("DE"),
        "nl" | "netherlands" => Some("NL"),
        "igo" => Some("IGO"),
        _ => None,
    }
}

/// The GNU licenses, `-only` unless the name says "or later".
fn gnu(words: &[&str]) -> Option<String> {
    let has = |word: &str| words.contains(&word);
    let kind = if has("agpl") || has("affero") {
        "AGPL"
    } else if has("lgpl") || has("lesser") {
        "LGPL"
    } else if has("gpl") || (has("gnu") && has("general") && has("public")) {
        "GPL"
    } else {
        return None;
    };
    let version = words.iter().find_map(|w| match *w {
        "2" | "2.0" => Some("2.0"),
        "2.1" => Some("2.1"),
        "3" | "3.0" => Some("3.0"),
        _ => None,
    })?;
    let scope = if has("later") { "or-later" } else { "only" };
    Some(format!("{kind}-{version}-{scope}"))
}

/// The UK's and Canada's Open Government Licences. A provincial or
/// territorial one ("Nova Scotia Open Government Licence") has no SPDX id,
/// so a word outside the licence's own name rules the match out.
fn ogl(words: &[&str]) -> Option<String> {
    let has = |word: &str| words.contains(&word);
    let licence = has("licence") || has("license");
    if !(has("ogl") || (has("open") && has("government") && licence)) {
        return None;
    }
    let version = |w: &str| match w.strip_prefix('v').unwrap_or(w) {
        "1" | "1.0" => Some('1'),
        "2" | "2.0" => Some('2'),
        "3" | "3.0" => Some('3'),
        _ => None,
    };
    let own = [
        "open",
        "government",
        "licence",
        "license",
        "ogl",
        "uk",
        "united",
        "kingdom",
        "the",
        "v",
        "version",
        "canada",
    ];
    if words.iter().any(|w| !own.contains(w) && version(w).is_none()) {
        return None;
    }
    let named = words.iter().find_map(|w| version(w));
    if has("canada") {
        return Some("OGL-Canada-2.0".to_owned());
    }
    Some(format!("OGL-UK-{}.0", named.unwrap_or('3')))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spellings from six searches over every source, October 2026.
    #[test]
    fn spellings_sources_send_become_one_id() {
        for (raw, id) in [
            ("cc-by-4.0", "CC-BY-4.0"),
            ("cc-by-4", "CC-BY-4.0"),
            ("CC BY 3", "CC-BY-3.0"),
            ("Creative Commons Attribution 4.0 International", "CC-BY-4.0"),
            ("Attribution 4.0 International (CC BY 4.0)", "CC-BY-4.0"),
            (
                "Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)",
                "CC-BY-NC-4.0",
            ),
            (
                "CC-By Attribution-NonCommercial-NoDerivatives 4.0 International",
                "CC-BY-NC-ND-4.0",
            ),
            (
                "Creative Commons Attribution Share-Alike (CC BY-SA)",
                "CC-BY-SA",
            ),
            ("Creative Commons Attribution International (CC BY)", "CC-BY"),
            ("CC BY NC ND", "CC-BY-NC-ND"),
            ("Creative Commons Attribution 2.5 Australia", "CC-BY-2.5-AU"),
            ("CC BY 3.0 IGO", "CC-BY-3.0-IGO"),
            (
                "Creative Commons Attribution 4.0 International (Embargo: 2020-07-31)",
                "CC-BY-4.0",
            ),
            ("CC0: Public Domain", "CC0-1.0"),
            ("cc-zero", "CC0-1.0"),
            ("CC 0", "CC0-1.0"),
            ("[CC0 1.0]", "CC0-1.0"),
            ("Public Domain / No restrictions (CC0)", "CC0-1.0"),
            (
                "Creative Commons 1.0 Universal (Public Domain Dedication)",
                "CC0-1.0",
            ),
            ("Open Database License (ODC-ODbL)", "ODbL-1.0"),
            ("odc-odbl", "ODbL-1.0"),
            ("Open Data Commons Attribution License", "ODC-By-1.0"),
            ("ODC Public Domain Dedication and Licence (PDDL)", "PDDL-1.0"),
            ("UK Open Government Licence (OGL)", "OGL-UK-3.0"),
            ("uk-ogl", "OGL-UK-3.0"),
            ("Open Government Licence V 3.0", "OGL-UK-3.0"),
            ("Open Government Licence v2", "OGL-UK-2.0"),
            ("Open Government Licence - Canada", "OGL-Canada-2.0"),
            ("ogl-canada-2.0", "OGL-Canada-2.0"),
            ("etalab 2.0", "etalab-2.0"),
            ("Apache License 2.0", "Apache-2.0"),
            ("Apache 2.0", "Apache-2.0"),
            ("BSD 2-Clause \"Simplified\" License", "BSD-2-Clause"),
            ("GNU General Public License (GPL) 3.0", "GPL-3.0-only"),
            ("GPL 2", "GPL-2.0-only"),
            ("agpl-3.0", "AGPL-3.0-only"),
            ("GNU Lesser General Public License (LGPL) 2.1", "LGPL-2.1-only"),
            (
                "Community Data License Agreement - Sharing - Version 1.0",
                "CDLA-Sharing-1.0",
            ),
            ("mit", "MIT"),
        ] {
            assert_eq!(normalized(raw).as_deref(), Some(id), "{raw}");
        }
    }

    #[test]
    fn license_uris_become_ids() {
        for (uri, id) in [
            ("https://creativecommons.org/publicdomain/zero/1.0/", "CC0-1.0"),
            (
                "http://creativecommons.org/licenses/by-nc/4.0/legalcode",
                "CC-BY-NC-4.0",
            ),
            (
                "https://creativecommons.org/licenses/by-nc-sa/4.0/deed.en",
                "CC-BY-NC-SA-4.0",
            ),
            (
                "https://creativecommons.org/licenses/by/3.0/at/deed.de",
                "CC-BY-3.0-AT",
            ),
            ("https://spdx.org/licenses/etalab-2.0.html", "etalab-2.0"),
            ("http://spdx.org/licenses/CC-BY-4.0", "CC-BY-4.0"),
            ("https://opendatacommons.org/licenses/odbl/summary", "ODbL-1.0"),
            ("https://opendatacommons.org/licenses/pddl/summary", "PDDL-1.0"),
            ("http://opendefinition.org/licenses/odc-odbl/", "ODbL-1.0"),
            ("http://www.opendefinition.org/licenses/cc-by", "CC-BY"),
            (
                "http://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/",
                "OGL-UK-3.0",
            ),
            (
                "http://reference.data.gov.uk/id/open-government-licence",
                "OGL-UK-3.0",
            ),
            ("http://dcat-ap.de/def/licenses/cc-by/4.0", "CC-BY-4.0"),
            ("http://dcat-ap.de/def/licenses/cc-by-de/3.0", "CC-BY-3.0-DE"),
            ("http://dcat-ap.de/def/licenses/dl-by-de/2.0", "DL-DE-BY-2.0"),
            (
                "http://dcat-ap.de/def/licenses/dl-zero-de/2.0",
                "DL-DE-ZERO-2.0",
            ),
            ("http://dcat-ap.de/def/licenses/odcpddl", "PDDL-1.0"),
            (
                "http://publications.europa.eu/resource/authority/licence/CC_BY_2_5",
                "CC-BY-2.5",
            ),
            (
                "https://www.etalab.gouv.fr/licence-ouverte-open-licence",
                "etalab-2.0",
            ),
        ] {
            assert_eq!(spdx(uri).as_deref(), Some(id), "{uri}");
        }
    }

    #[test]
    fn what_is_not_recognized_is_kept_and_placeholders_are_dropped() {
        for kept in [
            "Public Domain",
            "Nova Scotia Open Government Licence",
            "Open Government Licence - British Columbia",
            "Database: Open Database, Contents: Database Contents",
            "http://creativecommons.org/publicdomain/mark/1.0/deed.nl",
            "http://dcat-ap.de/def/licenses/other-closed",
            "https://www.nist.gov/open/license",
            "Creative Commons",
            "other-open",
        ] {
            assert_eq!(normalized(kept).as_deref(), Some(kept), "{kept}");
        }
        for placeholder in [
            "other",
            "Other",
            "unknown",
            "notspecified",
            "unspecified",
            "No license",
            "Other (specified in description)",
            " ",
        ] {
            assert_eq!(normalized(placeholder), None, "{placeholder}");
        }
    }

    #[test]
    fn an_id_normalizes_to_itself() {
        let ids = NAMES.iter().map(|(_, id)| *id).chain([
            "CC-BY-4.0",
            "CC-BY-NC-SA-3.0",
            "CC-BY-3.0-AU",
            "CC-BY",
            "CC0-1.0",
            "GPL-3.0-only",
            "LGPL-2.1-or-later",
            "OGL-UK-3.0",
            "OGL-Canada-2.0",
        ]);
        for id in ids {
            assert_eq!(normalized(id).as_deref(), Some(id), "{id}");
        }
    }

    #[test]
    fn a_family_matches_every_version_of_it_and_nothing_else() {
        assert_eq!(family("CC-BY-4.0"), "CC-BY");
        assert_eq!(family("CC-BY-3.0-AU"), "CC-BY");
        assert_eq!(family("CC0-1.0"), "CC0");
        assert_eq!(family("GPL-3.0-only"), "GPL");
        assert_eq!(family("MIT"), "MIT");
        assert!(matches("CC-BY-4.0", "cc-by"));
        assert!(matches("CC-BY-4.0", "CC-BY-4.0"));
        assert!(!matches("CC-BY-SA-4.0", "CC-BY"));
        assert!(!matches("CC-BY-3.0", "CC-BY-4.0"));
        assert!(matches("Public Domain", "public domain"));
    }
}
