//! Materials Project contributed datasets (MPContribs projects), listed
//! keyless and searched locally. The core Materials Project API is a
//! per-material database rather than a dataset catalog, so it is not used.
//!
//! One page holds up to 500 projects and the whole list is 72, so one request
//! gets everything (MPContribs, October 2026). `owner` is a personal e-mail
//! address and is never requested. `stats.size` is no longer computed by the
//! server and reads 0.0 for 28 of the 72 projects, so no size is read
//! (MPContribs, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://contribs-api.materialsproject.org/projects/")
        .query("_fields", "name,title,long_title,description,license")
        .query("_limit", 500)
        .slow()
        .json()?;
    Ok(parse(&body))
}

pub(super) fn parse(body: &Value) -> Vec<Dataset> {
    items(body, "/data").iter().filter_map(record).collect()
}

fn record(row: &Value) -> Option<Dataset> {
    let name = text(row, "/name")?;
    let short = text(row, "/title").unwrap_or_else(|| name.clone());
    let title = text(row, "/long_title").unwrap_or_else(|| short.clone());
    let description = match text(row, "/description") {
        Some(description) if title != short => {
            Some(format!("{short}. {description}"))
        }
        None if title != short => Some(short),
        description => description,
    };
    let mut dataset = Dataset::new(
        &title,
        &format!("https://contribs.materialsproject.org/projects/{name}"),
    )
    .describe(description);
    dataset.publisher = Some("Materials Project".to_owned());
    dataset.license = match text(row, "/license").as_deref() {
        Some("CCA4") => Some("CC-BY-4.0".to_owned()),
        _ => None,
    };
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let entries = parse(&fixture::json("materials.json"));
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[0],
            Dataset {
                title: "Electronic Transport Properties".into(),
                url: "https://contribs.materialsproject.org/projects/\
                      carrier_transport"
                    .into(),
                description: Some(
                    "Carrier Transport. Ab-initio electronic transport \
                     database for inorganic materials. Complex multivariable \
                     BoltzTraP simulation data is condensed down into \
                     tabular form of two main motifs."
                        .into()
                ),
                publisher: Some("Materials Project".into()),
                doi: None,
                license: Some("CC-BY-4.0".into()),
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_project_without_a_long_title_keeps_its_title() {
        let entries = parse(&fixture::json("materials.json"));
        assert_eq!(entries[4].title, "PyCroscopy");
        assert_eq!(
            entries[4].description.as_deref(),
            Some("Scientific Analysis of nanoscience Data")
        );
    }

    #[test]
    fn a_project_is_still_found_by_its_short_title() {
        let entries = parse(&fixture::json("materials.json"));
        let found = crate::catalog::search(&entries, "esters", 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Improved c-axis parameter for BiSe");
    }

    #[test]
    fn a_license_code_without_a_known_spdx_id_is_left_out() {
        let row = json!({
            "name": "open_data",
            "title": "Open Data",
            "license": "CCPD",
        });
        assert_eq!(record(&row).unwrap().license, None);
    }
}
