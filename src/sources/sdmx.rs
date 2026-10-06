//! SDMX dataflow lists from statistical agencies, parsed from SDMX-ML 2.1
//! (the one representation every agency serves) and searched locally. Each
//! dataflow is a dataset; its name is the title and the agency's data
//! browser, where one exists, is the link.

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

pub static IMF: Agency = Agency {
    dataflows: "https://api.imf.org/external/sdmx/2.1/dataflow",
    publisher: "International Monetary Fund",
    page: |agency, id| {
        Some(format!("https://data.imf.org/en/datasets/{agency}:{id}"))
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
pub static BIS: Agency = Agency {
    dataflows: "https://stats.bis.org/api/v1/dataflow",
    publisher: "Bank for International Settlements",
    page: |agency, id| {
        Some(format!("https://stats.bis.org/api/v1/dataflow/{agency}/{id}"))
    },
};
pub static ILO: Agency = Agency {
    dataflows: "https://sdmx.ilo.org/rest/dataflow",
    publisher: "International Labour Organization",
    page: |agency, id| {
        Some(format!("https://sdmx.ilo.org/rest/dataflow/{agency}/{id}"))
    },
};
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Description,
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
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().into_inner() {
                "Dataflow" => {
                    current = Some(Flow {
                        id: attribute(&e, "id").unwrap_or_default(),
                        agency: attribute(&e, "agencyID").unwrap_or_default(),
                        ..Flow::default()
                    });
                }
                name @ ("Name" | "Description") if current.is_some() => {
                    open = Some(Open {
                        field: if name == "Name" {
                            Field::Name
                        } else {
                            Field::Description
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
                    if let Some(flow) = current.take()
                        && !flow.id.is_empty()
                    {
                        let name = if flow.name.is_empty() {
                            flow.id.clone()
                        } else {
                            flow.name.clone()
                        };
                        flows.push(Flow { name, ..flow });
                    }
                }
                "Name" | "Description" => {
                    if let (Some(flow), Some(o)) =
                        (current.as_mut(), open.take())
                    {
                        let value = o.text.trim().to_owned();
                        let slot = match o.field {
                            Field::Name => &mut flow.name,
                            Field::Description => flow
                                .description
                                .get_or_insert_with(String::new),
                        };
                        if !value.is_empty() && (o.english || slot.is_empty())
                        {
                            *slot = value;
                        }
                    }
                }
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

fn attribute(element: &BytesStart<'_>, local: &str) -> Option<String> {
    element.attributes().flatten().find_map(|a| {
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
}
