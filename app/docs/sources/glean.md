# Glean

Glean is a **search-only** source. Alethiometer never polls it. Nothing it returns becomes an item, shows up in Incoming, or goes into the local index. ⌘K, the Search view and the Assistant ask it when you search. Results stay in an in-memory cache for `cache_minutes`. The only Glean-related things stored are the per-day call count (`glean_calls`) and your pins and rejects per topic (`search_feedback`).

Design: Glean hi-fi round 3 (`project/Glean Hi-fi.dc.html`: 3a Sources panel, 3b/3c ⌘K anchor, 3d owner list), built on rounds 1 and 2 where round 3 doesn't redraw something.

## Setup

1. Get a Glean API token with search scope (Glean admin → API tokens, or a user-scoped token if your org allows it). For a global token, also set `act_as` to your work email.
2. Settings → Sources → Add source → Glean. Set **Server** to your Glean API server, e.g. `https://acme-be.glean.com`.
3. Set the token: Secrets → `glean.token` → Set…

Or by hand in `settings.json`:

```json
{ "kind": "glean", "tag": "work", "name": "Glean", "enabled": true,
  "config": { "server_url": "https://acme-be.glean.com" } }
```

and put the token in `secrets.json` as `glean.token`.

## Config

| Key | Default | Meaning |
|---|---|---|
| `server_url` | none | Glean API server. Must be `https://` (plain `http://` only to 127.0.0.1/localhost, for a local stand-in). |
| `act_as` | none | `X-Glean-ActAs` for a global token. |
| `token_secret` | `glean.token` | Secret holding the API token. |
| `datasources` | all | Only these datasources (`datasourcesFilter`). |
| `exclude_datasources` | none | Never these (one negated `datasource` facet filter each), e.g. `slack` if Slack is read directly. |
| `window_days` | 180 | Results older than this are dropped ("outside 180d"). |
| `min_similarity` | 0.62 | Anchored search: a Glean result less similar than this to the topic is hidden. |
| `results_per_query` | 30 | `pageSize`. |
| `queries_per_ask` | 3 | Glean queries the Assistant runs per question. |
| `cache_minutes` | 15 | Same query within this long is answered from memory. |
| `calls_per_day` | 400 | Hard stop. Settings shows today's count. |
| `timeout_secs` | 8 | Per call. |

## What's sent

Only query text: what you type in ⌘K, or what the small model writes from your question and the Assistant's "Sent with your question" chips (topic title, people, Jira keys, channels, recent signal titles, and the topic note only if you add it). All of it comes from work sources. Nothing from a personal source is ever in a query.

Glean isn't an LLM endpoint, so it isn't gated by `ai.allowed_providers`. See ISSUES.md.

## API

`POST {server_url}/rest/api/v1/search` with `Authorization: Bearer <token>` and the request and response shapes from Glean's published Client API OpenAPI spec (`SearchRequest`, `SearchResponse`; `results[].document.metadata` for author, owner, assignee, update time, container). 401/403 says to replace the token; 429 says to wait.

## Hydrated search (⌘K, Search view)

- Local results appear as you type. **⌥↵** asks Glean for the current query. When Glean is asked is still an open question (ISSUES.md).
- A Glean result with the same URL as a local item merges into it: one row, both routes.
- **Anchor** (⌥A, ⌘K only): the topic you're in. Local items stay when they're filed under it or close to it (embeddings). Glean results stay only with **both** a shared entity (a person, Jira key, channel, link or repo of the topic) **and** similarity ≥ `min_similarity`. Shared words alone never keep one. Until the embedding model is ready, the entity check alone decides. Pins always stay and come first; rejects are dropped. Hidden rows sit behind "N off-topic hidden" (⇧↵ shows them, greyed, with the reason).
- The facet bar groups each route's sources. Hover highlights, click filters, ⌥click excludes.
- ⌘T links the selected result to the topic. For a local item that files it there. A Glean result isn't stored, so linking **pins** it to the topic.
- ⌘⌫ rejects the selected result for the anchored topic (again on a dropped row clears it).

## Assistant

See `docs/ai.md` → "Assistant with Glean".

## Tested

Against recorded responses shaped like the published spec (`tests/fixtures/glean/search_renewal.json`), plus a local stand-in server for screenshots. Not yet run against a real Glean instance.
