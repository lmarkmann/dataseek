//! `inspect`: the machine-readable metadata a dataset page publishes about
//! itself. Hugging Face pages carry none in their HTML, so for them the
//! Hub's Croissant endpoint is read instead; every other page is searched for
//! `application/ld+json` blocks holding a schema.org `Dataset` (top level,
//! in an array, or inside `@graph`). This is enrichment for one result, not
//! search: it never fans out.

use std::io::Write;

use anyhow::Result;
use serde_json::Value;

use crate::http::{Http, SourceError};
use crate::output::Out;
use crate::record::{clean, summary, text};
use crate::ui;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "{url} carries no schema.org Dataset markup\n  Try:   inspect the repository's landing page rather than a file or search page"
    )]
    NoMarkup { url: String },
    #[error(
        "cannot reach {url}; this machine looks offline\n  Try:   check the connection, then run it again"
    )]
    Unreachable {
        url: String,
        #[source]
        source: SourceError,
    },
    #[error(
        "inspect reads the page itself, so it needs the network\n  Try:   run it again without --offline"
    )]
    Offline,
    #[error(
        "cannot fetch {url}\n  Try:   open it in a browser to check that it is a dataset page"
    )]
    Fetch {
        url: String,
        #[source]
        source: SourceError,
    },
}

pub fn run(url: &str, out: &Out) -> Result<()> {
    let http = Http::new();
    let fetch_error = |e: SourceError| match e {
        SourceError::Offline => Error::Offline,
        SourceError::Unreachable(_) => {
            Error::Unreachable { url: url.to_owned(), source: e }
        }
        _ => Error::Fetch { url: url.to_owned(), source: e },
    };
    let progress = ui::spinner(format!("reading {url}"));
    let found = if let Some(id) = huggingface_id(url) {
        http.get(&format!(
            "https://huggingface.co/api/datasets/{id}/croissant"
        ))
        .json()
        .map_err(fetch_error)
    } else {
        http.get(url).text().map_err(fetch_error).and_then(|page| {
            json_ld(&page)
                .into_iter()
                .find_map(find_dataset)
                .ok_or_else(|| Error::NoMarkup { url: url.to_owned() })
        })
    };
    progress.finish_and_clear();
    let dataset = found?;
    print(out, url, &dataset)
}

fn huggingface_id(url: &str) -> Option<String> {
    let path = url.split_once("huggingface.co/datasets/")?.1;
    let id: Vec<&str> = path.split(['/', '?', '#']).take(2).collect();
    (id.len() == 2 && id.iter().all(|p| !p.is_empty())).then(|| id.join("/"))
}

/// Every parseable `<script type="application/ld+json">` block on the page.
fn json_ld(page: &str) -> Vec<Value> {
    page.split("application/ld+json")
        .skip(1)
        .filter_map(|tail| {
            let body = tail.split_once('>')?.1.split_once("</script>")?.0;
            serde_json::from_str(body).ok()
        })
        .collect()
}

fn find_dataset(value: Value) -> Option<Value> {
    match value {
        Value::Array(items) => items.into_iter().find_map(find_dataset),
        Value::Object(ref map) => {
            if is_dataset(map.get("@type")) {
                return Some(value);
            }
            map.get("@graph").cloned().and_then(find_dataset)
        }
        _ => None,
    }
}

fn is_dataset(kind: Option<&Value>) -> bool {
    match kind {
        Some(Value::String(t)) => t == "Dataset" || t.ends_with(":Dataset"),
        Some(Value::Array(ts)) => ts.iter().any(|t| is_dataset(Some(t))),
        _ => false,
    }
}

fn print(out: &Out, page: &str, dataset: &Value) -> Result<()> {
    if out.json {
        return out.json(&serde_json::json!({
            "schema": "dataseek-inspect/1",
            "url": page,
            "dataset": dataset,
        }));
    }
    let mut w = out.stdout();
    let field = |label: &str, value: Option<String>| -> std::io::Result<()> {
        match value {
            Some(v) if !v.is_empty() => {
                writeln!(out.stdout(), "{label:<12} {v}")
            }
            _ => Ok(()),
        }
    };
    field("name", text(dataset, "/name"))?;
    field("url", text(dataset, "/url").map(|u| out.link(&u)))?;
    field("identifier", names(dataset.get("identifier")))?;
    field("license", names(dataset.get("license")))?;
    field("creator", names(dataset.get("creator")))?;
    field("publisher", names(dataset.get("publisher")))?;
    field("modified", text(dataset, "/dateModified"))?;
    field("published", text(dataset, "/datePublished"))?;
    field("keywords", names(dataset.get("keywords")))?;
    field("temporal", text(dataset, "/temporalCoverage"))?;
    field("spatial", names(dataset.get("spatialCoverage")))?;
    field(
        "description",
        text(dataset, "/description").as_deref().and_then(summary),
    )?;
    let files: Vec<String> = ["distribution", "distributions"]
        .iter()
        .filter_map(|k| dataset.get(*k))
        .flat_map(|d| match d {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        })
        .filter_map(|file| {
            let link =
                text(&file, "/contentUrl").or_else(|| text(&file, "/url"))?;
            let format = text(&file, "/encodingFormat").unwrap_or_default();
            Some(format!("{format} {}", out.link(&link)).trim().to_owned())
        })
        .collect();
    for (i, file) in files.iter().enumerate() {
        let label = if i == 0 { "files" } else { "" };
        writeln!(w, "{label:<12} {file}")?;
    }
    Ok(())
}

/// A readable rendering of a schema.org value that may be a string, an
/// object with a `name`/`@id`/`url`, or a list of either.
fn names(value: Option<&Value>) -> Option<String> {
    let rendered = match value? {
        Value::String(s) => clean(s),
        Value::Array(items) => items
            .iter()
            .filter_map(|v| names(Some(v)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => {
            let v = value?;
            text(v, "/name")
                .or_else(|| text(v, "/value"))
                .or_else(|| text(v, "/@id"))
                .or_else(|| text(v, "/url"))?
        }
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!rendered.is_empty()).then_some(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_dataset_is_found_inside_a_graph() {
        let page = r#"<script type="application/ld+json">{"@type":"WebSite"}</script>
            <script type="application/ld+json">
            {"@graph":[{"@type":"Organization"},{"@type":["Thing","Dataset"],"name":"Rain"}]}
            </script>"#;
        let found = json_ld(page).into_iter().find_map(find_dataset).unwrap();
        assert_eq!(found["name"], "Rain");
    }

    #[test]
    fn huggingface_ids_come_from_dataset_urls() {
        assert_eq!(
            huggingface_id(
                "https://huggingface.co/datasets/stanfordnlp/imdb/tree/main"
            )
            .as_deref(),
            Some("stanfordnlp/imdb")
        );
        assert_eq!(huggingface_id("https://huggingface.co/models/x"), None);
    }

    #[test]
    fn names_flatten_people_and_lists() {
        let v = json!([{"@type":"Person","name":"Ada"}, "Grace"]);
        assert_eq!(names(Some(&v)).as_deref(), Some("Ada, Grace"));
    }
}
