//! Academic Torrents, read from the RSS 2.0 file of its whole database and
//! searched locally. Each item has a title, a category (Dataset, Paper,
//! Course), a details page and the torrent's size in bytes; only datasets
//! are kept.
//!
//! The file is 3.2 MB with 3,236 items, of which 1,724 are datasets and
//! 1,262 of those carry a description (November 2026). Many torrents
//! mirror data that its publisher gates behind an agreement, so the
//! dataset's own license page still decides who may use it. robots.txt
//! only lists `Allow` lines.

use quick_xml::Reader;
use quick_xml::events::Event;

use super::Ctx;
use super::sdmx::reference;
use crate::http::SourceError;
use crate::record::Dataset;

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let xml = ctx
        .http
        .get("https://academictorrents.com/database.xml")
        .slow()
        .text()?;
    parse(&xml)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Category,
    Link,
    Description,
    Size,
}

impl Field {
    fn of(element: &str) -> Option<Self> {
        match element {
            "title" => Some(Self::Title),
            "category" => Some(Self::Category),
            "link" => Some(Self::Link),
            "description" => Some(Self::Description),
            "size" => Some(Self::Size),
            _ => None,
        }
    }
}

/// The item being read, field by field. Text arrives in pieces around
/// entity references, so each field collects until its element closes.
#[derive(Default)]
struct Item {
    title: String,
    category: String,
    link: String,
    description: String,
    size: String,
}

impl Item {
    fn slot(&mut self, field: Field) -> &mut String {
        match field {
            Field::Title => &mut self.title,
            Field::Category => &mut self.category,
            Field::Link => &mut self.link,
            Field::Description => &mut self.description,
            Field::Size => &mut self.size,
        }
    }

    fn dataset(self) -> Option<Dataset> {
        if self.category.trim() != "Dataset" {
            return None;
        }
        let mut dataset = Dataset::new(&self.title, &self.link)
            .describe(Some(self.description));
        dataset.size_bytes = self.size.trim().parse().ok();
        dataset.valid()
    }
}

fn parse(xml: &str) -> Result<Vec<Dataset>, SourceError> {
    let mut reader = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut item: Option<Item> = None;
    let mut open: Option<Field> = None;
    let mut channel = false;
    loop {
        let piece = match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().into_inner();
                channel |= name == "channel";
                if name == "item" {
                    item = Some(Item::default());
                }
                open = item.as_ref().and_then(|_| Field::of(name));
                continue;
            }
            Ok(Event::End(e)) => {
                if e.local_name().into_inner() == "item" {
                    entries.extend(item.take().and_then(Item::dataset));
                }
                open = None;
                continue;
            }
            Ok(Event::Text(t)) => t.xml10_content().into_owned(),
            Ok(Event::GeneralRef(r)) => reference(&r).to_owned(),
            Ok(Event::Eof) if channel => return Ok(entries),
            Ok(Event::Eof) => {
                return Err(SourceError::shape("no RSS channel"));
            }
            Err(e) => {
                return Err(SourceError::shape(format!("bad RSS: {e}")));
            }
            Ok(_) => continue,
        };
        if let (Some(item), Some(field)) = (item.as_mut(), open) {
            item.slot(field).push_str(&piece);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::text("academictorrents.xml")).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "ACOUSLIC-AI : Abdominal Circumference Operator-agnostic \
                 UltraSound measurement in Low-Income Countries using \
                 Artificial Intelligence",
                "The National Disaster & Emergency Management University",
                titles[2],
            ],
            "the Paper item is dropped"
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: titles[0].into(),
                url: "https://academictorrents.com/details/\
                      ca0ddd6c34161c83afceb997f9bf14494b1e5215"
                    .into(),
                description: Some(
                    "Prenatal Ultrasound Frames acquired by novice operators \
                     using a pre-specified blind-sweep protocol. Annotations \
                     of fetal abdominal circumference in frames where it \
                     could be measured. Reference abdominal circumference \
                     measurements (in mm) per sweep. A detailed description \
                     of the imaging data and annotations can be..."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: None,
                size_bytes: Some(38_034_134_378),
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
        assert!(hits[2].title.starts_with("Smithsonian - National"));
        assert_eq!(hits[2].description, None);
    }

    #[test]
    fn a_body_that_is_not_rss_is_a_changed_shape() {
        for body in [
            "{\"error\": \"unexpected\"}",
            "<html><body>no feed</body></html>",
        ] {
            assert!(
                matches!(parse(body), Err(SourceError::Shape(_))),
                "{body}"
            );
        }
    }
}
