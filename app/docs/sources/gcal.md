# Google Calendar (work calendar)

Kind `gcal`, tag `work`. Reads events from one calendar (default: your primary work calendar), answers invitations (RSVP) and inserts holds when you choose `<` schedule on a Reflect carry row.

## 1. Create your own Google OAuth client (one time)

Alethiometer doesn't ship an OAuth client. You create one in a Google Cloud project, ideally under your work account.

1. Open <https://console.cloud.google.com/> signed in with your **work** Google account. Create a project (e.g. `alethiometer-personal`).
2. **Enable the API:** APIs & Services → Library → search "Google Calendar API" → Enable.
3. **Consent screen:** APIs & Services → OAuth consent screen (in newer consoles: Google Auth Platform → Branding / Audience / Data access).
   - App name `Alethiometer`, your email as support and developer contact.
   - **Audience / user type:**
     - **Internal**, if your Workspace allows it. Only people in your organization can use it, there's no test-user list, and refresh tokens don't expire after 7 days. This is the best choice.
     - Otherwise **External** in **Testing** mode, with yourself added under *Test users*. In Testing mode Google expires refresh tokens after **7 days**, so you'd reconnect weekly (see below). Switching the app to *In production* without verification removes the 7-day limit, but Google then shows an "unverified app" warning during consent. That's acceptable for a personal tool.
   - **Data access / scopes:** add `https://www.googleapis.com/auth/calendar.events.owned`.
4. **Client:** APIs & Services → Credentials (or Google Auth Platform → Clients) → Create credentials → OAuth client ID → Application type **Desktop app** → name it `Alethiometer`. Copy the client ID and client secret. Desktop clients need no redirect URI; Google allows any `http://127.0.0.1:<port>` loopback redirect for them.

> **Workspace admins may block this.** Many organizations restrict which third-party apps can access Google data (Admin console → Security → API controls → App access control). If consent fails with "Access blocked: … admin_policy_enforced" or "This app is blocked", ask your admin to trust your OAuth client ID, or use an Internal app in a project that belongs to the organization.

## 2. Store the secrets

Secrets never go in `settings.json`. On Linux dev they live in `<data dir>/secrets.json` (mode 0600); on the Mac they move to the Keychain once that's wired.

| Secret name | Value |
|---|---|
| `gcal.client_id` | OAuth client ID (`…apps.googleusercontent.com`) |
| `gcal.client_secret` | OAuth client secret (`GOCSPX-…`) |
| `gcal.refresh_token` | Written by Alethiometer after consent. Don't set it by hand. |

The `gcal` prefix comes from `config.secret_prefix`. Change it (e.g. to `gcal.work2`) if you ever add a second Google account as a second source.

Example `secrets.json`:

```json
{
  "gcal.client_id": "1234567890-abc.apps.googleusercontent.com",
  "gcal.client_secret": "GOCSPX-…"
}
```

## 3. Add the source to `settings.json`

```json
{
  "sources": [
    {
      "kind": "gcal",
      "tag": "work",
      "name": "Work calendar",
      "poll_secs": 120,
      "config": {
        "calendar_id": "primary",
        "account": "you@yourcompany.com",
        "category_by_color": { "3": "self", "10": "jobs" },
        "default_category": "work",
        "hold_minutes": 30,
        "hold_hours": [9, 17]
      }
    }
  ],
  "demo_data": false
}
```

All `config` fields are optional:

| Field | Default | Meaning |
|---|---|---|
| `calendar_id` | `primary` | Calendar to sync. A calendar you don't own needs the broader `calendar.events` scope (see `scopes`). |
| `account` | none | Passed to Google as `login_hint`, so consent picks the right account. |
| `days_back`, `days_ahead` | 30, 60 | Sync window. |
| `category_by_color` | `{}` | Google event `colorId` → a key in `settings.categories`. Use it to show self-holds (Lunch, Walk) in the purple `self` color. Event color ids in the Google Calendar UI (the API's `colors` endpoint is authoritative): 1 Lavender, 2 Sage, 3 Grape, 4 Flamingo, 5 Banana, 6 Tangerine, 7 Peacock, 8 Graphite, 9 Blueberry, 10 Basil, 11 Tomato. Events with the calendar's default color have no `colorId`. |
| `default_category` | `work` | Category for everything else. |
| `hold_minutes` | 30 | Length of a scheduled hold. |
| `hold_hours` | `[9, 17]` | Hours (local) a hold may be placed in. |
| `hold_search_days` | 5 | Workdays to search for a free slot. |
| `hold_color` | none | `colorId` for holds, e.g. `"3"`. |
| `rsvp_send_updates` | unset | `sendUpdates` for RSVPs: `all`, `externalOnly` or `none`. Unset uses Google's default. |
| `scopes` | `["https://www.googleapis.com/auth/calendar.events.owned"]` | Override only if you need a calendar you don't own. Then use `calendar.events` and reconnect. |
| `secret_prefix` | `gcal` | Prefix of the three secret names above. |

## 4. Connect (consent)

Restart Alethiometer after editing `settings.json`, then call the API. With the browser dev server:

```sh
curl -s -H 'X-Alethiometer: 1' -X POST http://127.0.0.1:1421/call/gcal.auth_start -d '{}'
# → {"url": "https://accounts.google.com/o/oauth2/v2/auth?…", "redirect_uri": "http://127.0.0.1:53817", …}
```

Open `url` in your browser, pick your work account and approve. The tab should say "Alethiometer is connected". Then:

```sh
curl -s -H 'X-Alethiometer: 1' -X POST http://127.0.0.1:1421/call/gcal.auth_status -d '{}'
# → {"state": "connected", "has_refresh_token": true, …}
```

Easiest: open the sync status in the tab bar and press **Connect** next to the calendar source. Alethiometer opens the consent screen in your browser and saves the refresh token as soon as you approve; `auth_status` only reports progress. The listener waits 10 minutes. Both calls take an optional `source_id` if you have more than one `gcal` source.

How it works: authorization code flow with PKCE (S256) and a loopback redirect on `127.0.0.1` at a random port. A random `state` guards the redirect, and the listener accepts only that. Alethiometer asks for offline access with `prompt=consent`, so Google always returns a refresh token. Access tokens live in memory only. They're refreshed a minute before expiry, or after a 401.

**Reconnecting.** If the sync error says "Google refused the refresh token", the token expired (Testing mode: 7 days) or you revoked access. Press **Connect** again (or run `gcal.auth_start`).

## What is read

`events.list` on the calendar with `singleEvents=true` (recurring meetings are expanded into instances) and `showDeleted=true`:

- The first sync covers the window: 30 days back to 60 ahead.
- After that, syncs are incremental with the stored `syncToken`. A `410 Gone` (expired token) triggers a full window sync.
- Once per day the poll also does a full window sync. An incremental sync never returns recurring instances that slide into the window without being edited, so this catches them.
- The cursor (`sources.cursor`) is JSON: `{sync_token, full_on, seen}`. `seen` maps event id → end time, a hash of start/end/status/guests, and whether an answer is pending. The connector can't read the database, so this map is how it knows whether an update is new signal.

Each event becomes an item of kind `event`:

| Item field | From |
|---|---|
| title | `summary` (self-holds like "Lunch" keep their titles: this is work-calendar data) |
| body | `description`, HTML stripped |
| summary | "Priya, Jordan, Lee +2" (first names; you and rooms left out), plus "needs your response", "tentative", "declined" or "all day" |
| author | organizer email as handle, display name as the person name. Nothing for your own events |
| url | `htmlLink` |
| day strip block | start/end stored in UTC, placed on the strip in your local time, category from `colorId`, attendees summary. **Tentative** (dashed) when your response is `tentative` or `needsAction`, or the event itself is tentative |

- **All-day events** (offsites, OOO days) stay as items but get no block on the 08–19 strip.
- **Declined** events stay as items with status `done` and no strip block. Re-accepting brings the block back.
- **Working-location** entries ("Home", "Office") are skipped; they aren't meetings.
- **New signal (Incoming):** new events, invitations still awaiting your answer, and changes to an event's time or guest list. Description or title edits and other guests' RSVPs don't resurface an event. Neither does your own decline.
- **Cancelled or deleted events are never deleted locally.** The item stays (title, history, topic links, notes), its status becomes `done`, and it shows once more in Incoming so you notice the cancellation. Only its row in `events`, the cached block on the strip, is removed. That row is a copy of remote times, not your content. Leaving it would show a meeting that no longer exists. Cancellations of events Alethiometer never stored are ignored.

## What is written (outbox ops)

Every write goes through the outbox and is confirmed only after Google answers.

### `gcal.rsvp`

Payload: `{"event_id": "<Google event id = item external_id>", "response": "accepted" | "declined" | "tentative"}`

Alethiometer fetches the event to find your own attendee entry (`self: true`). It then sends a `PATCH` with `attendeesOmitted: true` and just that one entry, so no other guest can be touched. It uses `If-Match` on the event's etag, so an event that changed in the meantime fails visibly (retry with ⌘R). Receipt: `{event_id, response, etag}`. The next poll updates the strip (the dashed border goes away).

### `gcal.hold`

Enqueued by Reflect's `<` schedule with payload `{item_id, title, carried_from}` (optionally `start`/`end` as local `YYYY-MM-DDTHH:MM:SS` to place it exactly).

Without times, Alethiometer finds the first free `hold_minutes` slot within `hold_hours`. It looks on the workdays after the later of `carried_from` and today, skipping weekends. It reads busy time from the calendar itself, ignoring free (transparent), cancelled and declined events, and rounds to the quarter hour. It then inserts an event titled with the item's title: `visibility: private`, `transparency: opaque` (busy), default reminders, and private extended properties `alethiometer_item_id` / `alethiometer_carried_from`.

The event id is derived from the calendar, item and carry date. If Google created the event but the response was lost, a retry gets a `409` and adopts the existing event instead of creating a duplicate. Receipt: `{event_id, start, end, html_link}`.

Note: your Proton "Life" blocks aren't visible to this search, because the connector only sees the work calendar.

## Scopes and access

- `https://www.googleapis.com/auth/calendar.events.owned`: "See, create, change, and delete events on Google calendars you own". This is the narrowest scope that covers `events.list`, `events.get`, `events.patch` (RSVP) and `events.insert` (holds) on your own calendar. Alethiometer never deletes events.
- For a calendar you don't own (e.g. a shared team calendar), set `"scopes": ["https://www.googleapis.com/auth/calendar.events"]` and reconnect.

## Not verified without real credentials

All tests use recorded-shape fixtures (`src-tauri/tests/fixtures/gcal/`). Against a real account, check:

- consent works for your Workspace (admin app-access policy) and the chosen user type;
- `calendar.events.owned` is accepted for RSVP on invitations organized by others;
- whether the organizer gets an email for your RSVP with `rsvp_send_updates` unset.

## Consent screen options

| Setup | Refresh token lifetime | Notes |
|---|---|---|
| **Internal** app (OAuth consent screen user type "Internal") | long-lived | Needs a Google Cloud project inside your Workspace org. Best option if your org lets you create one. |
| **External**, publishing status **Testing**, you as test user | expires after 7 days | Works anywhere; press Connect again weekly. |
| **External**, **In production**, unverified | long-lived | Google shows an "unverified app" warning you click through; Workspace admins may block unverified apps. |

Workspace admins can also restrict third-party apps entirely (Admin console → Security → API controls); if consent fails with "access blocked", ask them to allow your OAuth client ID.
