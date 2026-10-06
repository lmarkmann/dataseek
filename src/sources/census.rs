//! The U.S. Census Bureau's API datasets (ACS, decennial census, economic
//! census, CPS, ...), listed from the bureau's DCAT `data.json` and searched
//! locally. Each dataset's link is its variables page on the API host.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body =
        ctx.http.get("https://api.census.gov/data.json").slow().json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = items(body, "/dataset");
    if rows.is_empty() {
        return Err(SourceError::shape("no dataset list in data.json"));
    }
    Ok(rows
        .iter()
        .filter_map(|row| {
            let endpoint = text(row, "/distribution/0/accessURL")?
                .replacen("http://", "https://", 1);
            let title = match (text(row, "/title"), number(row, "/c_vintage"))
            {
                (Some(t), Some(year)) if !t.contains(&year.to_string()) => {
                    format!("{t} ({year})")
                }
                (t, _) => t?,
            };
            let mut dataset =
                Dataset::new(&title, &format!("{endpoint}.html"))
                    .describe(text(row, "/description"));
            dataset.publisher = Some("U.S. Census Bureau".to_owned());
            dataset.license = text(row, "/license");
            dataset.updated = day(text(row, "/modified"));
            dataset.valid()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let datasets = parse(&fixture::json("census.json")).unwrap();
        assert_eq!(datasets.len(), 4);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Jun 1994 Current Population Survey: Basic Monthly"
                    .into(),
                url: "https://api.census.gov/data/1994/cps/basic/jun.html"
                    .into(),
                description: Some(
                    "To provide estimates of employment, unemployment, and \
                     other characteristics of the general labor force, of \
                     the population as a whole, and of various subgroups of \
                     the population."
                        .into()
                ),
                publisher: Some("U.S. Census Bureau".into()),
                doi: None,
                license: Some(
                    "https://creativecommons.org/publicdomain/zero/1.0/"
                        .into()
                ),
                updated: Some("2019-10-09".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
