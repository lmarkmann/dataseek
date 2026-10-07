//! `inspect`: the machine-readable metadata a dataset page publishes about
//! itself, and the files it lists. Hugging Face pages carry none in their
//! HTML, so for them the Hub's Croissant endpoint is read instead; every other
//! page is searched for `application/ld+json` blocks holding a schema.org
//! `Dataset` (top level, in an array, or inside `@graph`). The file list is
//! the dataset's `distribution`: schema.org `DataDownload`s and Croissant
//! `FileObject`s and `FileSet`s, with whatever name, format, size, checksum
//! and link each carries; nothing is downloaded. This is enrichment for one
//! result, not search: it never fans out.

use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::find::human_bytes;
use crate::http::{Http, SourceError};
use crate::output::Out;
use crate::record::{clean, first_text, number, summary, text};
use crate::ui;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "{url} carries no schema.org Dataset markup\n  Try:   inspect the repository's landing page rather than a file or search page"
    )]
    NoMarkup { url: String },
    #[error(
        "cannot reach {url}\n  Try:   check the address and the connection, then run it again"
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
    let files = files(&dataset);
    if files.is_empty() {
        ui::warn("the page's metadata lists no files");
    }
    print(out, url, dataset, &files)
}

/// One entry of a dataset's file list, as the page's metadata describes it.
/// A Croissant file set has no link of its own: it names the files matching
/// `includes` inside another entry, usually the repository.
#[derive(Debug, Default, PartialEq, Serialize)]
struct File {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    /// `sha256:<hex>` or `md5:<hex>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    includes: Option<String>,
}

fn files(dataset: &Value) -> Vec<File> {
    ["distribution", "distributions"]
        .iter()
        .filter_map(|k| dataset.get(*k))
        .flat_map(|d| match d {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        })
        .map(|file| File {
            name: text(file, "/name"),
            format: names(
                file.get("encodingFormat").or_else(|| file.get("fileFormat")),
            ),
            size_bytes: number(file, "/contentSize").or_else(|| {
                text(file, "/contentSize").as_deref().and_then(with_unit)
            }),
            checksum: checksum(file),
            url: first_text(file, &["/contentUrl", "/url"]),
            includes: names(file.get("includes")),
        })
        .filter(|file| *file != File::default())
        .collect()
}

/// A size written with its unit, such as Zenodo's "8.19 MB": decimal units
/// up to TB, binary ones up to TiB. `None` for anything else, rather than a
/// guess.
fn with_unit(size: &str) -> Option<u64> {
    let unit_at = size.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (value, unit) = size.split_at(unit_at);
    let scale: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "b" | "bytes" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1_048_576.0,
        "gib" => 1_073_741_824.0,
        "tib" => 1_099_511_627_776.0,
        _ => return None,
    };
    let bytes = value.parse::<f64>().ok()? * scale;
    (bytes.is_finite() && bytes >= 0.0).then(|| whole_bytes(bytes))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the caller passes a finite, non-negative value; casts saturate"
)]
fn whole_bytes(bytes: f64) -> u64 {
    bytes.round() as u64
}

/// The file's checksum when it is one: sha256 or md5 as hex of the right
/// length. Hugging Face fills `sha256` with a link to an open issue instead.
fn checksum(file: &Value) -> Option<String> {
    [("sha256", 64), ("md5", 32)].into_iter().find_map(
        |(algorithm, digits)| {
            let hex = text(file, &format!("/{algorithm}"))?;
            (hex.len() == digits && hex.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| format!("{algorithm}:{}", hex.to_ascii_lowercase()))
        },
    )
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

/// `Dataset`, prefixed (`schema:Dataset`) or as the full address Zenodo
/// writes (`https://schema.org/Dataset`).
fn is_dataset(kind: Option<&Value>) -> bool {
    match kind {
        Some(Value::String(t)) => {
            t == "Dataset"
                || t.ends_with(":Dataset")
                || t.ends_with("schema.org/Dataset")
        }
        Some(Value::Array(ts)) => ts.iter().any(|t| is_dataset(Some(t))),
        _ => false,
    }
}

fn print(
    out: &Out,
    page: &str,
    mut dataset: Value,
    files: &[File],
) -> Result<()> {
    if out.json {
        if let Value::Object(fields) = &mut dataset {
            fields.insert("files".to_owned(), serde_json::json!(files));
        }
        return out.json(&serde_json::json!({
            "schema": "dataseek-inspect/1",
            "url": page,
            "dataset": dataset,
        }));
    }
    let dataset = &dataset;
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
    let mut w = out.stdout();
    for (i, file) in files.iter().enumerate() {
        let label = if i == 0 { "files" } else { "" };
        writeln!(w, "{label:<12} {}", listing(out, file))?;
    }
    Ok(())
}

/// One file on one line: name, format, size, checksum and pattern, whichever
/// are known, then the link.
fn listing(out: &Out, file: &File) -> String {
    [
        file.name.clone(),
        file.format.clone(),
        file.size_bytes.map(human_bytes),
        file.checksum.clone(),
        file.includes.clone(),
        file.url.as_deref().map(|url| out.link(url)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("  ")
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
    use crate::sources::fixture;
    use serde_json::json;

    #[test]
    fn a_zenodo_page_names_its_dataset_by_full_address() {
        let page = fixture::read("inspect", "zenodo.html");
        let found = json_ld(&page).into_iter().find_map(find_dataset).unwrap();
        assert_eq!(found["@type"], "https://schema.org/Dataset");
        assert_eq!(
            found["name"],
            "Evaluation of the influence of rain on air surface temperature measurements"
        );
    }

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

    #[test]
    fn zenodo_lists_each_file_by_format_and_link() {
        let page = fixture::read("inspect", "zenodo.html");
        let dataset =
            json_ld(&page).into_iter().find_map(find_dataset).unwrap();
        let files = files(&dataset);
        assert_eq!(files.len(), 3);
        assert_eq!(
            files[0],
            File {
                format: Some("text/csv".into()),
                url: Some("https://zenodo.org/api/records/13135140/files/Dataset_reference.csv/content".into()),
                ..File::default()
            }
        );
    }

    #[test]
    fn dataverse_lists_names_sizes_and_formats() {
        let dataset: Value = serde_json::from_str(&fixture::read(
            "inspect",
            "dataverse-schema-org.json",
        ))
        .unwrap();
        let files = files(&dataset);
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0],
            File {
                name: Some("RF_dry_transposed.tab".into()),
                format: Some("text/tab-separated-values".into()),
                size_bytes: Some(748_311),
                url: Some("https://dataverse.harvard.edu/api/access/datafile/4288346".into()),
                ..File::default()
            }
        );
    }

    #[test]
    fn croissant_file_objects_carry_their_checksums() {
        let dataset: Value = serde_json::from_str(&fixture::read(
            "inspect",
            "dataverse-croissant.json",
        ))
        .unwrap();
        assert_eq!(
            files(&dataset)[1],
            File {
                name: Some("RF_wet_transposed.xlsx.csv".into()),
                format: Some("text/csv".into()),
                size_bytes: Some(1_140_008),
                checksum: Some("md5:921e382a0751e5c6d77f0cb30069c3d0".into()),
                url: Some("https://dataverse.harvard.edu/api/access/datafile/4288345?format=original".into()),
                ..File::default()
            }
        );
    }

    #[test]
    fn a_croissant_file_set_names_its_pattern_not_a_link() {
        let dataset: Value = serde_json::from_str(&fixture::read(
            "inspect",
            "huggingface-croissant.json",
        ))
        .unwrap();
        assert_eq!(
            files(&dataset),
            [
                File {
                    name: Some("repo".into()),
                    format: Some("git+https".into()),
                    url: Some("https://huggingface.co/datasets/stanfordnlp/imdb/tree/refs%2Fconvert%2Fparquet".into()),
                    ..File::default()
                },
                File {
                    name: Some("parquet-files-for-config-plain_text".into()),
                    format: Some("application/x-parquet".into()),
                    includes: Some("plain_text/*/*.parquet".into()),
                    ..File::default()
                },
            ]
        );
    }

    #[test]
    fn sizes_are_read_with_their_units_or_not_at_all() {
        assert_eq!(with_unit("8.19 MB"), Some(8_190_000));
        assert_eq!(with_unit("1.5 GiB"), Some(1_610_612_736));
        assert_eq!(with_unit("12 bytes"), Some(12));
        assert_eq!(with_unit("2kB"), Some(2_000));
        assert_eq!(with_unit("3 parsecs"), None);
        assert_eq!(with_unit("about 2 MB"), None);
        assert_eq!(with_unit("1,024 B"), None);
    }

    #[test]
    fn a_checksum_must_be_hex_of_its_length() {
        let sha256 = "A".repeat(64);
        let file = json!({"sha256": sha256, "md5": "0".repeat(32)});
        assert_eq!(
            checksum(&file),
            Some(format!("sha256:{}", "a".repeat(64)))
        );
        let file = json!({"sha256": "main", "md5": "z".repeat(32)});
        assert_eq!(checksum(&file), None);
    }
}
