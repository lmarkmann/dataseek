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
    /// The page for a dataflow, from its agency id and dataflow id.
    pub page: fn(&str, &str) -> String,
}

pub static IMF: Agency = Agency {
    dataflows: "https://api.imf.org/external/sdmx/2.1/dataflow",
    publisher: "International Monetary Fund",
    page: |agency, id| {
        format!("https://data.imf.org/en/datasets/{agency}:{id}")
    },
};
pub static OECD: Agency = Agency {
    dataflows: "https://sdmx.oecd.org/public/rest/dataflow/all",
    publisher: "OECD",
    page: |agency, id| {
        format!(
            "https://data-explorer.oecd.org/vis?df[ds]=dsDisseminateFinalDMZ&df[id]={id}&df[ag]={agency}"
        )
    },
};
pub static ECB: Agency = Agency {
    dataflows: "https://data-api.ecb.europa.eu/service/dataflow",
    publisher: "European Central Bank",
    page: |_, id| format!("https://data.ecb.europa.eu/data/datasets/{id}"),
};
pub static BIS: Agency = Agency {
    dataflows: "https://stats.bis.org/api/v1/dataflow",
    publisher: "Bank for International Settlements",
    page: |agency, id| {
        format!("https://stats.bis.org/api/v1/dataflow/{agency}/{id}")
    },
};
pub static ILO: Agency = Agency {
    dataflows: "https://sdmx.ilo.org/rest/dataflow",
    publisher: "International Labour Organization",
    page: |agency, id| {
        format!("https://sdmx.ilo.org/rest/dataflow/{agency}/{id}")
    },
};
pub static UNDATA: Agency = Agency {
    dataflows: "https://data.un.org/ws/rest/dataflow",
    publisher: "United Nations Statistics Division",
    page: |agency, id| {
        format!("https://data.un.org/ws/rest/dataflow/{agency}/{id}")
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
    let flows = parse(&xml)?;
    if flows.is_empty() {
        return Err(SourceError::shape("no Dataflow elements"));
    }
    Ok(flows
        .into_iter()
        .filter_map(|flow| {
            let mut dataset = Dataset::new(
                &flow.name,
                &(agency.page)(&flow.agency, &flow.id),
            )
            .describe(flow.description.or(Some(flow.id)));
            dataset.publisher = Some(agency.publisher.to_owned());
            dataset.valid()
        })
        .collect())
}

#[derive(Default)]
struct Flow {
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

fn parse(xml: &str) -> Result<Vec<Flow>, SourceError> {
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
        let flows = parse(xml).unwrap();
        let names: Vec<_> = flows.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Exchange rates", "Balance of payments"]);
        assert_eq!(flows[0].agency, "ECB");
    }

    #[test]
    fn broken_xml_is_a_shape_error() {
        assert!(parse("<a><b></a>").is_err());
    }
}
