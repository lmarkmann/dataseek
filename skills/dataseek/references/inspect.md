# Inspect

Verified against `src/inspect.rs`, `dsk help inspect --json` and live runs on 2026-10-20.

`inspect` reads what one dataset page says about itself and the files it lists. It is enrichment for one result, not search: it reads one page and never fans out.

## Arguments

| MCP argument | CLI | Meaning |
|---|---|---|
| `url` (string, required) | the URL after `inspect` | the dataset's landing page, normally a search result's `url` |
| `offline` (boolean) | `--offline` | fail at once instead of reaching for the network |

## What it reads

- **Most pages:** every `application/ld+json` block holding a schema.org `Dataset`, at the top level, in an array or inside `@graph`.
- **Hugging Face:** the Hub's Croissant endpoint, because the page's HTML carries no metadata. Croissant adds `recordSet` (the columns) beside the schema.org fields.
- **Dataverse:** a `dataset.xhtml?persistentId=...` page is read through the installation's schema.org export, which answers where the page itself sits behind a bot wall (Harvard).
- **doi.org links:** `inspect` looks up where the DOI points and reads that page, so a result whose `url` is a DOI needs no extra step.
- **Files:** the dataset's `distribution`, schema.org `DataDownload` and Croissant `FileObject` and `FileSet` entries. Nothing is downloaded; a size, format or checksum is whatever the page declares.

## The result

Schema `dataseek-inspect/1`, two keys:

| Key | What it holds |
|---|---|
| `dataset` | the page's own JSON-LD node as the page wrote it: commonly `name`, `description`, `creator` or `author`, `publisher`, `license`, `datePublished`, `dateModified`, `identifier`, `keywords`, `distribution`, `size` or `contentSize` |
| `files` | dataseek's reading of the file list; each entry has whichever of `name`, `format`, `size_bytes`, `checksum` (`sha256:<hex>` or `md5:<hex>`), `url` and, for a Croissant file set, `includes` (the glob its files match, such as `plain_text/*/*.parquet`) the page gave |

Keys in `dataset` vary by publisher and may carry a `schema:` prefix; read what is there rather than expecting a fixed set. An empty `files` list means the page declares no files, not that the dataset has none.

## Which pages publish markup

Observed 2026-10-20 on landing pages from an `air quality` search, one per source. Publishers change their pages, so treat this as the expected outcome, not a guarantee:

| Outcome | Sources whose result URLs gave it |
|---|---|
| metadata and files | `zenodo`, `huggingface`, `kaggle`, `datagov`, `data-gov-au`, `harvard-dataverse` and the other Dataverse installations, `aws`, `cessda`, `dbnomics`, `modelscope`, `openaire` |
| `carries no schema.org Dataset markup` | `arcgis`, `b2find`, `dataone`, `earth-engine`, `europa`, `github`, `govdata`, `open-canada`, `opendatasoft`, `openml`, `osf` |
| `sent a challenge page instead of results` | `gbif`, and the AWS bot wall in front of `hdx` and `figshare` pages |

`cmr` results now link the provider's landing page or the DOI rather than NASA's record page, so whether they publish markup depends on that provider.

## When it fails

1. **`carries no schema.org Dataset markup`** with a `doi` on the result and a `url` that is not a doi.org link: inspect `https://doi.org/<doi>` once. `inspect` reads the publisher's canonical page the DOI names; a DOI that lands behind a bot wall fails as in step 3.
2. **Still no markup:** say the page publishes no machine-readable metadata, give its URL, and list what the search result itself carried. Read the page with a web fetch tool only if the user wants more.
3. **`cannot fetch`** with `sent a challenge page instead of results`: the site blocks automated requests; give the URL for the user to open.
4. **A file URL or a search page** handed to `inspect` fails the same way as a page with no markup; pass the landing page.

## After inspect

dataseek downloads nothing. When the user wants a file, hand them its `url` with `size_bytes` and `format`, and, if they ask you to fetch it, check the size first and verify `checksum` after the download. Reading rows or columns inside the dataset is the archive's own API, not dataseek.

## The answer

```text
Metadata
Evaluation of the influence of rain on air surface temperature measurements
https://zenodo.org/records/13135140
Publisher: Zenodo | License: CC BY 4.0 | Published 2024-07-30 | 8.19 MB

Files (3)
1. text/csv  https://zenodo.org/api/records/13135140/files/Dataset_reference.csv/content
2. ...

Not stated: file names apart from the links, sizes per file, checksums
```

**Not stated** names the fields the user asked about that the page does not carry, so a missing license reads as missing rather than as overlooked.
