//! Materials Project contributed datasets (MPContribs projects), listed
//! keyless and searched locally. The core Materials Project API is a
//! per-material database rather than a dataset catalog, so it is not used.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://contribs-api.materialsproject.org/projects/")
        .query("_fields", "name,title,description")
        .query("_limit", 500)
        .slow()
        .json()?;
    Ok(parse(&body))
}

pub(super) fn parse(body: &Value) -> Vec<Dataset> {
    items(body, "/data")
        .iter()
        .filter_map(|row| {
            let name = text(row, "/name")?;
            let title = text(row, "/title").unwrap_or_else(|| name.clone());
            Dataset::new(
                &title,
                &format!(
                    "https://contribs.materialsproject.org/projects/{name}"
                ),
            )
            .describe(text(row, "/description"))
            .valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let entries = parse(&fixture::json("materials.json"));
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries[0],
            Dataset {
                title: "Carrier Transport".into(),
                url: "https://contribs.materialsproject.org/projects/\
                      carrier_transport"
                    .into(),
                description: Some(
                    "Ab-initio electronic transport database for inorganic \
                     materials."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
