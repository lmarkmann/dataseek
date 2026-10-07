//! `inspect`: the machine-readable metadata a dataset page publishes about
//! itself, and the files it lists. Hugging Face pages carry none in their
//! HTML, so for them the Hub's Croissant endpoint is read instead; every other
//! page is searched for `application/ld+json` blocks holding a schema.org
//! `Dataset` (top level, in an array, or inside `@graph`, whose other nodes
//! answer `{"@id": ...}` references; keys may carry a `schema:` prefix, as
//! ckanext-dcat writes them on CKAN portals such as HDX). The file list is
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
use crate::record::{clean, number, summary, text};
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
    #[error("cannot verify the certificate of {url}\n  Try:   {hint}")]
    Certificate {
        url: String,
        /// From [`crate::http::certificate_hint`].
        hint: String,
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
        SourceError::Certificate(_) => Error::Certificate {
            url: url.to_owned(),
            hint: crate::http::certificate_hint(),
            source: e,
        },
        _ => Error::Fetch { url: url.to_owned(), source: e },
    };
    let progress = ui::spinner(format!("reading {url}"));
    let found = if let Some(id) = huggingface_id(url) {
        http.get(&format!(
            "https://huggingface.co/api/datasets/{id}/croissant"
        ))
        .json()
        .map(|dataset| Page { dataset, graph: Vec::new() })
        .map_err(fetch_error)
    } else {
        http.get(url).text().map_err(fetch_error).and_then(|html| {
            json_ld(&html)
                .iter()
                .find_map(Page::find)
                .ok_or_else(|| Error::NoMarkup { url: url.to_owned() })
        })
    };
    progress.finish_and_clear();
    let page = found?;
    let files = page.files();
    if files.is_empty() {
        match page.entries().len() {
            0 => ui::warn("the page's metadata lists no files"),
            1 => ui::warn("the page lists 1 file dataseek could not read"),
            n => ui::warn(format!(
                "the page lists {n} files dataseek could not read"
            )),
        }
    }
    print(out, url, &page, &files)
}

/// A page's schema.org Dataset node and the `@graph` it sits in. CKAN
/// portals with ckanext-dcat, such as HDX, write each file and organisation
/// as a node of its own in the graph and refer to it by `{"@id": ...}`.
struct Page {
    dataset: Value,
    /// Empty when the Dataset stands alone.
    graph: Vec<Value>,
}

impl Page {
    /// The first Dataset in one JSON-LD block: at the top level, in an
    /// array, or inside `@graph`.
    fn find(block: &Value) -> Option<Self> {
        let (dataset, graph) = find_dataset(block)?;
        Some(Self { dataset: dataset.clone(), graph: graph.to_vec() })
    }

    /// The graph node an `{"@id": ...}` reference names, or `node` itself
    /// when it is not a reference or the graph lacks that node.
    fn resolve<'a>(&'a self, node: &'a Value) -> &'a Value {
        let Value::Object(fields) = node else { return node };
        if fields.len() != 1 {
            return node;
        }
        let Some(id) = fields.get("@id") else { return node };
        self.graph
            .iter()
            .find(|n| {
                n.get("@id") == Some(id)
                    && n.as_object().is_some_and(|m| m.len() > 1)
            })
            .unwrap_or(node)
    }

    /// Every entry of the dataset's file list, references resolved,
    /// whether or not dataseek can read it.
    fn entries(&self) -> Vec<&Value> {
        ["distribution", "distributions"]
            .iter()
            .filter_map(|k| property(&self.dataset, k))
            .flat_map(|d| match d {
                Value::Array(items) => items.iter().collect(),
                other => vec![other],
            })
            .map(|entry| self.resolve(entry))
            .collect()
    }

    fn files(&self) -> Vec<File> {
        self.entries().into_iter().filter_map(|e| self.file(e)).collect()
    }

    /// One entry as a [`File`], `None` when nothing in it is readable. A
    /// bare string is the file's link; so is an `@id` that is a web address,
    /// when the entry names no other.
    fn file(&self, entry: &Value) -> Option<File> {
        if entry.is_string() {
            let url = text(entry, "")?;
            return Some(File { url: Some(url), ..File::default() });
        }
        let size = property(entry, "contentSize");
        let size_bytes = size.and_then(|size| {
            number(size, "")
                .or_else(|| text(size, "").as_deref().and_then(with_unit))
        });
        let file = File {
            name: property_text(entry, "name"),
            format: self.names(
                property(entry, "encodingFormat")
                    .or_else(|| property(entry, "fileFormat")),
            ),
            size_bytes,
            size_text: size
                .filter(|_| size_bytes.is_none())
                .and_then(|size| text(size, "")),
            checksum: checksum(entry),
            url: first_text_in(property(entry, "contentUrl"))
                .or_else(|| property_text(entry, "url"))
                .or_else(|| {
                    text(entry, "/@id").filter(|id| id.starts_with("http"))
                }),
            includes: self.names(property(entry, "includes")),
        };
        (file != File::default()).then_some(file)
    }

    /// A readable rendering of a schema.org value that may be a string, an
    /// object with a `name`/`@id`/`url`, a reference to such an object in
    /// the graph, or a list of any of these.
    fn names(&self, value: Option<&Value>) -> Option<String> {
        let value = self.resolve(value?);
        let rendered = match value {
            Value::String(s) => clean(s),
            Value::Array(items) => items
                .iter()
                .filter_map(|v| self.names(Some(v)))
                .collect::<Vec<_>>()
                .join(", "),
            Value::Object(_) => property_text(value, "name")
                .or_else(|| property_text(value, "value"))
                .or_else(|| text(value, "/@id"))
                .or_else(|| property_text(value, "url"))?,
            Value::Number(n) => n.to_string(),
            _ => return None,
        };
        (!rendered.is_empty()).then_some(rendered)
    }

    /// A text property of the dataset itself.
    fn text(&self, key: &str) -> Option<String> {
        property_text(&self.dataset, key)
    }

    /// A property of the dataset itself, rendered by [`Page::names`].
    fn names_of(&self, key: &str) -> Option<String> {
        self.names(property(&self.dataset, key))
    }
}

/// `node`'s `key`, written plainly or with the `schema:` prefix
/// ckanext-dcat uses.
fn property<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.get(key).or_else(|| node.get(format!("schema:{key}")))
}

fn property_text(node: &Value, key: &str) -> Option<String> {
    property(node, key).and_then(|v| text(v, ""))
}

/// The text of a value, or of the first entry of a list that has one.
fn first_text_in(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Array(items) => items.iter().find_map(|v| text(v, "")),
        other => text(other, ""),
    }
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
    /// `contentSize` as the page wrote it, when it is not a size dataseek
    /// reads. Only the text listing shows it; the JSON output carries the
    /// page's own metadata beside the file list.
    #[serde(skip)]
    size_text: Option<String>,
    /// `sha256:<hex>` or `md5:<hex>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    includes: Option<String>,
}

/// A size written with its unit, such as Zenodo's "8.19 MB": decimal units
/// up to PB, binary ones up to PiB. `None` for anything else, rather than a
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
        "pb" => 1e15,
        "kib" => 1024.0,
        "mib" => 1_048_576.0,
        "gib" => 1_073_741_824.0,
        "tib" => 1_099_511_627_776.0,
        "pib" => 1_125_899_906_842_624.0,
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

/// The first Dataset node in `value` and the `@graph` array holding it,
/// empty when there is none.
fn find_dataset(value: &Value) -> Option<(&Value, &[Value])> {
    match value {
        Value::Array(items) => items.iter().find_map(find_dataset),
        Value::Object(map) if is_dataset(map.get("@type")) => {
            Some((value, &[]))
        }
        Value::Object(map) => {
            let graph = map.get("@graph")?;
            let (dataset, inner) = find_dataset(graph)?;
            let around = match graph {
                Value::Array(nodes) if inner.is_empty() => nodes.as_slice(),
                _ => inner,
            };
            Some((dataset, around))
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

fn print(out: &Out, url: &str, page: &Page, files: &[File]) -> Result<()> {
    if out.json {
        let mut dataset = page.dataset.clone();
        if let Value::Object(fields) = &mut dataset {
            fields.insert("files".to_owned(), serde_json::json!(files));
        }
        return out.json(&serde_json::json!({
            "schema": "dataseek-inspect/1",
            "url": url,
            "dataset": dataset,
        }));
    }
    let field = |label: &str, value: Option<String>| -> std::io::Result<()> {
        match value {
            Some(v) if !v.is_empty() => {
                writeln!(out.stdout(), "{label:<12} {v}")
            }
            _ => Ok(()),
        }
    };
    field("name", page.text("name"))?;
    field("url", page.text("url").map(|u| out.link(&u)))?;
    field("identifier", page.names_of("identifier"))?;
    field("license", page.names_of("license"))?;
    field("creator", page.names_of("creator"))?;
    field("publisher", page.names_of("publisher"))?;
    field("modified", page.text("dateModified"))?;
    field("published", page.text("datePublished"))?;
    field("keywords", page.names_of("keywords"))?;
    field("temporal", page.text("temporalCoverage"))?;
    field("spatial", page.names_of("spatialCoverage"))?;
    field(
        "description",
        page.text("description").as_deref().and_then(summary),
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
        file.size_bytes.map(human_bytes).or_else(|| file.size_text.clone()),
        file.checksum.clone(),
        file.includes.clone(),
        file.url.as_deref().map(|url| out.link(url)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;
    use serde_json::json;

    /// The Dataset an HTML page's JSON-LD describes.
    fn page_in(html: &str) -> Page {
        json_ld(html).iter().find_map(Page::find).unwrap()
    }

    /// A fixture that is the Dataset itself, as Croissant and Dataverse's
    /// schema.org export are.
    fn standalone(name: &str) -> Page {
        let dataset = serde_json::from_str(&fixture::read("inspect", name));
        Page { dataset: dataset.unwrap(), graph: Vec::new() }
    }

    #[test]
    fn a_zenodo_page_names_its_dataset_by_full_address() {
        let found = page_in(&fixture::read("inspect", "zenodo.html")).dataset;
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
        assert_eq!(page_in(page).dataset["name"], "Rain");
    }

    #[test]
    fn a_ckan_page_lists_the_files_its_graph_refers_to() {
        let page = page_in(&fixture::read("inspect", "hdx.html"));
        assert_eq!(
            page.files(),
            [
                File {
                    name: Some("afg_admin_boundaries.shp.zip".into()),
                    format: Some("SHP".into()),
                    url: Some("https://data.humdata.org/dataset/4c303d7b-8eae-4a5a-a3aa-b2331fa39d74/resource/84b6e7e1-e907-488a-9a37-47b707145468/download/afg_admin_boundaries.shp.zip".into()),
                    ..File::default()
                },
                File {
                    name: Some("afg_admin_boundaries.xlsx".into()),
                    format: Some("XLSX".into()),
                    url: Some("https://data.humdata.org/dataset/4c303d7b-8eae-4a5a-a3aa-b2331fa39d74/resource/45f2ff90-13c0-46bf-a46e-7d443e9b9fdd/download/afg_admin_boundaries.xlsx".into()),
                    ..File::default()
                },
                File {
                    name: Some("afg_admin_boundaries.geojson.zip".into()),
                    format: Some("GeoJSON".into()),
                    url: Some("https://data.humdata.org/dataset/4c303d7b-8eae-4a5a-a3aa-b2331fa39d74/resource/330aad34-2254-4622-afac-e98ace1524ae/download/afg_admin_boundaries.geojson.zip".into()),
                    ..File::default()
                },
                File {
                    name: Some("afg_admin_boundaries.gdb.zip".into()),
                    format: Some("Geodatabase".into()),
                    url: Some("https://data.humdata.org/dataset/4c303d7b-8eae-4a5a-a3aa-b2331fa39d74/resource/361330e2-15f7-4bde-ad08-8bf9e37b5c41/download/afg_admin_boundaries.gdb.zip".into()),
                    ..File::default()
                },
            ]
        );
        assert_eq!(
            page.text("name").as_deref(),
            Some("Afghanistan - Subnational Administrative Boundaries")
        );
        assert_eq!(
            page.names_of("publisher").as_deref(),
            Some("OCHA Field Information Services Section (FISS)")
        );
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
        let page = Page { dataset: Value::Null, graph: Vec::new() };
        assert_eq!(page.names(Some(&v)).as_deref(), Some("Ada, Grace"));
    }

    #[test]
    fn zenodo_lists_each_file_by_format_and_link() {
        let files = page_in(&fixture::read("inspect", "zenodo.html")).files();
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
        let files = standalone("dataverse-schema-org.json").files();
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
        assert_eq!(
            standalone("dataverse-croissant.json").files()[1],
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
        assert_eq!(
            standalone("huggingface-croissant.json").files(),
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
    fn a_link_is_read_from_a_bare_string_a_list_or_an_id() {
        let dataset = json!({"@type": "Dataset", "distribution": [
            "https://example.org/a.csv",
            {"contentUrl": ["https://example.org/b.csv", "https://mirror.example.org/b.csv"]},
            {"@type": "DataDownload", "@id": "https://example.org/c.csv"},
            {"@type": "DataDownload", "@id": "_:b0"},
            42,
        ]});
        let page = Page { dataset, graph: Vec::new() };
        let link =
            |url: &str| File { url: Some(url.into()), ..File::default() };
        assert_eq!(
            page.files(),
            [
                link("https://example.org/a.csv"),
                link("https://example.org/b.csv"),
                link("https://example.org/c.csv"),
            ]
        );
        assert_eq!(page.entries().len(), 5);
    }

    #[test]
    fn sizes_are_read_with_their_units_or_not_at_all() {
        assert_eq!(with_unit("8.19 MB"), Some(8_190_000));
        assert_eq!(with_unit("1.5 GiB"), Some(1_610_612_736));
        assert_eq!(with_unit("12 bytes"), Some(12));
        assert_eq!(with_unit("2kB"), Some(2_000));
        assert_eq!(with_unit("1.5 PB"), Some(1_500_000_000_000_000));
        assert_eq!(with_unit("1 PiB"), Some(1_125_899_906_842_624));
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
