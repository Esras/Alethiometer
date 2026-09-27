# Work and personal data separation

_Started 2026-09-27 19:00 UTC_

---

## User

Continuing from "Personal Assistant UI Mockups".

## Assistant

We were building a unified personal assistant app for Linux (KDE Plasma) that aggregates tasks, emails, calendar events, Slack, Jira, Obsidian notes and other sources. The core jobs are a morning briefing, unified inbox triage, focus mode ("what now?"), and weekly reflection. You're a manager moving into director-track work with heavy meeting loads, and you lose track of time and attention easily.

We've built five main views through nine rounds of iteration:

- **Incoming (8a):** new items grouped by topic, with Confirm or Wrong Topic actions. Unsorted sits at the bottom. The selected topic shows new signals grouped by app.
- **Priorities (8b):** four quadrants (Now-ish, Make room, Quick touch, Can wait) with items selected by aqua-right highlight. Quadrants can collapse to compact topic chips. Each item shows faded topic name and reason pills (red/yellow/green by weight).
- **Topics (7a):** a two-column layout with left side (Upcoming, Open, Chronology, People—all collapsible, scrollable) and right side (Obsidian note). Fold button top-left. Timeline has open dots (upcoming/undone) and solid dots (past). Meetings linked to this topic highlight on the day strip. Day strip shows to-scale blocks with countdown to next meeting (green >15m, yellow 5-15m, orange 2-5m, red <2m).
- **Notes (9a):** free-form capture with optional prefixes (`-` note, `[]` do, `()` research, `^@name` delegated). Margin shows suggested destinations. Tab opens a picker to override. "Send to Incoming" moves everything across for triage.
- **Reflect (9b):** by day/week/month/quarter. Reflection note takes most space, compact side panel shows plan-vs-did bars and folded summaries. Carry forward actions (>, <, ×). Work·Life toggle. KDE nudge persists until answered.

Design decisions: Gruvbox dark hard palette with only your kitty colors (red, green, yellow, blue, purple, aqua) for category tags. Muted desaturated app icons from you. Keyboard-first. Topics are the root (meetings attach to them). No AI writes for you; it only suggests or answers questions. Reclassify just marks wrong, drops to Unsorted. No automatic deletions, ever. Calendar bar stays on every view except Reflect. Chat pane collapsible.

Open: work/personal separation (one app scoped by time, two profiles, or life-only on some devices?). You're leaning toward option 1.

Files: Assistant Hub Explorations.dc.html, DayStripGruv.dc.html, icons/ folder with your desaturated icons.

## User

The last agent and I were working on the 9a / 9b edits, and the question of how to keep things separate or not between work and life. In short, the goal is that I _don't_ want to be ignoring personal stuff during work, but at the same time, I do try to keep my work profiles and home profiles separate (that is, no using my work laptop for personal things). The challenge here is that there are things that would need to _never_ go in the personal versions of stuff (which I think would be relatively easy to do), but there are work things that MUST NOT ever cross into the personal side; the systems that this would talk to on the work side _can_ contain PHI (though it is unlikely), and thus should _only_ ever be used with LLM Providers that we have a BAA with (which, we do, and have that set up). So just trying to think about how to go about that.

## Assistant

Your constraint is asymmetric, so the architecture should be too. Personal data can flow into work, read-only. Work data never flows out. Option 1 (one app scoped by time) doesn't fit, because a single instance would hold both kinds of data and use one set of AI providers. I'd go with this instead:

**Two isolated instances, one codebase**
- **Work instance:** runs only on the work laptop. It uses only AI providers you have a BAA with (or a local model). Those providers are set per instance in config, not per request. If a provider isn't on the approved list, the request fails instead of falling back to another one. Work instance storage is separate and encrypted, with keys that never leave that machine.
- **Personal instance:** runs on your own devices with any provider. It has no work connectors at all; they aren't built into that version.

**One-way Life → Work feed**
- The personal side publishes a minimal read-only feed: calendar blocks (with a "busy only" option per event), task titles and due dates, and reminders.
- The work instance pulls it. The personal side stores no work credentials and has no endpoint to push to.
- Life items on the work side are view-only. You can snooze or acknowledge them, but that state stays local and never syncs back.
- This matches your "no personal use of the work laptop" rule: you *see* life things during work, and you *act* on them on your phone or home machine.

**Work → Life: nothing from the app**
- If you want your personal calendar to show when you're busy at work, use your work calendar's own free/busy sharing. That way the assistant is never the path out.
- Blocked by design: a work item can't be quick-captured into the personal side. If "remind me to call the dentist" gets typed on the work machine, it stays a work-side note until you retype it on your phone. It's a bit of friction, but content typed on a work machine could contain PHI, so it never crosses automatically.

**What changes in the current views**
- **Time-scoping still exists, but only as UI emphasis inside the work instance.** For example, life items rise in Priorities around lunch and after 5pm.
- **Life items get a distinct marker**, such as a one-way arrow icon plus a "Life" tag, and don't take actions that would change the source.
- **The chat pane shows which AI provider it's using** (for example "BAA · Provider"), so it's always clear where your questions go.
- **Reflect's Work·Life toggle works on the work side because life data is available there.** The personal instance's Reflect shows life only.
- **Open decision:** should life items be sent to the work-side AI at all? It's your data going to a BAA provider, so that's allowed, but you may want a per-feed "don't send to AI" toggle.

I can mock up the boundary settings screen and how life items look in Priorities and Incoming as 10a/10b.

## User

I'm not sure. What's being described adds an awful lot of friction to things, and the friction is what I'm trying to avoid overall here. At the same time, the separation is important (I do not want IT reading personal stuff, if there was reason to get on my laptop).

There's a case to be made for skipping the personal version and just being okay pulling in _some_ personal info, but not sure how I feel about that. Other thoughts?

## Assistant

Your new constraint (IT shouldn't be able to read personal stuff) changes the answer. Personal *content* shouldn't be stored on the work laptop at all, so pulling in some personal info doesn't really work either.

What you need during work is mostly **awareness and timing**, not the details. You need to know something personal is due at 5:30, not what it is.

**Content-free life signals**
- The personal side sends the work app only shapes, with no titles or notes:
  - untitled blocks on the day strip, e.g. "Life · 5:30–6:15"
  - counts, e.g. "Life: 2 due today, 1 overdue"
  - timing nudges, e.g. "Leave by 5:10"
- If IT looks at the laptop, they see "something personal at 5:30" and nothing else.
- Nothing personal reaches the work-side AI, so the BAA question disappears.
- The countdown colors work on life blocks exactly as they do on meetings.

**Your phone is where personal things happen**
- Clicking a life signal pings your phone and opens the item there. That's one action, not "remember to check later".
- KDE Connect can already do this on Plasma. Its notifications pass through the laptop without being saved. It depends on whether IT allows it on your machine.

**Capture: the one deliberate hole**
- In Notes, a `~` prefix (or similar) marks a line as life. On send, it goes to the phone and is deleted locally.
- You type it and send it yourself; the app never moves work content across. That keeps the risk to what you choose to type.

**What this removes compared with my last proposal**
- No life feed to maintain, and no read-only life items to manage on the work side.
- Life items don't need to be triaged in Incoming. They only need to not be forgotten.
- The personal instance can be as simple or as full as you like, even just your existing phone apps sending into the signal feed.

**Trade-off:** on the work side the AI can't reason about personal items. It can still see that a life block is coming, but can't answer "should I push this errand?". I think that's worth it for the separation.

Should I mock this up as 10a? It would show the day strip with life blocks, the life counts in the header, and the `~` capture in Notes.

## User

Say more about the KDE Connect piece - you're also describing "on your phone" in a couple of comments and while the original intent was that this was a desktop-native application, a couple of the ways being described here sound a bit more cross-platform.

I have pretty free reign of my work laptop, for better and worse.

I think I need to see 10a, yes.

## Assistant

_[tool: grep]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: dc_html_str_replace]_

_[tool: ready_for_verification]_

I added 10a at the top of the canvas. Helm stays a desktop app; the phone would be a paired device, like a mouse or headset.

**What KDE Connect is**
- It's KDE's built-in link between your Plasma machine and your phone. It works directly between the two devices over an encrypted connection on the same network, or over a VPN like Tailscale when you're away. Nothing goes through a cloud service.
- Since you control the laptop, installing it isn't an issue.

**What Helm would use it for**
- **Send text to the phone.** This is the `~` lines in Notes. The phone gets the text and you file it into Marvin with one tap. That tap is the one bit of friction left. It avoids keeping your personal Marvin login on the work laptop.
- **Open something on the phone.** Pressing ↵ on a Life block tells the phone to open its calendar at that time.
- **Check the phone is there.** The "Pixel · connected" in the tab bar is just that status.

**What it's *not* used for**
- **Your personal calendar.** The Life blocks come from your personal calendar's free/busy link, which carries times but no titles. KDE Connect doesn't sync calendars.
- **Mirroring phone notifications.** KDE Connect can do this, but I'd leave it off or allow only a few apps. Otherwise personal notification text would pass through the laptop, which is the leak you're trying to avoid.

**So "on your phone" means:** you act on personal things with the phone apps you already use. No phone version of Helm is needed.

**Open questions**
- Should the phone send anything back, like "picked up, done"? I'd say no, so it stays one-way.
- 10a drops the life counts I suggested last time (e.g. "2 due today"). Showing them would need something on the personal side to work them out, which is extra to build and maintain.

## User

Okay, so let me throw some more wrinkles in this:
* Work laptop is an ARM Macbook Pro. So the primary desktop application being described here isn't actually KDE anymore.
* Phone is an iPhone (not linked to the work laptop in any meaningful way)
* Phone has Proton Mail / Calendar, Amazing Marvin, Obsidian, etc, so data can sync there.
* I could set up a work Obsidian vault (the perpetual license I have allows for that) for _just_ work things, but could also tie that into Notion instead. Or something else. Not sure how GH links into this or not.
* One-way is fine, but a confirmation that it reached the other side would probably be better, just so that the work-local app doesn't delete things.
* I agree with keeping things simple, I need to consider more about what does or doesn't get synced over while you iterate a bit on this.

## Assistant

_[tool: run_script]_

_[tool: web_search]_

_[tool: web_search]_

_[tool: dc_html_str_replace]_

_[tool: ready_for_verification]_

I added turn 11 at the top of the canvas. The Mac and iPhone never connect to each other; both sides go only through the services themselves.

- **11a (Notes, just after sending):** a `~` line goes to Marvin's Inbox and your iPhone gets it through Marvin's own sync. The line stays on the Mac until Marvin confirms it; if sending fails, the line stays and retries.
- **11b (Crossings settings):** one row per boundary decision, each saying exactly what the Mac keeps. There's a "Not decided" group where nothing crosses until you move a row out, so you can think it through there.
- **Personal calendar:** Proton's share link has a Limited view option that shows when you're busy but doesn't show any event details. It's one link per calendar, so Personal and Family each need their own.
- **Day strip:** it now has a busy-only mode. It's on in 11a; the earlier views are unchanged.

**One catch with Marvin.** I'd assumed its basic API key could only add tasks. It can't: the same key can also get scheduled tasks and due/overdue tasks. So if someone pulled that key from the Mac, they could read your Marvin tasks. There are two ways to handle it:
- Accept it: the key sits in the Mac's Keychain and Helm only ever uses it to add tasks.
- Close the gap: run a tiny forwarding service you own that holds the Marvin key. The Mac would hold a key that can only add tasks and get confirmations back.

**Work notes and GitHub**
- A separate work Obsidian vault still fits best, because Helm reads the files directly and no other service sees them. Notion would work only if your organization's BAA covers it.
- GitHub could show up in two ways: as backup for the work vault, or as a source feeding pull requests and issues into Incoming. For the backup, check whether your BAA covers GitHub before putting notes there that could contain PHI.

The rows in "Not decided" are the questions for you to settle.

## User

11a: Aren't you describing a flow where Marvin (personal) is thus linked to the work version of the app? Or are you talking about a one-way sync here?

11b is overengineered, this is practically a diagram of the flows, not an actual settings page. Remember: personal tool, not a SaaS / paid-for solution to sell.

GH would be another source, not a backup for Obsidian. Mostly it's another Issues / PRs / etc. source to link into things (or topics might also have repos associated).

## Assistant

_[tool: run_script]_

_[tool: ready_for_verification]_

I cut 11b down to a plain Settings › Life page with three fields: busy-only calendar links, travel buffer, and whether `~` lines go to Marvin (with its key).

**On 11a:** yes, it's a link. Data only goes one way, but the work app holds a key to your personal Marvin, and that key can also read your tasks. If you don't want any personal key on the Mac, the Mac could hand off by showing a QR code of the `~` lines. You'd scan it with your iPhone and a Shortcut would add them to Marvin. That leaves no key on the Mac, but costs one extra step. It also loses the automatic confirmation: you'd confirm on the Mac once you've seen them land. Which do you prefer?

**GitHub:** understood, it's another source like Jira. Issues and PRs go into Incoming, and a topic can have linked repos. I'll show that in the next Topics pass.

## User

Again, with it being personal, hard-coding in these ideas is fine, having the URLs in a settings.json is fine (or mac keychain or whatever).

The QR code idea is interesting, but only if it brings enough info to be useful (as in, it has all of the appropriate fields filled out / transferred / whatever). I think in the grand scheme of building this as a tool, that one can be a bit further down the list, because getting a minimal thing I might actually use and iterate on is probably more important than the minutiae of how I interact with the personal part.

## Assistant

_[tool: run_script]_

I removed 11b. The calendar links and travel buffer can go in a `settings.json` file and the Marvin key in Keychain, so there's no settings screen. I've moved the QR hand-off to later.

For a first version you'd actually use, I'd cut it down to:
1. **Sources:** work calendar, Slack, Jira, GitHub, and the work Obsidian vault, all read-only.
2. **Views:** Topics (7a) with the day strip, and Incoming (8a).
3. **Life:** busy-only blocks from the Proton calendar links, and nothing going out yet.

Priorities, Notes, Reflect, and `~` sending to Marvin come after you've used that for a week or two.

Do you want me to write this up as a short build plan to hand to Claude Code?

## User

I just uploaded Obsidian's desaturated icon. Can't rename it from this UI, though, so it's `obsidian.md.png`.

I think your scoping is a little too narrow for the MVP. I do think it needs writeback capability, and I'm not that worried about it (in the sense that I don't think it's that much more added complexity here). There's definitely open questions I assume that a new thread coding this up is going to have about backend logic and wiring things up (Provider? Model? Different levels of model for different purposes?) Happy to answer those all there, too.

If you want me to be more precise on it, I would say Phase 0 is everything you just described, but Phase 1 needs to be done with it; that is, Priorities and Writeback included. The separation of Life stuff right now makes a bit of the filtering more moot than it was before, but I think I want to keep that (meaning a source gets tagged as either Personal or Work).

Sending / more personal integration / etc. can be a Phase 2 after usage thing.

What does the technology stack look like, given the MacOS thing instead of KDE? Especially since I think most of this can be local to a system and doesn't require biz logic on a server? I can think of some other tools that might help (The RAG store / indexing) but that's also all local.

## Assistant

_[tool: grep]_

_[tool: list_files]_

_[tool: copy_files]_

I renamed the icon to `icons/obsidian.png`, which the Obsidian panels in the Topics mockups load.

**Stack I'd pick: Tauri 2 (a Rust core with a web UI)**
- The mockups are already HTML and CSS, so the UI carries over almost directly, including the day strip and the dense Gruvbox styling.
- It's a real native app on Apple Silicon: small and fast, and far lighter than Electron.
- It still runs on Linux/KDE if you ever want that back.
- Tauri's plugins already cover global hotkeys, menu bar, notifications, launch at login and Keychain.
- The alternative is SwiftUI: the best Mac feel, but slower to iterate on and Mac-only. I'd only choose it if Mac-native polish matters more to you than those.

**Everything local, no server**
- **Storage:** one SQLite file. SQLite's built-in full-text search covers search everything, and the sqlite-vec extension adds the search index for the AI. Sources, items, topics, links and the outbox are all tables in it.
- **Config:** `settings.json`, with keys in the Mac's Keychain.
- **Sync:** each source is polled for only what's changed since the last check, since a local app can't receive incoming webhooks. Slack's Socket Mode is the exception: it gives live updates without a server. Obsidian is just files on disk that Helm watches for changes.
- **Personal/Work tag:** one field set per source, which every view and query filters on. Work data only ever goes to your BAA provider.

**Writeback, in the same pattern as the Marvin receipts**
- Every write (Jira status change or comment, a GitHub review or comment, Slack mark-as-read or reply, a calendar RSVP, an edit to an Obsidian file) goes into an outbox first. It's marked done only once the service confirms it.
- It never deletes anything, and failed writes stay visible with a retry.
- Edits to Obsidian files are made in place, never as full rewrites, so conflicts with Obsidian itself stay manageable.

**AI**
- **Search index:** the embedding model that builds it runs locally, through Ollama or MLX on Apple Silicon, so building the index never leaves the laptop.
- **Other work, all via the BAA provider's API:**
  - a small, fast model for topic suggestions and reason pills
  - a larger one for briefings and questions
  - possibly a local model for classification later
- The build thread can settle the provider and exact models, as you said.
- **Optional:** existing MCP servers (GitHub, Jira, Marvin) could serve as connectors instead of writing each one from scratch, but plain API clients are simpler to debug at first.

**Phases**
- **Phase 0:** read-only sources, Topics, Incoming, and life busy-blocks.
- **Phase 1:** Priorities and writeback.
- **Phase 2:** sending life lines out and deeper personal integration.

Where do you want Notes and Reflect: Phase 1 or Phase 2?

## User

Yuck, Tauri is just embedding Chrome. Is there a way to avoid that? Or use Firefox instead? I know on iPhone Apple mandates all web views end up being webkit.

I agree with lighter than Electron, it's hard not to be faster there, but would like to avoid embedding an entire browser surface.

How does the Slack websocket part work, does it require the whole "Install an app then get a single OAuth token for a single user" thing?

Don't know how worried I am about the SQL schema, but happy to help design that so it won't suck. I don't know what the Rust-equivalents of SQLAlchemy are.

MCP: I'm not sure MCP really buys anything here. I guess it may make the Auth step a bit easier? Slack doesn't have a proper one, as sort of referenced above on my OAuth commentary.

Notes / Reflect should be Phase 1.

## Assistant

Tauri doesn't bundle Chrome, which is why it's small. It uses the web view built into the operating system: on your Mac that's WebKit, the engine Safari uses and already installed. Electron is the one that ships Chromium with every app. Firefox's engine can't really be embedded in a desktop app anymore; nobody maintains a way to do it outside Android.

**If you'd rather have no web view at all**
- **SwiftUI with GRDB (a Swift SQLite library):** this is where I'd go. Native Mac controls, and the Mac features Helm needs are all built in: Keychain, calendar access, watching the Obsidian folder, global hotkeys, menu bar and notifications. GRDB is an excellent SQLite library with built-in full-text search and migrations, and sqlite-vec loads into it. The cost is that it's Mac-only and the mockups get rebuilt as views rather than ported.
- **A Rust native UI toolkit** (gpui, the framework behind the Zed editor, or iced or Slint): fast, keyboard-friendly, and could run on Linux later. But these toolkits are younger, with thinner documentation, and dense custom layouts like the day strip take more manual work.

My pick is SwiftUI with GRDB. You're on a Mac now, and native Mac features plus a mature database library outweigh keeping Linux open.

**Slack**
- Yes, you create an app, but it's just for you and there's no OAuth redirect server.
- You create the app in Slack's settings, turn on Socket Mode, and add the user permissions you need (read history, read DMs, mark as read, post).
- Installing it from that settings page hands you two tokens directly:
  - an app-level token, which opens the live connection
  - a user token, which reads and writes as you
- Subscribe to user events, and you get live messages, DMs and mentions as they happen, with no public server.
- The catch is your organization's Slack admin: many workspaces require approval before an app can be installed. Having full control of the laptop doesn't help with that.

**Database schema**
- **In Rust:**
  - SeaORM is the closest thing to SQLAlchemy's ORM.
  - Diesel checks queries at compile time but has its own query language.
  - sqlx isn't an ORM: plain SQL, checked at compile time, with migrations.
- **In Swift:** GRDB covers both styles, raw SQL or typed records.
- Either way the schema is small, so plain SQL with migrations is fine. I'd happily sketch it with you before the build thread starts: sources, items, topics, links, people, outbox and reflections.

**MCP:** agreed, it doesn't help here. You'd still set up login for each service yourself, and it adds a separate process to run. Direct API clients are simpler.

**Phases, updated**
- **Phase 0:** read-only sources, Topics, Incoming, and life busy-blocks.
- **Phase 1:** Priorities, writeback, Notes and Reflect.
- **Phase 2:** sending life lines out and deeper personal integration.

## User

I see, I was wrong on Tauri.

I will note that Zed (the company) ended up moving away from blade (the rendering layer), and at the same time ended up saying they were putting a lot less effort towards GPUI:

> > Hey y'all, GPUI develoment is getting some major brakes put on it. We gotta focus on some business relevant work in 2026, and so I'm going to be pushing off anything that isn't directly related to Zed's use case from now on. However, Nate, former employee #1 at Zed, has started a little side repo that people can keep iterating on if they're interested: https://github.com/gpui-ce/gpui-ce. I'm also a maintainer on that one, and would like to try to help maintain it off of work hours. But I'm not sure how much I'll be able to commit to this

SwiftUI sounds like potentially the way to go, though I don't exactly recall all of the MacOS "you can't build things without XCode and can't install things without a signing key" stuff. If that's going to be a pain, Tauri is probably fine.

## Assistant

Thanks for the GPUI update. That settles it: we shouldn't build on a toolkit whose owner is stepping back.

On signing, it's less painful than you remember, and it's the same for Tauri because Tauri on a Mac uses Apple's tools too.

**Building**
- You need Xcode installed, but you don't have to work inside it.
- Claude Code can build from the terminal.
- The fiddly part is the Xcode project file, which is hard to edit by hand. Generate it from a short YAML file with XcodeGen or Tuist, or use a plain Swift package plus a small script that wraps it into a `.app`.

**Signing and installing on your own Mac**
- **No paid developer account ($99/yr) needed.** That's only for notarizing apps you distribute to others.
- **Gatekeeper won't block it.** It only blocks apps downloaded from the internet, and one you build locally doesn't count.
- **One real annoyance:** macOS ties Keychain access and privacy permissions (Calendar, file access) to the app's signature. Without a stable signature, every rebuild counts as a new app, so you re-approve Keychain and Calendar access each time.
- **Fix:** sign in to Xcode with a free Apple ID. That gives you a personal development certificate, and every build signs with it. The signature stays the same, so approvals stick. You set it up once.

**Things to check on a work Mac**
- **Device management:** even with full control of the laptop, check whether your company's management profile blocks unsigned apps or personal Apple IDs.
- **Signing without your personal Apple ID:** to keep separation strict, you can create a separate free Apple ID just for signing. It never syncs anything; it only issues the certificate.

So I'd go with SwiftUI + GRDB. The setup is a one-time chore, and Tauri wouldn't avoid any of it on a Mac.

Should I sketch the database schema next (sources, items, topics, links, people, outbox, reflections), for you to mark up before the build thread?

## User

I was actually hoping to just hand off the next part to Claude Code in the Cloooouuuuuud ☁️ because of having that option, but I'm guessing there's no MacOS build tools available there?

The Keychain access is a bit of a pain. Any way to keep the sig stable?

I believe I can sign in to a personal one (or just make a work one). Gross, but doable.

Making me think a web UI is better (almost certainly not)

## Assistant

That changes my recommendation back to **Tauri**.

**Claude Code in the cloud**
- It runs on Linux, which has no Xcode and no SwiftUI. It could write Swift, but couldn't build or run the app, so every UI change would wait for you to build it locally.
- With Tauri, almost everything builds and runs on Linux: the Rust core, the database, the connectors and the AI calls. The UI is WebKit on both Linux and Mac, so what the cloud sees is close to what you'll see.
- Mac-only pieces like Keychain and the global hotkey sit behind a small layer. The cloud uses stand-ins, and they're tested for real on your Mac.
- You'd only build the actual `.app` on your Mac. GitHub's macOS build machines are an option too.

**A stable signature (same answer for Swift or Tauri)**
- Sign every build with the same certificate: an Apple Development certificate from a free Apple ID, created once. macOS recognizes the app by that certificate plus its app ID, not by the exact build. So Keychain and Calendar access survive rebuilds.
- In Tauri that's one line in its config, pointing at the certificate.
- If you'd rather skip Keychain entirely, keys can live in a file only your user can read, in the app's support folder. For a personal tool that's fine. Just keep it out of anything that syncs.

**A plain web UI (a browser tab talking to a local server)**
- It would work, but you'd lose the global capture hotkey, menu bar, reliable notifications and a proper app window.
- Tauri already gives you the web-style development loop without losing those, so I agree: almost certainly not.

**Handoff plan:** Tauri with a Rust core, and SQLite through sqlx or rusqlite. Build it in the cloud, and package and sign it on your Mac.

Should I write the handoff doc next, with the stack, phases, source list, writeback rules and open questions for the build thread?

## User

Yes.

## Assistant

_[tool: read_skill_prompt]_

_[tool: list_files]_

_[tool: grep]_

_[tool: run_script]_

_[tool: write_file]_

_[tool: copy_files]_

_[tool: present_fs_item_for_download]_

The handoff package is ready to download above: `README.md` plus the design files. The README covers the stack, the work/personal boundary rules, phases, sources, the confirm-before-delete outbox for writeback, a database sketch and the specs for each view.

Four things to check before you hand it over:
- **Work calendar:** I left it open between the Google Calendar API and macOS Calendar access.
- **Category meanings:** I inferred purple and green (purple = self-holds like Lunch/Walk, green = Jobs), so correct them if they're wrong.
- **GitHub icon:** there isn't one yet; add one in the same desaturated style.
- **Screenshots:** they aren't included. I can add some if you want them.

## User

Yes, please include screenshots. That will be useful for the agent building to compare against.

## Assistant

_[tool: show_to_user]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: snapshot_element]_

_[tool: view_image]_

_[tool: view_image]_

_[tool: str_replace_edit]_

_[tool: present_fs_item_for_download]_

I added ten screenshots in `screenshots/`, one per reference view, with a Screenshots section in the README. It warns the building agent about two things. A few labels wrap or overlap in the screenshots but not in a browser (e.g. "Search everything"). And the older rounds still show personal events with titles, which the rules for keeping personal data off the work Mac now override.

