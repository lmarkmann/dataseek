# ADR 0021: `related` reads one item's neighbors from the item's own host

- Status: proposed
- Date: 2026-11-23

## Context

A search answers a query. A hunt for every dataset on a subject also walks from an item it already holds to the items beside it: the other datasets of the same owner, the datasets that cite a paper, the datasets a model was trained on, the datasets of a collection, the forks of a repository. No query finds these lists. An owner's datasets share no word, and a fork has the name of its parent. The host of the item answers each of them in one call, but in its own JSON, so today the calls are made by hand and their answers are not in the record shape `search` prints.

## Decision

- `dsk related <URL>` prints the datasets beside the item a URL names, as records in the shape `search` prints. One host is asked. Nothing fans out, nothing is merged, and dataseek does not rank: the order is the host's.
- The URL's host and path choose one resolver, as in `inspect`:

| URL | the neighbors | the call |
|---|---|---|
| Hugging Face dataset page or owner page | the owner's datasets, the most downloaded first | `GET /api/datasets?author=OWNER` |
| Hugging Face paper page, or an arXiv link | the datasets that cite the paper | the `datasets` of `GET /api/arxiv/ID/repos` |
| Hugging Face model page | the datasets the model's card names | the `dataset:` tags of `GET /api/models/ID` |
| Hugging Face collection page | the datasets of the collection | the items of type `dataset` of `GET /api/collections/SLUG` |
| GitHub repository or owner page | the owner's repositories, the last pushed first; with `--by forks`, the forks of the repository, the most starred first | `GET /users/OWNER/repos`, `GET /repos/OWNER/REPO/forks` |
| GitLab project page | the projects of the project's group or user, the most starred first | `GET /projects/PATH`, then `GET /groups/PATH/projects` or `GET /users/NAME/projects` |

- `--by owner|forks` exists because a GitHub repository URL has two relations. Every other URL has one, and `--by forks` on it is an error that names the pages it works on.
- A call sends one request, two for GitLab, where the first one reads whether the project belongs to a group or a user. The answer is one page: at most 1,000 datasets from the Hub and 100 repositories from GitHub or GitLab. `--limit` (default 100) cuts the list.
- The item the URL names is left out of its own neighbors.
- Each row is mapped by the function the source's adapter uses, so the same JSON row is the same record in `search` and in `related`.
- A model's tags carry dataset ids and nothing else, so those records hold the id and the link. `inspect` reads the rest of one of them.
- Nothing is cached. As with `inspect`, the call is made once, on demand, for one item.
- A GitHub owner listing is not narrowed to repositories tagged `dataset`. The user named the item, and many data repositories carry no topic.
- `HF_TOKEN` and `GITHUB_TOKEN` are sent where they are set, as in `search`.
- `--json` prints `schema` (`dataseek-related/1`), `url`, `source`, `relation` and `results`.
- The MCP server gets a fourth tool, `related`, with its schema generated from the command's help as for the other three ([ADR 0015](0015-mcp-server-over-stdio.md)).

## Consequences

- An agent or a user walks from one item to its neighbors with one command and reads them like search results.
- A new relation is one resolver: one URL pattern, one call, one mapping function that mostly exists already.
- The lists are not complete past one page. An owner with more than 1,000 Hub datasets or 100 repositories is cut at that number, in the host's order.
- A listing shows whatever the owner holds, so a GitHub owner listing contains code repositories beside the data.
- Without a cache a repeated call is a repeated request. GitHub allows 60 anonymous requests an hour on these endpoints, which is enough for a walk by hand and little for a loop.

## Evidence

Probes of the live APIs on 2026-11-23:

- `GET /api/datasets?author=nyu-mll&limit=1000` on the Hub returns the owner's datasets as rows of the search listing.
- `GET /api/arxiv/1810.04805/repos` returns an object with `datasets`, `models` and `spaces`; each dataset is a listing row.
- `GET /api/models/distilbert/distilbert-base-uncased-finetuned-sst-2-english` carries the tags `dataset:sst2` and `dataset:glue`.
- A collection's `items` carry `type`, `id`, `author`, `downloads` and `lastModified`.
- `GET /users/OWNER/repos` and `GET /repos/OWNER/REPO/forks` on GitHub return repositories in the shape of a search item, and take `sort=pushed` and `sort=stargazers`.
- `GET /projects/restcountries%2Frestcountries` on GitLab names its namespace with `kind` (`group` or `user`) and `full_path`.
