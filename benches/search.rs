//! The CPU side of a search, by stage: parsing downloaded catalogs, loading
//! a cached catalog, searching it, fusing the per-source lists and cleaning
//! remote text. Network time dwarfs all of it; these exist so a change that
//! makes one stage grow faster than its input shows up before a user waits
//! on it (ADR 0012).
//!
//! Inputs are synthetic, shaped after real responses (ECB's SDMX-ML dataflow
//! list, Eurostat's table of contents, 2026-10-06) and built from a word list
//! by index, so every run sees the same bytes without a seed.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    missing_docs,
    reason = "setup arithmetic on small indices, a failed setup aborts the bench, and criterion_group! expands to an undocumented pub fn"
)]

use std::fmt::Write;
use std::hint::black_box;
use std::time::Duration;

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use dataseek::internals::{
    Cache, Dataset, Kind, catalog_search, clean, eurostat_toc, merge,
    sdmx_dataflows, weigh,
};

#[rustfmt::skip]
const WORDS: [&str; 48] = [
    "sea", "surface", "temperature", "population", "census", "region",
    "unemployment", "monthly", "gross", "domestic", "product", "inflation",
    "consumer", "price", "index", "exchange", "rates", "balance", "payments",
    "emissions", "carbon", "precipitation", "daily", "annual", "household",
    "income", "survey", "labour", "trade", "exports", "energy", "electricity",
    "mortality", "births", "education", "country", "employment", "wages",
    "agriculture", "land", "cover", "ocean", "salinity", "glacier", "species",
    "occurrence", "gene", "traffic",
];

const QUERY: &str = "sea surface temperature";

/// Catalog sizes: Eurostat's table of contents holds about 10,000 datasets
/// and tables, the SDMX agencies hundreds to a few thousand.
const CATALOG_SIZES: [usize; 3] = [1_000, 4_000, 16_000];

/// 76 sources answering with this many records each.
const RECORDS_PER_SOURCE: [usize; 3] = [5, 20, 80];
const SOURCES: usize = 76;

fn word(i: usize) -> &'static str {
    WORDS[i % WORDS.len()]
}

/// Four to seven words, varied by `i` so titles rarely repeat.
fn phrase(i: usize) -> String {
    (0..4 + i % 4)
        .map(|k| word(i.wrapping_mul(31).wrapping_add(k * 17) >> (k % 3)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sdmx_xml(flows: usize) -> String {
    let mut xml = String::from(
        r#"<?xml version='1.0' encoding='UTF-8'?><mes:Structure xmlns:mes="http://www.sdmx.org/resources/sdmxml/schemas/v2_1/message" xmlns:str="http://www.sdmx.org/resources/sdmxml/schemas/v2_1/structure" xmlns:com="http://www.sdmx.org/resources/sdmxml/schemas/v2_1/common"><mes:Header><mes:ID>IREF004131</mes:ID></mes:Header><mes:Structures><str:Dataflows>"#,
    );
    for i in 0..flows {
        let id = format!("DF{i:05}");
        write!(
            xml,
            r#"<str:Dataflow urn="urn:sdmx:org.sdmx.infomodel.datastructure.Dataflow=ECB:{id}(1.0)" isExternalReference="false" agencyID="ECB" id="{id}" isFinal="false" version="1.0">"#
        )
        .unwrap();
        if i % 2 == 0 {
            write!(
                xml,
                r#"<com:Name xml:lang="fr">{}</com:Name>"#,
                phrase(i + 1)
            )
            .unwrap();
        }
        write!(
            xml,
            r#"<com:Name xml:lang="en">{} &amp; {}</com:Name>"#,
            phrase(i),
            word(i)
        )
        .unwrap();
        if i % 3 == 0 {
            write!(
                xml,
                r#"<com:Description xml:lang="en">{}</com:Description>"#,
                phrase(i + 7)
            )
            .unwrap();
        }
        write!(
            xml,
            r#"<str:Structure><Ref package="datastructure" agencyID="ECB" id="ECB_{id}" version="1.0" class="DataStructure"/></str:Structure></str:Dataflow>"#
        )
        .unwrap();
    }
    xml.push_str("</str:Dataflows></mes:Structures></mes:Structure>");
    xml
}

/// Datasets and tables nested under folders, every eighth line a folder,
/// titles indented by depth as the real file does.
fn eurostat_tsv(lines: usize) -> String {
    let mut toc = String::from(
        "\"title\"\t\"code\"\t\"type\"\t\"last update of data\"\t\"last table structure change\"\t\"data start\"\t\"data end\"\t\"values\"\n",
    );
    for i in 0..lines {
        let indent = " ".repeat(4 * (1 + i % 6));
        let kind = match i % 8 {
            0 => "folder",
            1 => "table",
            _ => "dataset",
        };
        writeln!(
            toc,
            "\"{indent}{}\"\t\"{}_{i}\"\t\"{kind}\"\t\"{:02}.{:02}.2026\"\t\"10.01.2024\"\t\"2011\"\t\"2025\"\t{}",
            phrase(i),
            word(i),
            1 + i % 28,
            1 + i % 12,
            i * 37
        )
        .unwrap();
    }
    toc
}

fn catalog(entries: usize) -> Vec<Dataset> {
    (0..entries)
        .map(|i| {
            Dataset::new(
                &phrase(i),
                &format!("https://catalog.example.org/datasets/{i}"),
            )
            .describe(Some(format!(
                "{} {}",
                phrase(i + 3),
                phrase(i + 11)
            )))
        })
        .collect()
}

/// One source's ranked list. Every 25th record carries a DOI that every
/// source shares at that rank, matching the bench's measured 4% of hits
/// found by more than one source.
fn source_list(source: usize, records: usize) -> Vec<Dataset> {
    (0..records)
        .map(|rank| {
            let mut dataset = Dataset::new(
                &phrase(source * 101 + rank),
                &format!("https://repo{source}.example.org/records/{rank}"),
            )
            .describe(Some(format!("{QUERY} {}", phrase(rank))));
            dataset.publisher = Some(format!("Publisher {source}"));
            if rank % 25 == 0 {
                dataset.doi = Some(format!("10.5281/zenodo.{rank}"));
            }
            dataset
        })
        .collect()
}

fn html(bytes: usize) -> String {
    let paragraph = "<p>Monthly <b>sea surface temperature</b> from \
        <a href=\"https://example.org/sst\">NOAA&nbsp;ERSST</a> &amp; \
        partners, &quot;gridded&quot; at 2&#x27; resolution.\u{1b}[31m</p>\n";
    paragraph.repeat(bytes.div_ceil(paragraph.len()))
}

/// Prose whose ampersands start no entity, as in "R&D" or "Q&A", and which
/// has no semicolon to end one.
fn ampersands(bytes: usize) -> String {
    let sentence = "Research & development spending by R&D sector, \
        with Q&A notes from AT&T and P&G. ";
    sentence.repeat(bytes.div_ceil(sentence.len()))
}

fn parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/sdmx");
    for flows in [100, 1_000, 10_000] {
        let xml = sdmx_xml(flows);
        group.throughput(Throughput::Elements(flows as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(flows),
            &xml,
            |b, xml| {
                b.iter(|| sdmx_dataflows(black_box(xml)).unwrap());
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("parse/eurostat");
    for lines in CATALOG_SIZES {
        let toc = eurostat_tsv(lines);
        group.throughput(Throughput::Elements(lines as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(lines),
            &toc,
            |b, toc| {
                b.iter(|| eurostat_toc(black_box(toc)));
            },
        );
    }
    group.finish();
}

fn catalogs(c: &mut Criterion) {
    let mut group = c.benchmark_group("catalog/search");
    for entries in CATALOG_SIZES {
        let catalog = catalog(entries);
        group.throughput(Throughput::Elements(entries as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(entries),
            &catalog,
            |b, catalog| {
                b.iter(|| {
                    catalog_search(black_box(catalog), black_box(QUERY), 20)
                });
            },
        );
    }
    group.finish();

    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path().to_owned());
    let mut group = c.benchmark_group("catalog/load");
    for entries in CATALOG_SIZES {
        let key = format!("catalog-{entries}");
        cache.store(Kind::Catalog, &key, &catalog(entries));
        group.throughput(Throughput::Elements(entries as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(entries),
            &key,
            |b, key| {
                b.iter(|| {
                    cache
                        .load::<Vec<Dataset>>(
                            Kind::Catalog,
                            black_box(key),
                            Duration::MAX,
                        )
                        .unwrap()
                });
            },
        );
    }
    group.finish();
}

fn fusion(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge");
    for records in RECORDS_PER_SOURCE {
        let lists: Vec<(&'static str, Vec<Dataset>)> = (0..SOURCES)
            .map(|s| {
                let id: &'static str =
                    Box::leak(format!("source{s}").into_boxed_str());
                (id, source_list(s, records))
            })
            .collect();
        group.throughput(Throughput::Elements((SOURCES * records) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(SOURCES * records),
            &lists,
            |b, lists| {
                b.iter(|| weigh(merge(black_box(lists)), black_box(QUERY)));
            },
        );
    }
    group.finish();
}

fn sanitize(c: &mut Criterion) {
    let mut group = c.benchmark_group("clean");
    for bytes in [200, 2_000, 20_000] {
        let text = html(bytes);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(bytes),
            &text,
            |b, text| {
                b.iter(|| clean(black_box(text)));
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("clean/ampersands");
    for bytes in [2_000, 20_000] {
        let text = ampersands(bytes);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(bytes),
            &text,
            |b, text| {
                b.iter(|| clean(black_box(text)));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, parse, catalogs, fusion, sanitize);
criterion_main!(benches);
