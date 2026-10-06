//! CKAN's `package_search`, the action API shared by hundreds of open-data
//! portals. Each [`Portal`] pairs the API root with the public page prefix,
//! because several portals serve the API from a different host than their
//! website (data.gov.uk runs it on ckan.publishing.service.gov.uk).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub struct Portal {
    pub api: &'static str,
    pub page: &'static str,
}

pub static DATA_GOV_UK: Portal = Portal {
    api: "https://ckan.publishing.service.gov.uk/api/action",
    page: "https://www.data.gov.uk/dataset/",
};
pub static OPEN_CANADA: Portal = Portal {
    api: "https://open.canada.ca/data/api/action",
    page: "https://open.canada.ca/data/en/dataset/",
};
pub static DATA_GOV_AU: Portal = Portal {
    api: "https://data.gov.au/data/api/3/action",
    page: "https://data.gov.au/data/dataset/",
};
pub static GOVDATA: Portal = Portal {
    api: "https://ckan.govdata.de/api/3/action",
    page: "https://www.govdata.de/suche/daten/",
};
pub static HDX: Portal = Portal {
    api: "https://data.humdata.org/api/3/action",
    page: "https://data.humdata.org/dataset/",
};
pub static B2FIND: Portal = Portal {
    api: "https://b2find.eudat.eu/api/3/action",
    page: "https://b2find.eudat.eu/dataset/",
};

pub fn search(
    ctx: &Ctx<'_>,
    portal: &Portal,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{}/package_search", portal.api))
        .query("q", query)
        .query("rows", limit)
        .json()?;
    if body.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(SourceError::shape("CKAN did not report success"));
    }
    Ok(items(&body, "/result/results")
        .iter()
        .filter_map(|row| record(portal, row))
        .take(limit)
        .collect())
}

fn record(portal: &Portal, row: &Value) -> Option<Dataset> {
    let name = text(row, "/name")?;
    let title = text(row, "/title").unwrap_or_else(|| name.clone());
    let extra = |key: &str| {
        items(row, "/extras").iter().find_map(|e| {
            (e.get("key").and_then(Value::as_str) == Some(key))
                .then(|| text(e, "/value"))
                .flatten()
        })
    };
    let mut dataset = Dataset::new(&title, &format!("{}{name}", portal.page))
        .describe(text(row, "/notes"))
        .doi_from(extra("DOI"));
    dataset.publisher = text(row, "/organization/title")
        .or_else(|| extra("Publisher"))
        .or_else(|| text(row, "/author"));
    dataset.license = text(row, "/license_title");
    dataset.updated = day(text(row, "/metadata_modified"));
    if let Some(source_page) = text(row, "/url") {
        dataset.aliases.push(source_page);
    }
    dataset.valid()
}
