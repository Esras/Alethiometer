# Work AI (Amazon Bedrock)

Helm's AI answers questions in the assistant pane and in Reflect ("Ask about today"), and suggests topics for Unsorted items. It runs on Amazon Bedrock through the **Converse API** (`POST https://bedrock-runtime.<region>.amazonaws.com/model/<modelId>/converse`), signed with AWS SigV4. Code: `src-tauri/src/ai/`.

It follows three boundary rules:

- **Allow-list, fail closed (rule 2).** Work data goes only to providers listed in `ai.allowed_providers`. If a tier's provider isn't on the list, or its model is empty, or credentials are missing, the call fails with an error and nothing is sent. There is no fallback provider.
- **Nothing personal (rule 4).** Prompts are built only from `work`-tagged sources. Life blocks aren't sent at all, not even as busy times.
- **Suggests and answers only (rule 6).** The system prompt tells the model not to draft replies, notes or reflections. Topic suggestions are stored as `suggested`, never `confirmed`. Only you confirm them.

## Before you turn it on: BAA

Work data may contain PHI. Your organization must confirm that **Amazon Bedrock, in the region and account you use, is covered by its AWS Business Associate Addendum**, and that the models you pick are HIPAA-eligible under that agreement. Helm can't check this. `allowed_providers: ["bedrock"]` records your decision; it doesn't prove coverage.

Also decide on routing. A `global.` inference profile can run the request in any commercial AWS region. A `us.` profile stays in US regions. If your compliance rules require a geography, use the regional prefix.

## Setup

### 1. Enable model access

In the AWS console, open **Amazon Bedrock → Model access** in the region you'll use (for example `us-east-1`) and request access to the Anthropic models you'll use. Access is per account and per region. Without it, Bedrock returns 403 and Helm shows "model access not enabled in Bedrock console for <region>".

### 2. Pick model IDs

Claude models that are current on the Converse API take an **inference-profile ID**: the base model ID with a `us.` / `global.` / `eu.` (…) prefix. The bare base ID fails with "on-demand throughput isn't supported", and Helm tells you to add a prefix.

Suggested values. These IDs come from Anthropic's Bedrock docs (Sept 2026). They haven't been called live from Helm.

| Tier | Use | Suggested `model` |
|---|---|---|
| `small` | topic suggestions (short JSON answers) | `us.anthropic.claude-haiku-4-5-20251001-v1:0` |
| `large` | questions (assistant pane, Reflect) | `us.anthropic.claude-sonnet-4-6`, or `us.anthropic.claude-opus-4-6-v1` for harder questions |

Swap `us.` for `global.` if global routing is acceptable. You can also give a full inference-profile ARN (`arn:aws:bedrock:<region>:<account>:inference-profile/<id>`).

**Newer models:** Claude Opus 4.7 and later (Opus 4.8, Opus 5, Opus 5.5, Sonnet 5, Fable 5.x) are served on Bedrock's Messages-API endpoint (`https://bedrock-mantle.<region>.api.aws/anthropic/v1/messages`, model IDs like `anthropic.claude-opus-5`, IAM action `bedrock-mantle:CreateInference`). Anthropic documents them for `InvokeModel` but not for Converse. Helm doesn't talk to that endpoint yet. If you want those models, adding that endpoint is a small follow-up: the same SigV4 signer with service `bedrock-mantle` and a Messages-API body.

The shipped defaults leave both models empty, so the AI stays off until you choose models and confirm the BAA.

### 3. IAM policy

Converse needs `bedrock:InvokeModel` on the model or inference profile named in the request. With a cross-region inference profile, the caller needs `bedrock:InvokeModel` on **both** the inference-profile ARN and the underlying foundation-model ARN in every region the profile can route to. A region wildcard covers that:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "HelmBedrockInvoke",
      "Effect": "Allow",
      "Action": "bedrock:InvokeModel",
      "Resource": [
        "arn:aws:bedrock:us-east-1:<account-id>:inference-profile/us.anthropic.claude-sonnet-4-6",
        "arn:aws:bedrock:us-east-1:<account-id>:inference-profile/us.anthropic.claude-haiku-4-5-20251001-v1:0",
        "arn:aws:bedrock:*::foundation-model/anthropic.claude-sonnet-4-6",
        "arn:aws:bedrock:*::foundation-model/anthropic.claude-haiku-4-5-20251001-v1:0"
      ]
    }
  ]
}
```

Helm doesn't stream, so it doesn't need `bedrock:InvokeModelWithResponseStream`. Nothing else is needed: no list or describe calls, and no S3.

### 4. Credentials

Helm reads the first match in this order:

| # | Where | What |
|---|---|---|
| 1 | environment | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN` |
| 2 | Helm secrets (Keychain on the Mac, `secrets.json` on Linux) | `aws_access_key_id`, `aws_secret_access_key`, optional `aws_session_token` |
| 3 | `~/.aws/credentials` | static keys for the profile |
| 4 | `~/.aws/config` | static keys, or `credential_process`, for the profile |

The profile is `ai.aws_profile` in settings. If that's empty, Helm uses `AWS_PROFILE`, then `default`. `AWS_SHARED_CREDENTIALS_FILE` and `AWS_CONFIG_FILE` are honored. Apps started from the Dock don't see shell environment variables, so on the Mac use `ai.aws_profile` or the Keychain.

**SSO (IAM Identity Center).** Helm doesn't read the SSO token cache. Add a profile that asks the AWS CLI for credentials, and point Helm at it:

```ini
# ~/.aws/config
[profile my-sso]
sso_session = corp
sso_account_id = 123456789012
sso_role_name = HelmBedrock
region = us-east-1

[profile helm]
credential_process = aws configure export-credentials --profile my-sso --format process
```

Then set `"aws_profile": "helm"`. When the SSO session expires, the call fails with "credential_process … failed (expired SSO login? run `aws sso login`)". If Helm finds an SSO profile with no `credential_process`, it says so.

Credentials are read for every call and are never logged or shown in errors.

### 5. settings.json

```json
"ai": {
  "allowed_providers": ["bedrock"],
  "region": "us-east-1",
  "aws_profile": "helm",
  "small": { "provider": "bedrock", "model": "us.anthropic.claude-haiku-4-5-20251001-v1:0" },
  "large": { "provider": "bedrock", "model": "us.anthropic.claude-sonnet-4-6" }
}
```

Restart Helm. The assistant pane header shows `bedrock · <large model>`. `ai.status` reports each tier's config check without calling AWS.

## What each feature sends

Everything goes in one Converse request per call, over HTTPS to `bedrock-runtime.<region>.amazonaws.com`. Helm stores nothing on the AWS side.

| Feature | Command | Tier | Sent to Bedrock |
|---|---|---|---|
| Ask about a topic (assistant pane with a topic open) | `ai.ask {question, topic_id}` | large | The question. The topic name. The topic's Obsidian note (first 6,000 chars). For confirmed, work-tagged items linked to the topic (open tasks first, then newest first): source, kind, time, status, due date, owner, title, summary, first 600 chars of the body, and the last 6 messages (300 chars each). |
| Ask about today (Reflect, or the assistant pane with no topic) | `ai.ask {question}` or `scope: "today"` | large | The question. Today's work calendar events (time, title, attendees, tentative). Work items that happened or got a message today, with the same fields as above. Life blocks aren't sent. |
| Topic suggestions | `ai.suggest_topics {limit}` | small | For each Unsorted item (up to `limit`, default 10, max 50): source, kind, title and summary (no body). All active topic names with a one-line gist (the first lines of each topic's note). The names of topics you marked wrong for that item, as "NOT about". |

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

Each Unsorted item costs one small-model call of roughly 300 input and 30 output tokens. Items that get a suggestion leave Unsorted, so each is asked about only once. Items that get no pick are asked again on later runs. If that repeats too often, track "last asked" (open question below).

## Errors you may see

| Message starts with | Meaning |
|---|---|
| `AI provider 'x' is not on the allow-list` | That tier's provider isn't in `allowed_providers`. Nothing was sent. |
| `No model set for the Large tier` | The model is empty. The AI is off. |
| `No AWS credentials found` | None of the credential sources above matched. |
| `Bedrock denied access … model access not enabled in Bedrock console for <region>` | 403. Enable model access in that region, or fix the IAM policy. |
| `Bedrock rejected the AWS credentials` | 403: bad, expired or unsigned credentials. |
| `Bedrock needs an inference-profile id` | 400. Add a `us.` or `global.` prefix. |
| `Bedrock is throttling requests` | 429, a quota limit or the model isn't ready. Retry later. |
| `Bedrock had a temporary problem` | 408, 424 or 5xx. |

## Not verified here

There were no AWS credentials in the build environment, so no live call was made. What was checked offline:

- The SigV4 signature matches the AWS SigV4 test suite (`get-vanilla`) and botocore 1.43 on the exact Converse request shape.
- Request and response JSON match the botocore `bedrock-runtime` service model.
- Error statuses match that model's error shapes.

Still to confirm on a real account: the model and inference-profile IDs and region availability, the exact IAM resource list, and real error message texts.
