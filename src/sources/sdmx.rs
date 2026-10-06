//! SDMX dataflow lists from statistical agencies, parsed from SDMX-ML 2.1
//! (the one representation every agency serves) and searched locally. Each
//! dataflow is a dataset; its name is the title and the agency's data
//! browser, where one exists, is the link.
//!
//! Every agency answers with its whole list in one response, so nothing
//! pages: 15 dataflows at UNdata, 1,216 at ILOSTAT (7.3 MB) and 1,548 at the
//! OECD (8.9 MB, 1.5 MB gzipped), measured in October 2026. The OECD allows
//! 60 data downloads an hour per IP and may block an address above that
//! (OECD API best practices, March 2026); one list a week stays far below
//! it. The IMF's terms prohibit bulk download by automated technology
//! without permission (IMF copyright and terms, October 2026). ILOSTAT dates
//! each dataflow in a `LAST_UPDATE` annotation, day first (ILOSTAT, October
//! 2026).

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

pub struct Agency {
    pub dataflows: &'static str,
    pub publisher: &'static str,
    /// The page for a dataflow, from its agency id and dataflow id; `None`
    /// when the dataflow has no page of its own.
    pub page: fn(&str, &str) -> Option<String>,
}

/// Vintage snapshots (`..._VINTAGE`, 119 of 222 on 2026-10-06) and the
/// flows `NA_MAIN` and `SDG` have no page on the data portal (all 404), so
/// they are not listed. The other 101 answered 200.
pub static IMF: Agency = Agency {
    dataflows: "https://api.imf.org/external/sdmx/2.1/dataflow",
    publisher: "International Monetary Fund",
    page: |agency, id| {
        let has_page =
            !id.ends_with("_VINTAGE") && !matches!(id, "NA_MAIN" | "SDG");
        has_page
            .then(|| format!("https://data.imf.org/en/datasets/{agency}:{id}"))
    },
};
pub static OECD: Agency = Agency {
    dataflows: "https://sdmx.oecd.org/public/rest/dataflow/all",
    publisher: "OECD",
    page: |agency, id| {
        Some(format!(
            "https://data-explorer.oecd.org/vis?df[ds]=dsDisseminateFinalDMZ&df[id]={id}&df[ag]={agency}"
        ))
    },
};
/// ECB.DISS dataflows are the published-series subsets of other dataflows
/// and have no page on the portal (89 of 215 on 2026-10-06, all 404).
///
/// The link is a dataset's information tab, not its bare page. The bare
/// page redirects to itself with the dataset's name in the query string,
/// which the portal's firewall blocks for the "International Reserves of the
/// Eurosystem" flows (503), and it refuses datasets still in draft (403).
/// The information tab answered 200 without a redirect for all 126 other
/// flows, drafts included, on 2026-10-06.
pub static ECB: Agency = Agency {
    dataflows: "https://data-api.ecb.europa.eu/service/dataflow",
    publisher: "European Central Bank",
    page: |agency, id| {
        (agency != "ECB.DISS").then(|| {
            format!(
                "https://data.ecb.europa.eu/data/datasets/{id}/data-information"
            )
        })
    },
};
pub static BUNDESBANK: Agency = Agency {
    dataflows: "https://api.statistiken.bundesbank.de/rest/metadata/dataflow/BBK",
    publisher: "Deutsche Bundesbank",
    page: |agency, id| {
        Some(format!(
            "https://api.statistiken.bundesbank.de/rest/metadata/dataflow/{agency}/{id}"
        ))
    },
};
/// The BIS asks API users to use the newest version it has released (terms of
/// permitted use of BIS statistics, October 2026), which is v2; its dataflow
/// list is the same document as v1's.
pub static BIS: Agency = Agency {
    dataflows: "https://stats.bis.org/api/v2/structure/dataflow",
    publisher: "Bank for International Settlements",
    page: |agency, id| {
        Some(format!(
            "https://stats.bis.org/api/v2/structure/dataflow/{agency}/{id}"
        ))
    },
};
pub static ILO: Agency = Agency {
    dataflows: "https://sdmx.ilo.org/rest/dataflow",
    publisher: "International Labour Organization",
    page: |agency, id| {
        Some(format!("https://sdmx.ilo.org/rest/dataflow/{agency}/{id}"))
    },
};
/// The documented `/ws/rest` path answers 302 to `/legacy/ws/rest`, which the
/// client follows (UNdata API manual and live response, October 2026).
pub static UNDATA: Agency = Agency {
    dataflows: "https://data.un.org/ws/rest/dataflow",
    publisher: "United Nations Statistics Division",
    page: |agency, id| {
        Some(format!("https://data.un.org/ws/rest/dataflow/{agency}/{id}"))
    },
};

pub fn list(
    ctx: &Ctx<'_>,
    agency: &Agency,
) -> Result<Vec<Dataset>, SourceError> {
    let xml = ctx
        .http
        .get(agency.dataflows)
        .header("Accept", "application/vnd.sdmx.structure+xml;version=2.1")
        .slow()
        .text()?;
    parse(agency, &xml)
}

pub(super) fn parse(
    agency: &Agency,
    xml: &str,
) -> Result<Vec<Dataset>, SourceError> {
    let flows = flows(xml)?;
    if flows.is_empty() {
        return Err(SourceError::shape("no Dataflow elements"));
    }
    Ok(flows
        .into_iter()
        .filter_map(|flow| {
            let page = (agency.page)(&flow.agency, &flow.id)?;
            let mut dataset = Dataset::new(&flow.name, &page)
                .describe(flow.description.or(Some(flow.id)));
            dataset.publisher = Some(agency.publisher.to_owned());
            dataset.updated = flow.updated;
            dataset.valid()
        })
        .collect())
}

#[derive(Default)]
pub struct Flow {
    id: String,
    agency: String,
    name: String,
    description: Option<String>,
    updated: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Description,
    AnnotationTitle,
    AnnotationType,
}

/// The element being read inside a dataflow: which field, whether it is the
/// English one, and its text so far. Text arrives in pieces around entity
/// references, so it is collected until the element closes.
struct Open {
    field: Field,
    english: bool,
    text: String,
}

pub fn flows(xml: &str) -> Result<Vec<Flow>, SourceError> {
    let mut reader = Reader::from_str(xml);
    let mut flows = Vec::new();
    let mut current: Option<Flow> = None;
    let mut open: Option<Open> = None;
    let mut annotation_title = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().into_inner() {
                "Dataflow" => current = Some(start_of(&e)),
                name @ ("Name" | "Description" | "AnnotationTitle"
                | "AnnotationType")
                    if current.is_some() =>
                {
                    open = Some(Open {
                        field: match name {
                            "Name" => Field::Name,
                            "Description" => Field::Description,
                            "AnnotationTitle" => Field::AnnotationTitle,
                            _ => Field::AnnotationType,
                        },
                        english: attribute(&e, "lang")
                            .is_none_or(|l| l == "en"),
                        text: String::new(),
                    });
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some(o) = open.as_mut() {
                    o.text.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(o) = open.as_mut() {
                    o.text.push_str(match &*r {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => " ",
                    });
                }
            }
            Ok(Event::End(e)) => match e.local_name().into_inner() {
                "Dataflow" => {
                    if let Some(mut flow) = current.take()
                        && !flow.id.is_empty()
                    {
                        if flow.name.is_empty() {
                            flow.name.clone_from(&flow.id);
                        }
                        flows.push(flow);
                    }
                }
                "Name" | "Description" | "AnnotationTitle"
                | "AnnotationType" => {
                    if let (Some(flow), Some(o)) =
                        (current.as_mut(), open.take())
                    {
                        close(flow, o, &mut annotation_title);
                    }
                }
                "Annotation" => annotation_title.clear(),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(SourceError::shape(format!("bad SDMX-ML: {e}")));
            }
            Ok(_) => {}
        }
    }
    Ok(flows)
}

fn start_of(element: &BytesStart<'_>) -> Flow {
    let (mut id, mut agency) = (None, None);
    for a in element.attributes().with_checks(false).flatten() {
        let slot = match a.key.local_name().into_inner() {
            "id" => &mut id,
            "agencyID" => &mut agency,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(a.value.into_owned());
        }
        if id.is_some() && agency.is_some() {
            break;
        }
    }
    Flow {
        id: id.unwrap_or_default(),
        agency: agency.unwrap_or_default(),
        ..Flow::default()
    }
}

/// An element closed inside `flow`: a name or description is kept, an
/// annotation's title waits for its type, and a `LAST_UPDATE` type dates the
/// flow.
fn close(flow: &mut Flow, open: Open, annotation_title: &mut String) {
    let value = open.text.trim();
    match open.field {
        Field::Name => keep(&mut flow.name, value, open.english),
        Field::Description => keep(
            flow.description.get_or_insert_with(String::new),
            value,
            open.english,
        ),
        Field::AnnotationTitle => *annotation_title = open.text,
        Field::AnnotationType => {
            if value == "LAST_UPDATE" {
                flow.updated = day_first(annotation_title.trim());
            }
        }
    }
}

/// `value` into `slot` when it is the English text, or when nothing else is
/// there yet.
fn keep(slot: &mut String, value: &str, english: bool) {
    if !value.is_empty() && (english || slot.is_empty()) {
        value.clone_into(slot);
    }
}

/// "03/10/2026 07:10:53" as "2026-10-03": ILOSTAT writes the day first.
fn day_first(stamp: &str) -> Option<String> {
    let (day, rest) = stamp.split_once('/')?;
    let (month, rest) = rest.split_once('/')?;
    let year = rest.split_whitespace().next()?;
    let digits =
        |s: &str, len| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    let within = |s: &str, max: u8| {
        s.parse::<u8>().is_ok_and(|n| (1..=max).contains(&n))
    };
    (digits(day, 2)
        && digits(month, 2)
        && digits(year, 4)
        && within(day, 31)
        && within(month, 12))
    .then(|| format!("{year}-{month}-{day}"))
}

fn attribute(element: &BytesStart<'_>, local: &str) -> Option<String> {
    element.attributes().with_checks(false).flatten().find_map(|a| {
        (a.key.local_name().into_inner() == local)
            .then(|| a.value.into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn dataflows_take_the_english_name() {
        let xml = r#"<?xml version="1.0"?>
        <mes:Structure xmlns:mes="m" xmlns:str="s" xmlns:com="c">
          <str:Dataflows>
            <str:Dataflow id="EXR" agencyID="ECB" version="1.0">
              <com:Name xml:lang="de">Wechselkurse</com:Name>
              <com:Name xml:lang="en">Exchange rates</com:Name>
            </str:Dataflow>
            <str:Dataflow id="NONAME" agencyID="ECB"/>
            <str:Dataflow id="BOP" agencyID="ECB"><com:Name>Balance of payments</com:Name></str:Dataflow>
          </str:Dataflows>
        </mes:Structure>"#;
        let flows = flows(xml).unwrap();
        let names: Vec<_> = flows.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Exchange rates", "Balance of payments"]);
        assert_eq!(flows[0].agency, "ECB");
    }

    #[test]
    fn broken_xml_is_a_shape_error() {
        assert!(flows("<a><b></a>").is_err());
    }

    #[test]
    fn dataflows_map_from_a_recorded_list() {
        let datasets = parse(&ECB, &fixture::text("sdmx.xml")).unwrap();
        let titles: Vec<&str> =
            datasets.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "AGR",
                "AMECO",
                "Exchange Rates",
                "Quarterly non-financial accounts, QSA by country",
                "International Reserves of the Eurosystem (BPM6)",
            ]
        );
        assert_eq!(
            datasets[0],
            Dataset {
                title: "AGR".into(),
                url: "https://data.ecb.europa.eu/data/datasets/AGR/\
                      data-information"
                    .into(),
                description: Some("AGR".into()),
                publisher: Some("European Central Bank".into()),
                ..Dataset::default()
            }
        );
        let urls: Vec<&str> =
            datasets[3..].iter().map(|d| d.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://data.ecb.europa.eu/data/datasets/IEAF/data-information",
                "https://data.ecb.europa.eu/data/datasets/RA6/data-information",
            ],
            "a draft and a firewalled flow link past the bare page"
        );
    }

    #[test]
    fn imf_lists_only_the_flows_with_a_portal_page() {
        let datasets = parse(&IMF, &fixture::text("sdmx.imf.xml")).unwrap();
        let titles: Vec<&str> =
            datasets.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Effective Exchange Rate (EER)", "Consumer Price Index (CPI)"],
            "a vintage snapshot, NA_MAIN and SDG answer 404 on the portal"
        );
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Effective Exchange Rate (EER)".into(),
                url: "https://data.imf.org/en/datasets/IMF.STA:EER".into(),
                description: Some(
                    "The Effective Exchange Rate (EER) dataset includes \
                     annual, quarterly and monthly nominal and real \
                     effective exchange rates by economy."
                        .into()
                ),
                publisher: Some("International Monetary Fund".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn ilo_dates_a_flow_by_its_last_update_annotation() {
        let datasets = parse(&ILO, &fixture::text("sdmx.ilo.xml")).unwrap();
        assert_eq!(datasets.len(), 2);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "SDG Harmonized Global Dataflow".into(),
                url: "https://sdmx.ilo.org/rest/dataflow/IAEG-SDGs/DF_SDG_GLH"
                    .into(),
                description: Some(
                    "Reporting and dissemination dataflow for harmonized \
                     global SDG indicators."
                        .into()
                ),
                publisher: Some("International Labour Organization".into()),
                ..Dataset::default()
            },
            "no LAST_UPDATE annotation, no date"
        );
        assert_eq!(
            datasets[1],
            Dataset {
                title: "Unemployment rate by sex and age".into(),
                url: "https://sdmx.ilo.org/rest/dataflow/ILO/\
                      DF_UNE_DEAP_SEX_AGE_RT"
                    .into(),
                description: Some(
                    "With the aim of promoting international comparability, \
                     statistics presented on ILOSTAT are based on standard \
                     international definitions wherever feasible and may \
                     differ from official national figures. This series is \
                     based on the 13th ICLS definitions."
                        .into()
                ),
                publisher: Some("International Labour Organization".into()),
                updated: Some("2026-10-03".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn an_annotation_title_does_not_leak_into_the_next_annotation() {
        let xml = r#"<mes:Structure xmlns:mes="m" xmlns:str="s" xmlns:com="c">
          <str:Dataflow id="A" agencyID="ILO">
            <com:Annotations>
              <com:Annotation>
                <com:AnnotationTitle>03/10/2026 07:10:53</com:AnnotationTitle>
                <com:AnnotationType>DEFAULT</com:AnnotationType>
              </com:Annotation>
              <com:Annotation>
                <com:AnnotationType>LAST_UPDATE</com:AnnotationType>
              </com:Annotation>
            </com:Annotations>
          </str:Dataflow>
        </mes:Structure>"#;
        assert_eq!(flows(xml).unwrap()[0].updated, None);
    }

    #[test]
    fn only_a_day_first_date_is_read() {
        assert_eq!(
            day_first("03/10/2026 07:10:53").as_deref(),
            Some("2026-10-03")
        );
        assert_eq!(day_first("2026-10-03T07:10:53"), None);
        assert_eq!(day_first("3/10/2026"), None);
        assert_eq!(day_first("03/10"), None);
        assert_eq!(day_first("99/99/2026 00:00:00"), None);
        assert_eq!(day_first("00/10/2026"), None);
    }

    #[test]
    fn bis_links_to_the_v2_structure_of_each_flow() {
        let datasets = parse(&BIS, &fixture::text("sdmx.bis.xml")).unwrap();
        let urls: Vec<&str> =
            datasets.iter().map(|d| d.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://stats.bis.org/api/v2/structure/dataflow/BIS/\
                 BIS_REL_CAL",
                "https://stats.bis.org/api/v2/structure/dataflow/BIS/\
                 WS_CBPOL",
                "https://stats.bis.org/api/v2/structure/dataflow/BIS.CBS/CBS",
            ]
        );
        assert_eq!(
            datasets[1],
            Dataset {
                title: "Central bank policy rates".into(),
                url: urls[1].into(),
                description: Some(
                    "The interest rate which best captures the monetary \
                     authorities' policy intentions."
                        .into()
                ),
                publisher: Some("Bank for International Settlements".into()),
                ..Dataset::default()
            }
        );
    }
}
