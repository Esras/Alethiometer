# Proton Calendar (Life busy blocks)

Tag: `personal`. Reads Proton Calendar **"Limited view"** share links (ICS) and turns them into untitled "Life" busy blocks on the day strip. Nothing else about your personal calendar reaches this machine's database, and nothing is ever written back.

## Create a share link in Proton Calendar

Do this once for each calendar you want to count as busy:

1. Open Proton Calendar on the web (calendar.proton.me).
2. Go to **Settings → All settings → Calendars** and choose the calendar, or click the settings icon next to the calendar in the sidebar.
3. Scroll to **Share with anyone** and choose **Create link**.
4. Choose **Limited view** ("shows when you're busy but doesn't show any event details"). Don't choose **Full view**.
5. Copy the link.

Proton offers Limited view only for links (**Share with anyone**), not when sharing with other Proton users. Proton notes that people using a link may see changes only after a delay (up to several hours), so a new event can take a while to appear in Alethiometer.

The link carries the key that unlocks the calendar. Treat it like a password: Alethiometer never logs it or puts it in error messages. To revoke access, delete the link in the same settings screen.

This wording matches Proton's support page ("How to share a calendar with anyone via a link") as of September 2026. The page couldn't be opened directly from the build environment, so the steps are based on its published text.

## Configure

Put the links in `settings.json`:

```json
{
  "life_calendars": [
    "https://calendar.proton.me/api/calendar/v1/url/…/calendar.ics?CacheKey=…&PassphraseKey=…"
  ]
}
```

That's all you need. On startup, if `life_calendars` has links and no `protoncal` source is configured, Alethiometer adds one named **Life** (personal, polled every 15 min) and logs that it did so, without the links. It does this on purpose: listing a link in `life_calendars` already says "show this as Life", and the source can only ever produce untitled busy blocks.

To change the defaults, add the source yourself. Its `urls` default to `life_calendars`:

```json
{
  "sources": [
    {
      "kind": "protoncal",
      "tag": "personal",
      "name": "Life",
      "poll_secs": 900,
      "config": { "past_days": 1, "future_days": 14, "all_day": "busy" }
    }
  ]
}
```

| Config key | Default | Meaning |
|---|---|---|
| `urls` | `settings.life_calendars` | Links for this source only. Handy for splitting calendars into two sources; otherwise leave it out. |
| `past_days`, `future_days` | 1, 14 | The window of busy blocks kept: from midnight `past_days` ago to the end of today + `future_days`. |
| `all_day` | `"busy"` | `"skip"` ignores all-day events. By default an opaque all-day event blocks the whole day, and an all-day event marked free is ignored. |

A `protoncal` source is always treated as `personal`, even if configured otherwise. No secrets are stored in the secret store; the links live only in `settings.json`.

## What is read

Each poll downloads every link (`webcal://` is fetched as `https://`) and keeps only timing:

- `DTSTART`, `DTEND` / `DURATION`, `RRULE`, `RDATE`, `EXDATE`, `RECURRENCE-ID`, `TRANSP`, `STATUS`.
- `SUMMARY`, `DESCRIPTION`, `LOCATION`, `ATTENDEE`, `ORGANIZER` and alarms are never read. `UID` is only hashed in memory, to match a moved or cancelled instance to its series; it's never stored.
- Recurring events are expanded within the window (`rrule` crate), with excluded dates, extra dates and moved or cancelled instances applied.
- Times are converted to UTC instants: `TZID` (IANA names, including `/mozilla.org/…/Europe/Berlin`-style prefixes), UTC (`…Z`), or floating (read as wall time in your zone). DST changes are handled per zone. A Windows zone name that isn't recognized is read as floating time.
- Events marked free (`TRANSP:TRANSPARENT`) and cancelled events are skipped.
- Overlapping and back-to-back intervals are merged.

The result replaces this source's busy blocks. Per-block travel buffers you set with `b` are kept when a block's start time doesn't change. If any link fails (network, HTTP error, not an ICS file), the whole poll fails and the previous blocks stay. A partial result would silently drop a calendar. Errors name the calendar by position ("calendar 2: HTTP 404"), never by link.

## What is written

Nothing. Personal writes (Marvin life lines) are Phase 2 and don't go through this source.

## Known limits

- Only the window is stored, and each poll replaces the source's blocks wholesale. Life hours from before `past_days` therefore drop out of Reflect's week, month and quarter totals. Raise `past_days` (for example to 92 for a quarter) if you want those totals.
- `RANGE=THISANDFUTURE` on a moved instance is treated as moving that single instance.
