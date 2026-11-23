//! The Papers with Code dataset index, read from the archive of it frozen
//! on 2025-07-28 and searched locally. paperswithcode.com now sends its
//! dataset pages to Hugging Face repositories it guesses at (COCO lands on
//! COCO-AB), so each record links to the dataset's own homepage, and the
//! 3,743 datasets without one are dropped.
//!
//! The dump is one gzip file of 8.4 MB in the `pwc-archive/files`
//! repository on Hugging Face; it inflates to 47 MB of JSON with 15,008
//! datasets and 27 fields each, more than the cache budget holds (ADR
//! 0007). Only the title, homepage, full name, tasks, modalities, license
//! name and paper count are read and kept, and the tasks and modalities
//! are the description, so a query matches the title, tasks and
//! modalities. The paper count is the popularity.

use std::io::Read;

use flate2::read::GzDecoder;
use serde::Deserialize;

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

const DUMP: &str = "https://huggingface.co/datasets/pwc-archive/files/\
    resolve/main/jul-28-datasets.json.gz";
/// Room for the 47 MB the dump inflates to, and a stop for a body that
/// would inflate without end.
const INFLATED_LIMIT: u64 = 128 * 1024 * 1024;

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let gzip = ctx.http.get(DUMP).slow().bytes()?;
    parse(&inflate(&gzip)?)
}

fn inflate(gzip: &[u8]) -> Result<Vec<u8>, SourceError> {
    let mut json = Vec::new();
    GzDecoder::new(gzip)
        .take(INFLATED_LIMIT)
        .read_to_end(&mut json)
        .map_err(|e| SourceError::shape(format!("not gzip: {e}")))?;
    if u64::try_from(json.len()).is_ok_and(|n| n >= INFLATED_LIMIT) {
        return Err(SourceError::shape("the dump inflates past 128 MB"));
    }
    Ok(json)
}

#[derive(Deserialize)]
struct Row {
    name: Option<String>,
    full_name: Option<String>,
    homepage: Option<String>,
    tasks: Option<Vec<Task>>,
    modalities: Option<Vec<String>>,
    license_name: Option<String>,
    num_papers: Option<u64>,
}

#[derive(Deserialize)]
struct Task {
    task: Option<String>,
}

fn parse(json: &[u8]) -> Result<Vec<Dataset>, SourceError> {
    let rows: Vec<Row> = serde_json::from_slice(json)
        .map_err(|e| SourceError::shape(format!("not a dataset list: {e}")))?;
    Ok(rows.into_iter().filter_map(record).collect())
}

fn record(row: Row) -> Option<Dataset> {
    let name = row.name?;
    let tasks: Vec<String> = row
        .tasks
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| t.task)
        .collect();
    let modalities = row.modalities.unwrap_or_default();
    let facts = [
        row.full_name.filter(|full| !full.is_empty() && *full != name),
        (!tasks.is_empty()).then(|| format!("Tasks: {}", tasks.join(", "))),
        (!modalities.is_empty())
            .then(|| format!("Modalities: {}", modalities.join(", "))),
    ];
    let description: Vec<String> = facts.into_iter().flatten().collect();
    let mut dataset = Dataset::new(&name, &row.homepage?)
        .describe(Some(description.join(". ")));
    dataset.license = row.license_name;
    dataset.popularity = row.num_papers;
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(fixture::text("pwc.json").as_bytes()).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            ["MNIST", "CelebA", "GLUE"],
            "JFT-300M has no homepage"
        );
        assert_eq!(
            hits[1].description.as_deref(),
            Some(
                "CelebFaces Attributes Dataset. Tasks: Image \
                 Classification, Image Generation, Image Super-Resolution, \
                 Face Alignment. Modalities: Images"
            )
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "MNIST".into(),
                url: "http://yann.lecun.com/exdb/mnist/".into(),
                description: Some(
                    "Tasks: Image Classification, Speech Recognition, \
                     Question Answering, Image Generation. Modalities: \
                     Images"
                        .into()
                ),
                publisher: None,
                doi: None,
                license: Some("Unknown".into()),
                updated: None,
                size_bytes: None,
                popularity: Some(7651),
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_dump_is_read_through_gzip() {
        let json = fixture::text("pwc.json");
        let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
        gzip.write_all(json.as_bytes()).unwrap();
        let inflated = inflate(&gzip.finish().unwrap()).unwrap();
        assert_eq!(inflated, json.as_bytes());
    }

    #[test]
    fn a_body_that_is_not_the_dump_is_a_changed_shape() {
        assert!(matches!(
            inflate(b"{\"error\": \"unexpected\"}"),
            Err(SourceError::Shape(_))
        ));
        assert!(matches!(
            parse(b"{\"error\": \"unexpected\"}"),
            Err(SourceError::Shape(_))
        ));
    }
}
