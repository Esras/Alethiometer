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
- GPT-6 Luna also defaults to effort `medium`. A topic suggestion is a small classification, so 1,024 tokens is plenty (the cap is in `ai/suggest.rs`).

If a model uses its whole cap on reasoning, you get "Bedrock returned no text (stop reason: max_tokens…)" rather than a blank answer.

## Region and routing

- `ai.region` must be a commercial US region (`us-east-1`, `us-east-2`, `us-west-2`, …). Otherwise every call fails with "ai.region '…' is not a US region … Nothing was sent". GovCloud (`us-gov-*`) is rejected too: it's a separate AWS partition with different endpoints and ARNs.
- Model IDs must start with `us.`. A `us.` inference profile keeps the request in US regions. `global.` profiles can run it in any commercial region, and `eu.`, `jp.` and others route abroad. Bare IDs (`anthropic.claude-opus-5-5`, `openai.gpt-6-luna`) aren't accepted either. Full ARNs aren't accepted, because Alethiometer can't tell where an application inference profile routes. The error names the `us.` ID to use instead.
- **A `us.` profile may run the request in any of several US regions**, not just `ai.region`. Bedrock picks one for capacity. The request is signed and sent to `bedrock-runtime.<ai.region>.amazonaws.com`, and AWS routes it on from there within the US. This is why the IAM policy below allows the foundation model in `us-*` regions and not only in your own region.

The check lives in `bedrock::check_route`. It runs in `ai::check` (so `ai.status` shows the error too) and again inside `bedrock::converse`.

## Setup

### 1. Enable model access

In the AWS console, open **Amazon Bedrock → Model access** in your region (for example `us-east-1`) and enable **Claude Opus 5.5** and **OpenAI GPT-6 Luna**. Access is per account. Without it, Bedrock returns 403 and Alethiometer shows "model access not enabled in Bedrock console for <region>". Then check **Bedrock → Cross-region inference** and confirm both `us.anthropic.claude-opus-5-5` and `us.openai.gpt-6-luna` are listed there.

### 2. Credentials: long-lived IAM access keys

For now the supported setup is an **IAM user with a long-lived access key**, stored in Alethiometer's secret store:

| Secret name | Value |
|---|---|
| `aws_access_key_id` | the access key ID (`AKIA…`) |
| `aws_secret_access_key` | the secret access key |

The secret store is the **Keychain on the Mac** and `<data dir>/secrets.json` (mode 0600) on Linux dev. Note that the Keychain backend is still a TODO in `platform.rs`, so today the Mac uses the same file. Don't set `aws_session_token` for long-lived keys.

If neither is found, every call fails with:

> No AWS credentials found for Bedrock. Store your IAM access key in Alethiometer's secret store (Keychain on the Mac, secrets.json on Linux dev) as aws_access_key_id and aws_secret_access_key. (Also looked in AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY and AWS profile '…' in ~/.aws.) See docs/ai.md

Keys are read for every call. They are never logged, never put in settings.json, and never shown in errors. Rotate the key in IAM from time to time and update both secrets.

**Other sources (kept, not the recommended path).** The resolver still walks a small credential chain, and the first match wins:

| # | Where | What |
|---|---|---|
| 1 | environment | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN` |
| 2 | Alethiometer secrets | `aws_access_key_id`, `aws_secret_access_key`, optional `aws_session_token` |
| 3 | `~/.aws/credentials` | static keys for the profile |
| 4 | `~/.aws/config` | static keys, or `credential_process`, for the profile |

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

### Effort setting: status

The per-family wire mapping is implemented and tested (`bedrock::Effort`, `effort_fields`) for `low`, `medium` and `high`. **It isn't exposed in settings.json yet.** `AiTier` lives in `settings.rs`, which this change didn't touch. Until a `tier.effort` field is added, both tiers run at the model's default (`medium`), and no `additionalModelRequestFields` are sent. Wiring it is a small change: add `#[serde(default)] pub effort: String` to `AiTier`, then make `ai::tier_effort` return `bedrock::Effort::parse(&t.effort)`. When it lands, the settings will look like `"small": { "provider": "bedrock", "model": "us.openai.gpt-6-luna", "effort": "low" }`.

## What each feature sends

Everything goes in one Converse request per call, over HTTPS to `bedrock-runtime.<region>.amazonaws.com` in a US region. Alethiometer stores nothing on the AWS side.

| Feature | Command | Tier / model | Sent to Bedrock |
|---|---|---|---|
| Ask about a topic (assistant pane with a topic open) | `ai.ask {question, topic_id}` | large / Opus 5.5 | The system prompt. The question. The topic name. The topic's Obsidian note (first 6,000 chars). For confirmed, work-tagged items linked to the topic (open tasks first, then newest first): source, kind, time, status, due date, owner, title, summary, first 600 chars of the body, and the last 6 messages (300 chars each). |
| Ask about today (Reflect, or the assistant pane with no topic) | `ai.ask {question}` or `scope: "today"` | large / Opus 5.5 | The system prompt. The question. Today's work calendar events (time, title, attendees, tentative). Work items that happened or got a message today, with the same fields as above. Life blocks aren't sent. |
| Topic suggestions | `ai.suggest_topics {limit}` | small / GPT-6 Luna | The system prompt. For each Unsorted item (up to `limit`, default 10, max 50; one call per item): source, kind, title and summary (no body). All active topic names with a one-line gist (the first lines of each topic's note). The names of topics you marked wrong for that item, as "NOT about". |

Context is capped at 24,000 characters per question, newest first. Items from `personal` sources are filtered out in SQL and checked again in code. Tests plant personal rows straight into the database to prove they never reach a request.

`ai.ask` returns `{answer, used}`, where `used` lists the item IDs that were in the context, so the UI can cite them. Answers are plain text.

## Topic suggestions

`ai.suggest_topics` looks at new Unsorted items (unseen, from work sources, with no topic link other than `wrong`). For each one it asks the small model for the best existing topic or none. It stores a `topic_links` row with state `suggested`, the model's `confidence`, and a short `reason`. Rules:

- It never writes `confirmed`. It never overwrites an existing row, so a `wrong` verdict always stands.
- Topics you marked wrong for an item are left out of that item's list and named as "NOT about". If the model picks one anyway, the pick is dropped.
- Picks below confidence 0.4, or of an unknown topic ID, are dropped. The item stays in Unsorted.
- If the AI isn't configured, it returns `{status: "ai_off", message}` and does nothing. If a call fails (for example missing credentials or throttling), it stops and returns `{status: "failed", message}` with whatever it already stored. It never returns an error.

**Scheduling (not wired yet).** Run it after a sync that brought new signal, on the sync thread and outside the DB lock:

```rust
// in sync::start's loop, after sync_source(&app, id) returns Ok(n) with n > 0
let out = crate::ai::with_live(&app, |d| crate::ai::suggest::suggest_topics(&app, d, 10));
if let Ok(o) = out { if o.status == "failed" { eprintln!("suggest: {:?}", o.message) } }
```

Items that get a suggestion leave Unsorted, so each is asked about only once. Items that get no pick are asked again on later runs.

## Errors you may see

| Message starts with | Meaning |
|---|---|
| `AI provider 'x' is not on the allow-list` | That tier's provider isn't in `allowed_providers`. Nothing was sent. |
| `No model set for the Large tier` | The model is empty. The AI is off. |
| `ai.region '…' is not a US region` / `… is AWS GovCloud` | Set `ai.region` to a commercial `us-*` region. Nothing was sent. |
| `Model '…' is not a US inference profile` | Use the `us.` ID the message names. Nothing was sent. |
| `Model '…' is from a vendor Alethiometer doesn't know` | Only `us.anthropic.*` and `us.openai.*` are supported. |
| `No AWS credentials found for Bedrock. Store your IAM access key …` | Add `aws_access_key_id` and `aws_secret_access_key` to the secret store. |
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
