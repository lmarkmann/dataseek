# ADR 0020: Filters run on the merged list; a source filters only where it is never stricter

- Status: accepted
- Date: 2026-10-20

## Context

An agent or a user often knows more than the words of a query: the data must be openly licensed, recent, offered as CSV, or confirmed by more than one source. Putting those constraints in the query does not work, because every source treats them as more words to match. Each source has its own filter syntax or none at all: CKAN takes Solr `fq` clauses, NASA CMR an `updated_since` parameter, ArcGIS Hub a CQL `filter`, Dataverse `fq` over its own Solr fields, and most of the 76 sources nothing. The fields disagree too: licenses arrive as SPDX ids, Creative Commons deed URLs, EU vocabulary URIs or free text ("CC-By Attribution 4.0 International"), and a record's date is a different field on every source.

## Decision

- `search` takes `--license`, `--updated-since`, `--format` and `--min-sources`, and applies them to the merged, ranked list before `--limit` cuts it. A merged dataset passes on a field any of its sources stated. A dataset that does not state a field never passes a filter on it.
- Every source's license is read as an SPDX id at the source boundary (`src/license.rs`): `CC-BY-4.0` however the source spelled it, placeholders such as `notspecified` dropped, anything unrecognized kept as written. `--license cc-by` matches the family, every version and port of CC BY but not CC BY-SA.
- `--updated-since` takes a year, a month or a day and compares at the precision a record states, so a dataset dated only `2024` counts as updated after `2024-06-01`.
- File formats are read as one name per format (`CSV`, `NETCDF`, `GEOTIFF`) from the sources that list them: CKAN portals, data.europa.eu, ArcGIS Hub, NASA CMR and data.gov.
- With any filter set, each source is asked for 30 results instead of 10, since most of what comes back is dropped. `--per-source` still overrides it.
- A source is handed a filter only where its own filter is never stricter than dataseek's, so it drops nothing the merged-list filter would keep. Today that is `--updated-since` on CKAN (`metadata_modified`), CMR (`updated_since`) and ArcGIS Hub (`modified`). The query cache key includes the day.
- `--json` reports `filtered_out`, the merged datasets the filters dropped, and `total` counts the ones that passed.

## Consequences

- One filter means the same thing for every source, whatever its API offers, and a source with no filter at all is filtered just as well.
- A filter cannot find what a source did not return: a search for recent CSV files sees only the 30 best matches of each source, filtered. A source-side filter widens that window only where it is safe.
- Licenses are sparse: 315 of 978 merged results over three queries name one, so a license filter drops most of a search. The summary line says how many matched without the filters, so an empty result is not mistaken for no data.
- Pushing a filter down to another source needs evidence that its field is never stricter than the one dataseek reads, recorded in that adapter's header.

## Evidence

Probes of the live sources, October 2026, recorded in the adapter headers:

- CKAN: across 600 records on data.gov.uk, Open Canada and data.gov.au, no record's displayed date was later than its `metadata_modified`. All six portals accept the range in `fq`. `res_format` matches the publisher's free text exactly ("CSV", "csv", ".csv"), so the format stays on the merged list.
- CMR: in 200 "sea ice" collections no `DataDates` update was later than the revision date `updated_since` compares; the parameter cut that query from 3,387 matches to 957.
- ArcGIS Hub: the CQL filter on `modified` compares the modification date, although the queryables list gives it the path `properties.created`. Of 100 filtered items, 13 were created before the cut-off and none modified before it.
- Dataverse is left out: `dateSort`, the only date field `fq` reaches, is the publication date, and 2 of the 54 Harvard records updated since 2024 were published earlier, so it would drop them.
- data.europa.eu is left out: `minDate` filters on `modified` by default, while dataseek falls back to `issued`, and 109 of 200 "air quality" records have no `modified`.
- Licenses: no source filters on SPDX ids, so no license filter is pushed down. Of 978 merged results for "sea surface temperature", "air quality" and "unemployment rate", 315 name a license.
