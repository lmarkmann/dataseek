# Related

Verified against `src/related.rs`, `dsk help related --json` and live runs on 2026-11-23. Not yet measured with skill evals.

`related` lists the datasets beside one item, read from the item's own host. It is a walk from something already found, not search: one host is asked, nothing is merged, and the order is the host's.

## Arguments

| MCP argument | CLI | Meaning |
|---|---|---|
| `url` (string, required) | the URL after `related` | the item's page; the table below lists the pages it reads |
| `by` (`owner` or `forks`) | `--by` | which neighbors a GitHub repository lists; `owner` is the default, `forks` works on a GitHub repository only |
| `limit` (integer) | `-n`, `--limit` | how many datasets to return, 100 by default |
| `offline` (boolean) | `--offline` | fail at once instead of reaching for the network |

## Which pages it reads

| The URL | What comes back | `relation` |
|---|---|---|
| a Hugging Face dataset page, `huggingface.co/datasets/OWNER/NAME`, or an owner page, `huggingface.co/OWNER` | the owner's other datasets, the most downloaded first | `owner` |
| a Hugging Face paper page, `huggingface.co/papers/ID`, or an arXiv link, `arxiv.org/abs/ID` | the Hub datasets that cite the paper | `paper` |
| a Hugging Face model page, `huggingface.co/OWNER/MODEL` | the datasets the model's card names; each record holds the id and the link only | `model` |
| a Hugging Face collection page | the datasets of the collection | `collection` |
| a GitHub repository or owner page | the owner's repositories, the last pushed first; code repositories are among them | `owner` |
| a GitHub repository with `by` set to `forks` | its forks, the most starred first | `forks` |
| a GitLab project page | the other projects of its group or user, the most starred first | `owner` |

Any other page is refused with `is not a page related reads`. A Zenodo record, a DOI link or a Kaggle page has no resolver: search for its title instead.

## The result

Schema `dataseek-related/1`, five keys:

| Key | What it holds |
|---|---|
| `schema` | `dataseek-related/1` |
| `url` | the URL as it was passed |
| `source` | the id of the host that answered: `huggingface`, `github` or `gitlab` |
| `relation` | `owner`, `paper`, `model`, `collection` or `forks` |
| `results` | the datasets, each with the keys a `search` result has (`title`, `url`, and where the host gave them `description`, `publisher`, `license`, `updated`, `size_bytes`, `popularity`, `doi`), without `sources` and `score` |

## Limits

- A list is one page of the host: at most 1,000 datasets from Hugging Face and 100 repositories from GitHub or GitLab. A list of exactly that length may be cut.
- The item the URL names is not in its own list.
- Nothing is cached, so every call is a request. GitHub allows 60 an hour without `GITHUB_TOKEN` and GitLab 60 an hour; a GitLab call sends two.
- A record of the `model` relation has no description or license. Call `inspect` with its `url` for the rest.

## The answer shape

**Neighbors**: numbered, in the order returned. Each carries the title and the URL, then license, last update and size when the record has them. **Source**: one line with the `relation`, the `source`, how many came back and the `limit`, and the `dsk related ...` command that reproduces the call.
