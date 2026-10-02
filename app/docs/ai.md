# Work AI (Amazon Bedrock)

Alethiometer's AI answers questions in the assistant pane and in Reflect ("Ask about today"), and suggests topics for Unsorted items. It runs on Amazon Bedrock through the **Converse API** (`POST https://bedrock-runtime.<region>.amazonaws.com/model/<modelId>/converse`), signed with AWS SigV4. Code: `src-tauri/src/ai/`.

It follows these boundary rules:

- **Allow-list, fail closed (rule 2).** Work data goes only to providers listed in `ai.allowed_providers`. If a tier's provider isn't on the list, or its model is empty, or credentials are missing, the call fails with an error and nothing is sent. There is no fallback provider.
- **US only, fail closed.** A Bedrock tier must use a commercial `us-*` region and a `us.` inference-profile ID. Anything else fails before credentials are read or a request is built (see [Region and routing](#region-and-routing)).
- **Nothing personal (rule 4).** Prompts are built only from `work`-tagged sources. Life blocks aren't sent at all, not even as busy times.
- **Suggests and answers only (rule 6).** The system prompt tells the model not to draft replies, notes or reflections. Topic suggestions are stored as `suggested`, never `confirmed`. Only you confirm them.

## Defaults

The models are now set in `settings.rs`, so the AI is on as soon as credentials exist:

| Tier | Used for | Model (inference profile) | Output cap per call |
|---|---|---|---|
| `small` | topic suggestions (short JSON answers) | `us.openai.gpt-6-luna` (OpenAI GPT-6 Luna, served by Bedrock) | 1,024 tokens |
| `large` | questions (assistant pane, Reflect) | `us.anthropic.claude-opus-5-5` (Claude Opus 5.5) | 16,000 tokens |

```json
"ai": {
  "allowed_providers": ["bedrock"],
  "region": "us-east-1",
  "aws_profile": "",
  "small": { "provider": "bedrock", "model": "us.openai.gpt-6-luna" },
  "large": { "provider": "bedrock", "model": "us.anthropic.claude-opus-5-5" }
}
```

**BAA.** You've confirmed that your AWS Business Associate Addendum covers Amazon Bedrock in US regions. `allowed_providers: ["bedrock"]` records that decision; Alethiometer can't check it. Also confirm that both models are HIPAA-eligible under your agreement: GPT-6 Luna is a third-party model served by Bedrock, not by OpenAI's own API.

**Output caps and reasoning.** Both models reason before they answer, and reasoning tokens count toward the cap.

- Opus 5.5 always thinks: thinking can't be turned off, and it defaults to effort `medium`. Questions pull context from several places, so the large tier allows 16,000 tokens. That follows Anthropic's guidance for non-streaming calls (about 16k, below HTTP timeouts). The earlier 4,096 could run out on a long reasoning pass. The HTTP timeout is 300 s to match. Bedrock reserves `maxTokens` against your output-token quota while a call runs. That doesn't matter for one user, but on a brand-new account with a small Opus quota you may see a 429.
- GPT-6 Luna runs at effort `low` by default here (the model's own default is `medium`). A topic suggestion is a small classification, so 1,024 tokens is plenty (the cap is in `ai/suggest.rs`).

If a model uses its whole cap on reasoning, you get "Bedrock returned no text (stop reason: max_tokens…)" rather than a blank answer.

## Region and routing

- `ai.region` must be a commercial US region (`us-east-1`, `us-east-2`, `us-west-2`, …). Otherwise every call fails with "ai.region '…' is not a US region … Nothing was sent". GovCloud (`us-gov-*`) is rejected too: it's a separate AWS partition with different endpoints and ARNs.
- Model IDs must start with `us.`. A `us.` inference profile keeps the request in US regions. `global.` profiles can run it in any commercial region, and `eu.`, `jp.` and others route abroad. Bare IDs (`anthropic.claude-opus-5-5`, `openai.gpt-6-luna`) aren't accepted either. Full ARNs aren't accepted, because Alethiometer can't tell where an application inference profile routes. The error names the `us.` ID to use instead.
- **A `us.` profile may run the request in any of several US regions**, not just `ai.region`. Bedrock picks one for capacity. The request is signed and sent to `bedrock-runtime.<ai.region>.amazonaws.com`, and AWS routes it on from there within the US. This is why the IAM policy below allows the foundation model in `us-*` regions and not only in your own region.

The check lives in `bedrock::check_route`. It runs in `ai::check` (so `ai.status` shows the error too) and again inside `bedrock::converse`.

## Setup

### 1. Enable model access

In the AWS console, open **Amazon Bedrock → Model access** in your region (for example `us-east-1`) and enable **Claude Opus 5.5** and **OpenAI GPT-6 Luna**. Access is per account. Without it, Bedrock returns 403 and Alethiometer shows "model access not enabled in Bedrock console for <region>". Then check **Bedrock → Cross-region inference** and confirm both `us.anthropic.claude-opus-5-5` and `us.openai.gpt-6-luna` are listed there.

### 2. Credentials: a Bedrock API key (recommended) or a long-lived IAM access key

The simplest setup is a **Bedrock API key**: a bearer token, no request signing needed. In the AWS console: **Bedrock → API keys → Long-term API keys → Generate**. Store it as:

| Secret name | Value |
|---|---|
| `bedrock_api_key` | the API key |

Or set it as `AWS_BEARER_TOKEN_BEDROCK` in the environment (matches AWS's own SDK examples). Either way, Alethiometer sends it as `Authorization: Bearer <token>` and never signs the request.

A long-term key is backed by an IAM user AWS creates for you; change its permissions from **API keys → Long-term API keys → select key → Manage in IAM Console**, or attach the policy in step 3 directly to that user.

**Alternative: an IAM user with a long-lived access key**, signed with SigV4, stored as:

| Secret name | Value |
|---|---|
| `aws_access_key_id` | the access key ID (`AKIA…`) |
| `aws_secret_access_key` | the secret access key |

The secret store is the **Keychain on the Mac** and `<data dir>/secrets.json` (mode 0600) on Linux dev. Note that the Keychain backend is still a TODO in `platform.rs`, so today the Mac uses the same file. Don't set `aws_session_token` for long-lived keys.

If nothing is found, every call fails with a message naming everywhere Alethiometer looked (bearer env, bearer secret, key env, key secret, then the `~/.aws` profile chain). See `docs/ai.md`.

Keys are read for every call. They are never logged, never put in settings.json, and never shown in errors. Rotate them from time to time.

**Other sources (kept, not the recommended path).** The resolver still walks a small credential chain, and the first match wins:

| # | Where | What |
|---|---|---|
| 1 | environment | `AWS_BEARER_TOKEN_BEDROCK` |
| 2 | Alethiometer secrets | `bedrock_api_key` |
| 3 | environment | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN` |
| 4 | Alethiometer secrets | `aws_access_key_id`, `aws_secret_access_key`, optional `aws_session_token` |
| 5 | `~/.aws/credentials` | static keys for the profile |
| 6 | `~/.aws/config` | static keys, or `credential_process`, for the profile |

The profile is `ai.aws_profile`. If that's empty, Alethiometer uses `AWS_PROFILE`, then `default`. Environment variables win over the secret store, so a stray `AWS_ACCESS_KEY_ID` in a dev shell overrides your stored key. Apps started from the Dock don't see shell variables. SSO isn't read directly: a profile can use `credential_process = aws configure export-credentials --profile <sso-profile> --format process`, and an SSO profile without one gets an error saying so.

### 3. IAM policy (minimal)

Attach this to the IAM user. Converse needs only `bedrock:InvokeModel`. It applies to both the inference profile and the foundation model behind it, in every region the profile can route to. Replace `<account-id>`, and replace `us-east-1` with `ai.region`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AlethiometerInferenceProfiles",
      "Effect": "Allow",
      "Action": "bedrock:InvokeModel",
      "Resource": [
        "arn:aws:bedrock:us-east-1:<account-id>:inference-profile/us.anthropic.claude-opus-5-5",
        "arn:aws:bedrock:us-east-1:<account-id>:inference-profile/us.openai.gpt-6-luna"
      ]
    },
    {
      "Sid": "AlethiometerFoundationModelsUS",
      "Effect": "Allow",
      "Action": "bedrock:InvokeModel",
      "Resource": [
        "arn:aws:bedrock:us-*::foundation-model/anthropic.claude-opus-5-5",
        "arn:aws:bedrock:us-*::foundation-model/openai.gpt-6-luna"
      ],
      "Condition": {
        "StringLike": {
          "bedrock:InferenceProfileArn": [
            "arn:aws:bedrock:us-east-1:<account-id>:inference-profile/us.anthropic.claude-opus-5-5",
            "arn:aws:bedrock:us-east-1:<account-id>:inference-profile/us.openai.gpt-6-luna"
          ]
        }
      }
    }
  ]
}
```

- The inference-profile ARN is in `ai.region`, the region you call.
- The foundation-model ARNs use `us-*` because a `us.` profile can run the request in several US regions (commonly `us-east-1`, `us-east-2` and `us-west-2`; the exact set is on the profile under **Cross-region inference**). The wildcard keeps the grant inside US regions. If you'd rather list regions explicitly, copy them from the profile's details.
- The condition lets the foundation models be used only through these two profiles, so the key can't call them directly. If a 403 names this condition, you can drop it; the grant still stays within `us-*`.
- The foundation-model IDs are the profile IDs without `us.` (`anthropic.claude-opus-5-5`, `openai.gpt-6-luna`). That's AWS's usual naming, but it hasn't been checked against a live account. If you get a 403 that names a different foundation-model ARN, copy that ARN into the policy.
- Nothing else is needed. Alethiometer doesn't stream, so it doesn't need `bedrock:InvokeModelWithResponseStream`. It makes no list or describe calls and uses no S3. It never calls the `bedrock-mantle` endpoint, so it needs no `bedrock-mantle:*` permissions.

### 4. Check it

Restart Alethiometer. The assistant pane header shows `bedrock · us.anthropic.claude-opus-5-5`. `ai.status` reports each tier's config check, including the US-routing check, without calling AWS.

## How the two model families are called

Both families use the same Converse body:

```json
{
  "system": [{ "text": "<system prompt>" }],
  "messages": [{ "role": "user", "content": [{ "text": "<context + question>" }] }],
  "inferenceConfig": { "maxTokens": 16000 }
}
```

The family comes from an explicit prefix map (`bedrock::family`): `us.anthropic.` → Anthropic, `us.openai.` → OpenAI. Any other `us.` vendor fails closed ("… doesn't know the request shape for …").

| | `us.anthropic.*` (Opus 5.5) | `us.openai.*` (GPT-6 Luna) |
|---|---|---|
| `system` block, `inferenceConfig.maxTokens` | yes | yes |
| `temperature`, `topP`, `stopSequences` | rejected (no sampling parameters on Opus 5.5); never sent | rejected by Converse for GPT-6; never sent |
| reasoning in the response | a `reasoningContent` block with `reasoningText` (empty by default), before the `text` block | a `reasoningContent` block with opaque `redactedContent`, before the `text` block |
| effort field (`additionalModelRequestFields`) | `{"output_config": {"effort": "low\|medium\|high"}}` | `{"reasoning": {"effort": "low\|medium\|high"}}` (the flat `reasoning_effort` is rejected with 400) |
| default effort | medium | medium |

The answer is the concatenation of the `text` blocks only. Reasoning blocks are ignored, and a reply with no text is an error. Calls are single-turn, so reasoning is never sent back.

### Effort setting

Each tier takes an optional `effort`: `"low"`, `"medium"`, `"high"`, or `""` for the model's default. Anything else fails closed. Defaults: small (GPT-6 Luna) `low`, large (Opus 5.5) the model's default (`medium`).

```json
"small": { "provider": "bedrock", "model": "us.openai.gpt-6-luna", "effort": "low" },
"large": { "provider": "bedrock", "model": "us.anthropic.claude-opus-5-5", "effort": "" }
```

## What each feature sends

Everything goes in one Converse request per call, over HTTPS to `bedrock-runtime.<region>.amazonaws.com` in a US region. Alethiometer stores nothing on the AWS side.

| Feature | Command | Tier / model | Sent to Bedrock |
|---|---|---|---|
| Ask about a topic (assistant pane with a topic open) | `ai.ask {question, topic_id}` | large / Opus 5.5 | The system prompt. The question. The topic name. The topic's Obsidian note (first 6,000 chars). The topic's new signals still in Incoming (suggested, not confirmed), marked as such, then confirmed work-tagged items linked to the topic (open tasks first, then newest first). For each: source, kind, time, status, due date, owner, title, summary, first 600 chars of the body, and the last 6 messages (300 chars each). |
| Ask about today (Reflect, or the assistant pane with no topic) | `ai.ask {question}` or `scope: "today"` | large / Opus 5.5 | The system prompt. The question. Today's work calendar events (time, title, attendees, tentative). Work items that happened or got a message today, with the same fields as above. Life blocks aren't sent. |
| Assistant with Glean: queries | `ai.ask_glean {question, topic_id?, off?, on?}` | small / GPT-6 Luna | The system prompt (write up to `queries_per_ask` Glean queries in Glean's syntax). The question. The "Sent with your question" chips that are on: topic title, its people's names, Jira keys, channels, up to 10 recent signal titles (confirmed, plus suggested ones still in Incoming), and the note (first 3,000 chars) only if you added it. |
| Assistant with Glean: answer | same call | large / Opus 5.5 | The system prompt (answer only from numbered evidence, cite by number, people ranked when asked about people). The chips that are on. Up to 12 kept evidence items, numbered: for Glean results title, system, container, the people Glean names and its snippet; for local work items title, source, date, summary and the first 500 chars of the body. The question. |
| Topic suggestions | `ai.suggest_topics {limit}` | small / GPT-6 Luna | The system prompt (existing topic, a new 2-5 word one, or none). For each Unsorted item (up to `limit`, default 10, max 50): source, kind, title and summary (no body), and its stored messages (time, author, first 240 chars each: numbered when the item can be split, at most 30; otherwise the last 10). All active topic names with a one-line gist (the first lines of each topic's note). The names of topics you marked wrong for that item, as "NOT about". |

Context is capped at 24,000 characters per question, newest first. Items from `personal` sources are filtered out in SQL and checked again in code. Tests plant personal rows straight into the database to prove they never reach a request.

`ai.ask` returns `{answer, used}`, where `used` lists the item IDs that were in the context, so the UI can cite them. Answers are plain text.

## Assistant with Glean

The pane keeps one conversation per topic (and one per view without a topic) while the app is open; switching away and back brings it back, and ↺ in the header starts over. Nothing is written to disk.

When a Glean source is on, the Assistant pane (440px, design hi-fi 3a and 3d) uses `ai.ask_glean` instead of `ai.ask`:

1. The small model turns the question and the chips into a few Glean queries. Without a small tier, the question is the query.
2. Glean runs each one. The local route adds the topic's own signals and the work items nearest the question (embeddings, else full text).
3. The local check from anchored search (`docs/sources/glean.md`) drops Glean results that aren't about the topic. Pins stay, rejects go, and results outside the window go too.
4. The large model answers from the kept evidence only (at most 12). Citations to numbers that weren't given are removed. For people questions it also returns a ranked list (ownership › code › review › management) with a role, one sentence of why and evidence numbers.

The reply lists the queries (with their marks and returned → kept), every source kept or dropped with its reason, the people, and the chips. If Glean can't answer (not set up, no token, daily limit, down), the answer comes from local evidence and says so. Both tiers are checked against the allow-list before anything is read or sent; the large one first.

## Local embeddings

`embed/`: BAAI/bge-small-en-v1.5 (ONNX, 384 dimensions, CLS pooling, L2-normalised), run in-process with tract, a pure-Rust ONNX runtime. Nothing is sent anywhere. On first run the model (`onnx/model.onnx`, about 130 MB, and `tokenizer.json`) downloads from huggingface.co into `<data dir>/models/bge-small-en-v1.5/`. Settings → AI shows the state, with Download/Retry if it failed. Vectors live in SQLite through sqlite-vec (`vec_items`, `vec_topics`). A background pass embeds work items (title, summary, start of body) and topics (name, recent signal titles, start of the note) and re-embeds whatever changed. Personal sources are never embedded.

Used by the off-topic check (similarity to the topic) and the Assistant's local route. Until the model is ready, those fall back to entity matching and full-text search.

Tested here against a tiny generated model with the same interface (`tests/fixtures/embed/make.py`). The real download and model weren't reachable from the build environment, so the first real run is on the Mac.

## Topic suggestions

`ai.suggest_topics` looks at new Unsorted items (unseen, from work sources, with no topic link other than `wrong`). For each one it asks the small model for the best existing topic, a new one, or none. It stores a `topic_links` row with state `suggested`, the model's `confidence`, and a short `reason`. Rules:

- It never writes `confirmed`. It never overwrites an existing row, so a `wrong` verdict always stands.
- Topics you marked wrong for an item are left out of that item's list and named as "NOT about". If the model picks one anyway, the pick is dropped.
- Picks below confidence 0.4, or of an unknown topic ID, are dropped. The item stays in Unsorted.
- **New topics.** Instead of an existing `topic_id`, the model may propose `new_topic`: a 2 to 5 word name, meant to be broad enough to hold future related items rather than describe this one event. Anything outside 2–5 words is dropped (treated as no suggestion). A proposal reuses an existing active topic of the same name (case-insensitive) rather than duplicating it; otherwise a new `topics` row is created (category `work`, no Obsidian note) and linked as `suggested` like any other pick, so you still confirm it yourself. Only available in the whole-item shape, not in split groups (below).
- **Splits by topic.** An item with 2 to 30 messages (a DM's unread run, new thread replies) isn't assumed to be one subject. The model may answer with groups (`{"groups": [{"messages": [1, 2], "topic_id": …, "confidence": …, "reason": …}]}`) instead of one topic. New topics aren't supported in this shape (each group picks an existing topic or none) — an item needing a genuinely new topic and a split should get the whole-item shape on a later run once it's down to one message, or you can split it manually. The item is split (`queries/split.rs`) only when at least two groups hold up: each needs confidence 0.7 (a one-message group 0.85), messages in weaker groups or left out stay with the message before them, groups with the same topic (or both without one) merge, and more than 4 groups or a bad or repeated message number means no split. The group holding the newest message stays on the item; each other group becomes a child item (`<external id>#<n>`) with its own `suggested` link, or none (then it sits in Unsorted). Children inherit the item's `wrong` topics. With fewer than two groups left, the item gets a normal whole-item suggestion. Only Unsorted items are considered; an item that already has a suggestion isn't re-asked, and a split child isn't split again. The result lists `split: [{item_id, children}]`.
- If the AI isn't configured, it returns `{status: "ai_off", message}` and does nothing. If a call fails (for example missing credentials or throttling), it stops and returns `{status: "failed", message}` with whatever it already stored. It never returns an error.

**Scheduling.** Runs after every sync (`sync::start`'s poll loop, and `sync.suggest`, which the UI calls after a manual `sync.now` has refreshed Incoming), outside the DB lock, regardless of whether that sync brought new items — so it also catches a backlog, not just freshly-synced items. Every run, from a sync or from `ai.suggest_topics` directly, records its outcome in `ai_suggest_run` (status, message, suggested count, time, last success). `ai.suggestions` returns that plus how many signals wait in Unsorted; the sync chip shows it (design 13b): the dot turns orange and the chip says "suggestions failed", and the popover's **AI · Bedrock** section has the state, the error and **Retry**.

Each Unsorted item costs one small-model call of roughly 300 input and 30 output tokens, plus up to about 2,500 input tokens for an item with many messages (and ~150 output tokens for a split answer). Items that get a suggestion leave Unsorted, so each is asked about only once. Items that get no pick are asked again on later runs. If that repeats too often, track "last asked" (open question below).

## Token usage

Every call Bedrock answers records its `usage` (tier, model, input and output tokens) in the `ai_usage` table, including calls that came back without text (for example when reasoning used up the whole budget). Failed requests (throttling, auth, network) aren't counted; Bedrock reports no usage for them. The sync status popover in the tab bar shows today's calls and tokens per tier; hover for the last 7 days. `ai.usage` returns the same numbers. With `ai.prices` in `settings.json` (model id → US dollars per million tokens, `{"input": 4.40, "output": 22.00}`), the popover also shows the cost; a model without a price shows tokens only. Alethiometer sends no cache points, so there are no cache reads or writes to price, and no call comes near GPT-6 Luna's long-context rate (a topic suggestion is a few thousand tokens). Bedrock's own numbers are in CloudWatch (`AWS/Bedrock`, `InputTokenCount` / `OutputTokenCount` per model) and Cost Explorer.

## Errors you may see

| Message starts with | Meaning |
|---|---|
| `AI provider 'x' is not on the allow-list` | That tier's provider isn't in `allowed_providers`. Nothing was sent. |
| `The Large tier is off in Settings` | That tier's toggle is off (`ai.large.enabled`). Nothing was sent. |
| `No model set for the Large tier` | The model is empty. The AI is off. |
| `ai.region '…' is not a US region` / `… is AWS GovCloud` | Set `ai.region` to a commercial `us-*` region. Nothing was sent. |
| `Model '…' is not a US inference profile` | Use the `us.` ID the message names. Nothing was sent. |
| `Model '…' is from a vendor Alethiometer doesn't know` | Only `us.anthropic.*` and `us.openai.*` are supported. |
| `No AWS credentials found for Bedrock. Store a Bedrock API key …` | Add `bedrock_api_key` (or an IAM access key pair) to the secret store. |
| `Bedrock denied access … model access not enabled in Bedrock console for <region>` | 403. Enable model access, or fix the IAM policy (a foundation-model ARN in a routed region is the usual gap). |
| `Bedrock rejected the AWS credentials` | 403: wrong, disabled or rotated key. |
| `Bedrock needs an inference-profile id` | 400. The profile ID isn't known in that region; check Cross-region inference. |
| `Bedrock rejected the request (400)` | Anything else the model or Converse refused. AWS's text follows. |
| `Bedrock returned no text (stop reason: max_tokens…)` | The model used its whole budget, usually on reasoning. |
| `Bedrock is throttling requests` | 429: a quota limit (including the `maxTokens` reservation) or the model isn't ready. Retry later. |
| `Bedrock had a temporary problem` | 408, 424 or 5xx. |

## Verified and not verified

There were no AWS credentials in the build environment, so no live call was made. AWS's documentation sites (`docs.aws.amazon.com`, `aws.amazon.com`) were blocked from the build environment, so the GPT-6 facts come from AWS pages seen only as search-result snippets and from third-party reports of live calls.

Checked offline:

- The SigV4 signature matches the AWS SigV4 test suite (`get-vanilla`) and botocore 1.43 on the exact Converse request shape.
- Request and response JSON match the botocore `bedrock-runtime` service model. Tests cover an OpenAI-shaped reply with `redactedContent` reasoning, a Claude reply with `reasoningText`, the non-US region and non-`us.` ID refusals, and the missing-credentials message.

From secondary sources (Sept 2026):

- GPT-6 Luna on Bedrock: IDs `us.openai.gpt-6-luna` / `global.openai.gpt-6-luna`, effort `none`…`max` with default `medium`, 1M context (AWS model card and launch post, via search snippets). A live `us-east-1` Converse reply with `reasoningContent.redactedContent` then `text` (langfuse#17857). Converse rejects `temperature`, `topP` and `stopSequences` for GPT-6 (aws-samples/bedrock-chat#1161, langflow#15349). `additionalModelRequestFields.reasoning.effort` is honored and flat `reasoning_effort` is rejected (vercel/ai#21207, NousResearch/hermes-agent#122117, both from live calls). A Converse call to GPT-6 Luna with a `system` block and `inferenceConfig.maxTokens` succeeded in llama_index#23212 (Luna listed as "every call also succeeds").
- Opus 5.5 on Bedrock: inference-profile-only (`us.anthropic.claude-opus-5-5`), no sampling parameters (aws-samples/bedrock-chat#1160). Effort via `additionalModelRequestFields.output_config.effort` on Converse (AWS "Adaptive thinking" user-guide page, via search snippet; documented for Opus 4.x and assumed for 5.5). Opus 5.5 thinking can't be disabled and defaults to effort `medium` (Anthropic's model docs).

Still to confirm on the real account:

- the exact US regions each profile routes to;
- the foundation-model ARNs (`anthropic.claude-opus-5-5`, `openai.gpt-6-luna`) and whether the `bedrock:InferenceProfileArn` condition is accepted as written;
- whether GPT-6 Luna needs more than 1,024 tokens for a suggestion at `medium` effort;
- HIPAA eligibility of GPT-6 Luna under your BAA;
- real error message texts.
