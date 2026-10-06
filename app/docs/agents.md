# Agents

Coding-agent sessions from two kinds of source, in one tree (design "Agents Final" 01–08; spec in `project/design_handoff_helm/README.md` → Agents). Code: `src-tauri/src/agents/`, `src/views/Agents.svelte`, `src/views/agents/`. Tab 7 (`⌘7`), or `g a` from anywhere (`g i` back to Incoming). Local agents run in a separate agent host, so they keep going when the app quits or rebuilds. The tab's badge is the Needs you count.

- **Local ACP sessions.** Alethiometer is the ACP client and launches the agent itself (`agents.harnesses`). It sees only sessions it started; one Zed launched belongs to Zed.
- **Scion agents.** Alethiometer is a Hub client (`agents/scion.rs`, a `scion` source in Settings › Sources). Rooms, DMs and delivery states come from the Hub. There's no messaging layer here, and local sessions never join rooms.

## How it fits together

The core keeps the tab's state in memory (`agents/store.rs`) with a revision counter; local agents themselves run in the agent host (`agents/daemon.rs`, `app/agentd`). The UI polls `agents.poll { rev, thread, thread_rev, tab }` every 400 ms while the tab is open and gets the tree when it changed and the open thread's changed rows. Local groups, sessions and transcripts are flushed to SQLite once a second (`0017_agents.sql`, rows as JSON), not per streamed chunk, so a busy agent doesn't contend for the one database connection. Scion state isn't stored; it comes back from the Hub.

`agents/model.rs` is the shared shape: `Group`, `Session`, `Row` (one row grammar for rooms and transcripts), `Request` (a pinned approval or ask). A thread is `s:<session id>` or `r:<room id>`.

## Local sessions (ACP)

`agents/acp.rs` speaks ACP v1 (JSON-RPC, one message per line, through the agent host to the adapter's stdio), hand-written on std threads: the official `agent-client-protocol` crate needs an async runtime the core doesn't have.

- **Harnesses** (`agents.harnesses`, settings.json only): `claude` runs `npx -y @agentclientprotocol/claude-agent-acp`, `pi` runs `npx -y pi-acp`. Both need Node 22+; Pi also needs `pi` on the PATH. The command is looked up on the login shell's PATH (`$SHELL -ilc`), since apps started from the Dock get a minimal one. Each harness signs in its own way (Claude's login, Pi's providers); Alethiometer passes no credentials. A not-signed-in agent fails the first prompt; the transcript says so.
- **Thinking:** the agent's ACP config option of category `thought_level` (Claude's `effort`, pi's `thought_level`) is kept on the session (`Session.thinking`, refreshed by `config_option_update`). The new-agent form offers its levels (as the harness lists them for its default model) and sets one after the model; the status line's picker changes it on a live session (`agents.set_thinking`, `session/set_config_option`). Agents without one show neither.
- **Client capabilities:** form elicitation only. No `fs/*` or `terminal/*`, so agents use their own tools (both adapters work this way). Agent requests other than `session/request_permission` and `elicitation/create` get "method not found".
- **Mapping** (`apply`): `agent_message_chunk` → Output (chunks join until something else happens), `agent_thought_chunk` → Thinking (tokens are a ~4-chars estimate; ACP doesn't count them), `tool_call`/`tool_call_update` → one Tool row updated in place (name and target from claude-agent-acp's `_meta.claudeCode.toolName` and `rawInput`, or pi-acp's bare titles; a read shows "N lines", an edit "+a −r" and "applied"; pi's terminal output streams into the result), `plan` → one State row updated in place, `usage_update` → context %, model, cost (claude-agent-acp's running total), `session_info_update` → title.
- **Subagents** (Claude's Task tool, `_meta.claudeCode.subagent`): a message box in the parent ("to settings-scan · task", the folded "N tool calls · read ×3 …", "from settings-scan · result") and its own session under the parent in the tree. Inner updates carry `_meta.claudeCode.parentToolUseId` and go to the subagent's thread.
- **Prompts:** a message sent while the agent is busy isn't a row yet — a State row ("N message(s) queued") stands in for it, and the composer's text is held client-side, not sent concurrently (firing it alongside the running turn let a connector's own cancel wipe both at once). It becomes a real row, and is actually sent, only once the running turn ends: by finishing normally, or by `⌘.`/Interrupt cancelling it (which also cancels any open approval). While a message is held back, a **Send now** button (`⌘↵` on an empty box) sits next to Interrupt: during a turn it cancels it, with nothing running it sends the message at once. Cancelling the running turn is exactly what lets the held-back prompt go out next, landing in the stream at the point it's actually sent rather than when it was typed.
- **Outliving the app:** agents run in the agent host (below), so quitting, crashing or rebuilding the app leaves them running, mid-turn or waiting on an approval; the next start attaches again and answers what's pending. Quit no longer stops agents: **Stop** in the composer stops one, **Stop all** at the bottom of the tree stops every one. An agent that exited (stopped, or the machine restarted) shows its stored transcript; sending to it starts the harness again and resumes the ACP session (`session/resume`, else `session/load` with the replay dropped, since the transcript is already stored). A harness that can do neither says so.
- **Open in Zed** opens the session's directory; the terminal button opens a terminal there.

### Approvals

A `session/request_permission` pins a card above the composer and lists the session under Needs you. Its buttons are the **agent's own options**, labelled with the agent's own names (a connector that gets an option's kind wrong can't make "No" read as "Allow once"), keyed `1`, `2`, `3`… in the order the agent sends them (matching the Ask card's numbered choices, and keeping Needs you keys numeric rather than mnemonic letters that collide with other single-letter shortcuts): allow once, an allow-always whose name says it's for the session, any other allow-always (claude-agent-acp's "Yes, and don't ask again for …", which Claude Code saves in its own settings), and deny (`Deny…` takes an optional reason: if the option you pick asks for one (pi's "No, provide reason" follows up with an `elicitation/create`), it answers that question; otherwise it goes out as your next message when the turn ends). Alethiometer keeps no allowlist of its own (the user's call): whatever "Always" means, the agent keeps the rule.

- **Edits inside the project don't ask** (`agents.accept_edits`, on by default, Settings › Agents): after a session starts, Alethiometer switches it to the agent's own accept-edits mode (`session/set_mode` → Claude Code's `acceptEdits`). The agent applies the rule, so edits outside the project and every shell command still ask. Pi has no permission prompts of its own; extensions gate tools with `ui.select`, which pi-acp sends as a permission request.
- **Parsed** (`agents/shell.rs`): a deterministic parse of the command with `tree-sitter-bash`: numbered steps (nested `sh -c`, `xargs … sh -c`, `find -exec`, `$(…)` re-parsed as lettered substeps), tags per step (reads files, reads git, writes files, writes git, deletes, network, shell per file, runs shell, local bin, installs, env, unknown command, sudo), notes under a step (the loop or conditional it sits in, and what each unquoted wildcard matches in the directory right now, read from disk when the card opens; both also go to the explainer), and checks (writes, paths outside the repo, network, shells per file, code it can't see into, unknown commands, sudo, credentials read). The verdict badge: unknown (anything unparsed, unknown, or code it can't inspect such as `python x.py` or `npm test`), else network, else writes, else read-only. Reading `.env`, `~/.ssh/…`, tokens or dumping the environment stays read-only but warns "Reads credentials". The command table and checks are ours; no library classifies commands. `tests/fixtures/shell/corpus.jsonl` (387 labelled commands from godspede/construct-auto-classifier, Apache-2.0) checks it never calls a dangerous command read-only.
- **Explained** (`agents/explain.rs`): the small AI tier reads the command **as if it was planted**: the realistic threat is an agent steered by prompt injection in something it read. It sees the command, the parse, your latest prompt, your last 10 answers to the agent's questions (each with the question's text, labelled as the agent's wording) and any project scripts the command runs (`python x.py`, `node x.js`, `bash x.sh`, `npm run x` → the file or the package.json script, inside the project only, clipped to 200 lines), and **never anything the agent wrote about the command** (its thinking, its output, a tool description), since a steered agent's words are where a planted "this is just a formatting check" would be. It answers in JSON: what it does, whether it goes beyond your request, up to three concrete ways it could be misused (none when it's benign) and the worst case. It never says allow or deny or suggests a rule, since the buttons can't apply one. Cached per command, directory and script contents; "Off" when the small tier is off. Not yet seen against a live model here (no Bedrock credentials in the build environment); the reply parsing is tested.
- **The request is an update** to a tool call the agent already sent (ACP's `ToolCallUpdate`): what it leaves out (kind, input, tool name) comes from that call, so a gate that repeats only the call's id still gets Parsed and Explained. Text in the request's own `content` (a gate's prompt, a description) shows under the command as the agent's words; the explainer never sees it.
- **The card keeps the session in view** (`ApprovalCard.svelte`, `NeedCard.svelte`, `Pane.svelte`): the transcript always keeps `min(160px, 25vh)` (about five short rows; a quarter of a short window), and the card, at most 55vh, shrinks first. Only Parsed and Explained give way: the area has a floor of 80px and each panel scrolls on its own, so scrolling one doesn't move the other. The command block (160px, scrolls), the buttons, the composer and the status line keep their size. Below about 800px of window height even that isn't enough (the command and the buttons alone are ~250px): the card then scrolls as a whole and its answer buttons stay stuck to its bottom edge. The ask card isn't capped and isn't affected.
- The Needs you tag: `low` (read-only), `writes`, `network`, `unknown`, `outside repo`, `question` (asks).

### Questions

The client advertises form elicitation (`clientCapabilities.elicitation.form`), so an agent's questions arrive as `elicitation/create` (Claude Code's AskUserQuestion; pi's questions and text input through pi-acp). Each pins an **Ask card** and a row in the transcript; nothing is parsed or explained, there's no command.

- One single-choice field (with or without an optional free-text "Other"): the options are choice cards, keyed `1`–`9` in the agent's order; pi's "1. Label - what it means" labels split into title and detail. Picking one answers with that option's own value.
- Write a reply… (`R`) opens a field in the card, as a deny reason does on an approval (Enter sends, Esc goes back). So does picking pi's "Type something." choice (`Choice.free`): the words go with that choice and answer pi's follow-up question, so no second card. The reply otherwise fills the first free-text field (Claude's "Other", pi's text input). When the question has none, it declines the question and sends your text as the next message.
- Anything else (several questions, multi-select, numbers, yes/no): the fields as a small form with Send answer.
- Questions wait in the turn state like approvals, so they survive an app restart. ⌘. cancels them.

## In Incoming

Design "Agents Incoming" 2a–2c (`project/design_handoff_helm/README.md` → Agents in Incoming). Agent signals are items of a built-in `agents` source (kind `agents`, tag work, made on first use; never polled, not in the sync status), so Incoming files, confirms, moves and dismisses them like any signal, and its badge counts them. `agents/incoming.rs`.

- **What comes in:** waiting on you: an approval or ask (`req:<request>`), a Scion message the Hub didn't deliver (`undelivered:…`, "didn't get your message") or refused ("didn't take your message"), a session that failed (`failed:<session>:<time>`). Not waiting: Scion messages to you since your last word in that thread (`wrote:…`, one signal per run of them), the end of each local turn (`turn:<session>:<turn>`, your last message faded before the agent's final output), a Scion agent that finished (`done:<session>`, with task, time, turns, PR, ticket). Subagents never show up by themselves.
- **Kept in step with the store**, about once a second while it changes (`incoming::start`): a signal whose cause is gone (answered, replied to, delivered, restarted) is marked seen with `$.auto_seen`; if the cause comes back (an approval rebuilt after a restart) so does the signal. One you dismissed stays dismissed. A local session whose transcript isn't loaded yet is left alone. Incoming polls `agents.incoming_rev` every 2 s and reloads when it moves.
- **Topic:** the session's ticket (the facts box's) → the topic its Jira item is filed under; else the topic of the session's earlier signals; else the small tier's usual suggestion (at most once a minute). The AI sees the group, its goal and the first task, or the directory and first task.
- **Where:** an **Agents** app group after GitHub, oldest first; and **Agents waiting** under the rule next to Unsorted: every waiting agent signal across topics, grouped by project, each naming its group and topic. `c` confirms the signal's own topic there (picks one if it has none).
- **Row:** the agent's status mark (live, as in the tree) in the gutter, its name in its colour and the reason, then `project › group · time`. Reply `r` on messages and turn ends and the PR on a finished agent sit before the divider. The row's actions touch only the signal.
- **Open (↵):** the messages in Incoming's layout, a line on why it's here, then the agent's answers on number keys (only while open): an approval's command with the same Parsed and Explained panels as the Agents card (`Panels.svelte`) and the agent's own options (a deny asks for an optional reason in place); an ask's choices with their details and Reply… `r`; undelivered: Start it again and send, Send to another agent, Discard; refused: Retry, Edit (back to the Agents composer), Cancel; failed: Open in Agents, Open on Hub / in Zed. A reply to a turn end is the session's next prompt (it resumes the session if needed). `o` opens the session in Agents. `views/incoming/AgentPart.svelte`.
- **Answered here:** a green check and the answer replace the actions, the status mark follows the agent, and the row goes when you move off it.

## Agent host (`app/agentd`)

A small separate program, `alethiometer-agentd`, runs the local agents' adapters and owns their stdio. It speaks no ACP: it passes lines through, numbers every line an agent writes, and keeps them in a log per agent (`<data dir>/agentd/logs/<id>.jsonl`). The app talks to it over a Unix socket (`<data dir>/agentd.sock`, or one in the temp directory when that path is too long), one JSON message per line (`agentd/src/proto.rs`): a control connection for requests (`spawn`, `write`, `kill`, `list`, `forget`, `shutdown`) and a subscription for events. Its own log is `<data dir>/agentd/agentd.log`.

- **Picking up after a restart:** the store keeps, per agent, how far into its log the app has read and the turn state (open rows, tool calls, prompts and approvals in flight: `acp::Turn`, table `agent_hosts`). A new app asks the host what's running, reattaches those agents with the approvals they're waiting on, and subscribes from where it left off, so nothing is applied twice and nothing is missed. Prompts are tracked by JSON-RPC id in the turn, so an answer that arrives in the next run still finishes the turn there. ACP's own remote transport (a draft) doesn't replay missed messages yet; the host's log does.
- **Separate on purpose:** it's its own crate with two dependencies and no database, settings or sync, so app changes don't rebuild it. The app runs a **copy** in `<data dir>/bin/`, started in its own process group: a rebuild, `cargo clean`, `tauri dev` restart or a closed terminal can't kill it or delete it from under it.
- **Starting:** the app starts it on demand, the first time a local session needs it (no login item, no launchd). With nothing running and no app connected it exits after 10 minutes.
- **Builds:** the app takes the first build it finds: `ALETHIOMETER_AGENTD`, next to the app (the copy `npm run build:mac` puts in the bundle), then `app/agentd/target/{debug,release}` of the repo it was built from (the newer). When that build differs from the copy and no agent is running, it swaps the copy and restarts the host; with agents running it waits and the tree's footer says "a newer build is ready" with **Restart host** (which stops them; it asks twice). A protocol mismatch (`proto::PROTOCOL`) shows there too.
- **Platforms:** macOS and Linux (Unix sockets). Windows would need a named pipe; until then local agents aren't available there.
- **Signing and security tools:** the copy keeps the build's signature (ad-hoc from `npm run build:mac`); it listens on no network port (no firewall prompt); it starts the same commands the harness settings name, as the app did before. A managed Mac's endpoint policy could still object to a long-lived unsigned process; that's untested.

### In development

```sh
cd app
npm run agentd            # builds app/agentd (debug); npm run devserver and npx tauri dev run this first
npm run devserver         # browser mode; the app finds app/agentd/target/debug and copies it to the data dir
npx tauri dev             # desktop; beforeDevCommand builds agentd too
npm run build:mac         # release .app with the agent host inside (re-signed with signingIdentity)
cd agentd && cargo test   # the host's own tests
```

- Rebuilding the app (Rust or Svelte) leaves agents running; the restarted app reattaches. Rebuilding **agentd** is picked up the next time the app connects with nothing running, or by **Restart host**.
- Dev and the real app keep separate hosts as long as they use separate data directories (`ALETHIOMETER_DATA_DIR`): the socket and the copy live in the data directory.
- To see what the host is doing: `<data dir>/agentd/agentd.log`, the logs in `<data dir>/agentd/logs/`, and `alethiometer-agentd --version`. To stop it by hand: **Restart host**, or kill the pid in `<data dir>/agentd/pid` (agents stop with it; the next app start brings the host back).
- `tests/agents_restart.rs` runs the host in-process with `scripts/fake-acp.mjs` as the agent (skipped without `node`):
  - an approval asked in one App is answered in the next, on the same agent process and JSON-RPC request;
  - the devserver binary is `kill -9`'d while the agent streams numbered chunks (`FAKE_ACP_MODE=stream`), and the next one shows every chunk exactly once, both when it comes back after the turn ended and when it joins mid-stream. This is what caught the flush writing the log position and the rows in separate snapshots (`Store::gate` now makes them one).
- `ALETHIOMETER_AGENTD_KEEP=1` makes the app use whatever host is on the socket and never install, swap or start one; the tests set it so the app under test doesn't replace the test's host.
- The host stops an agent by signalling its process group with `killpg`, never by running `kill(1)`: procps-ng's `kill -TERM -<pid>` sends to pid −1, every process the user owns.

## Scion agents

`agents/scion.rs` (+ `scion/api.rs`, `scion/map.rs`), one thread per enabled `scion` source (Settings › Sources: Hub URL, a `scion_pat_…` user access token as `scion.token`, optional project filter). Built from Scion's Go source at `2220c46` (GoogleCloudPlatform/scion, v0.3 preview, "grove" already renamed "project"); **never run against a live Hub**. Fixtures in `src-tauri/tests/fixtures/scion/` are hand-written from the Go JSON tags (the README there maps each to its Go type). The Hub's state shows in the tree (a red line when it can't connect), not in the sync chip.

- **Polling, not SSE:** the Hub's stream is per agent, while one conversation list per project covers every room and DM. Every 3 s: `GET /conversations?include_project_groups=<project>`, then the messages of conversations whose `lastActivityAt` moved (or that hold a send still settling). Every 15 s (or right after you act): `/auth/me`, `/projects`, `/projects/{id}/agents`, room participants, and agent ↔ agent mail (`/chat/conversations/dm:…/interagent`) for agents whose activity moved.
- **Tree:** a project's group threads (conversations of `kind: group`) are groups with a room row; their agent participants are the group's sessions; other agents sit in a group named after the project. An agent in several threads goes under the most recently active one; `ancestry` gives a subagent's parent.
- **Status:** failed for phase `error` or activity `limits_exceeded` / `stalled` / `crashed`; waiting for an open ask or `waiting_for_input` / `blocked`; running while starting or `working` / `thinking` / `executing`; done for `completed`; idle otherwise.
- **Rows:** `instruction` / `reply` / `mention` are messages (mention copies fold into one row; the ✓ count is recipients dispatched), `state-change` State, `system` a Hub notice, `input-needed` an ask (choices read from numbered or bulleted lines, keyed 1–3; it closes when the agent posts again, you send it something or answer it here, or it stops). `assistant-reply` (a copy of each turn's final text) is the session's Output. **A Scion session's thread is its messages and state changes only:** the Hub exposes no structured harness transcript (`/agents/{id}/logs` is raw `agent.log`).
- **Delivery (08):** per message, `dispatchState` `pending | dispatched | failed | deferred` with `dispatchFailureReason`. Unreachable is `failed` with "Agent unreachable (<phase>)". "Deferred · X was mid-turn" is inferred (dispatched, X busy, X hasn't posted since); the Hub's own `deferred` only means the agent is being reincarnated. There's no retry time on the Hub, so "next" never shows. Discard, Cancel and Edit only hide the row here; nothing is deleted on the Hub. Start it again and send starts the agent, waits up to 2 minutes for it to run, then sends.
- **Sending:** to a room, `POST /chat/conversations/{topic}/messages` (native chat; routing is the Hub's: @mentions, else the thread's default agent); a DM, `POST /agents/{id}/message {structured_message:{msg,type}}`. `⌘.` sends the harness's interrupt key (`POST /agents/{id}/keys`: Escape for Claude, C-c for others).
- **The New agent modal (05)** (`N`, the + in the filter row, or Agent in a room's composer) covers the whole tab below the title bar and takes the keyboard (Tab stays inside it, ⌘⏎ starts, Esc closes). A **Joins** block at the top draws the group it lands in as the tree does: goal and host, the room while "Add to #room" is on, the members with their status marks, and the new agent dashed with the Name field's name. A local agent's form has the harness, a **Model** dropdown (the models the harness reports from a brief `session/new`, cached per harness; default is the harness's own; applied with `session/set_model`, else the `model` config option), name, directory, group and task. `NewModal.svelte`, `Joins.svelte`, `NewLocal.svelte`, `NewScion.svelte`.
- **New agent (05):** templates (same-named templates in different scopes are the "versions"), harness configs and brokers come from `/templates`, `/harness-configs` and `/projects/{id}/providers`; Start is `POST /agents`, then "Add to #room" `POST /conversations/{id}/participants` (if that's refused, it posts "@name joined this thread", since a mention also adds the agent).
- **Terminal:** `scion attach <slug> --hub <url> -g <project>`.

`scripts/fake-scion-hub.mjs` serves the fixtures on `http://127.0.0.1:1431` and keeps posts in memory, for UI work (any value for the `scion.token` secret).

### To check against a live Hub

1. Do UAT tokens reach `/api/v1/chat/*`, and is native chat on? Without it, room posts and agent ↔ agent mail don't work.
2. The scopes the token needs: `project:read`, `agent:read`/`list`, `agent:message`, `agent:create`, `agent:lifecycle`, `agent:attach` (interrupt, agent ↔ agent mail).
3. Does `POST /agents/{id}/message` take `{structured_message:{msg,type}}` with no version or sender (the web UI sends only `msg`), and `type: "reply"` from a user?
4. Default order and largest `limit` of `/conversations/{id}/messages` (assumed newest first, like `store.ListMessages`).
5. The sender and recipient on an agent's own post to a thread, and the system sender (guessed `system:hub`).
6. Does `include_project_groups` list every project group to a user who isn't a participant?
7. Does `runtimeBrokerId` take a name? Names are resolved to ids from providers or agents.
8. Do your DMs with agents appear in `GET /conversations` as `kind: direct`, and do mention copies share `createdAt` exactly?
9. `start` on a stopped agent and `/keys`; whether `pi` is a harness name (it isn't in this checkout).
10. Unread counts are always 0 (the conversations API has no read state; chat thread lists have `hasUnread`), and threads have no goal, so a room's header is its name.
11. The loop cap is Scion's hard-coded 10 mentions per agent pair per 10 minutes; suppressed mentions aren't stored, so no notice appears in the room unless the Hub posts one.
12. How the Hub adds a newly started agent to an existing group thread (the design's open question; participants are tried first).

## Tree

Projects (the repo of a local group, the Scion project), groups, and rows. Local groups are made in the New agent modal ("New group…", with a goal); a session with no group is under **Unsorted**, where the small tier guesses a group from its directory, title and first prompt ("Looks like alethiometer › Agents tab · Move · Not this", `agents/guess.rs`). Nothing moves until you press Move. A folded group shows its loudest status and count. Status marks: running dot, waiting half ring, idle ring, done check, failed x.

## Facts box

Ticket, PR, branch and todo for a session (`agents/facts.rs`), refreshed when a session starts and after each turn: the branch from git in its directory (and how far ahead of upstream), the ticket as a Jira key in the branch (any case), title or first prompt (upper case only, so "utf-8" isn't one) and that issue's status from the synced Jira items, the PR as a synced GitHub PR for that branch or key, the todo as an open todo naming the key. A room shows its members' facts.

## Thread

Rows follow the row grammar (icon column coloured by kind, mono header, body). A session's own rows drop its name (`agents.names.self_in_transcript` brings it back). A run of messages between the same two agents folds to its last message ("N earlier · E"); the latest run stays open in one box (`agents.boxes.fold_pairs`). Thinking folds to one italic line; long tool output to three lines (`e`). The legend chips filter kinds. Scion sessions have a Messages tab (`t`/`m`): you ↔ the agent, your mentions of it in rooms, its asks.

## Keys

`⏎` goes in, `Esc` comes out, `Tab` cycles tree → thread → composer. `?` shows the sheet (06).

| Where | Keys |
|---|---|
| Everywhere | `⌘K` search, `/` filter sessions, `g i` / `g a`, `⌘.` interrupt, `?` keys |
| Tree | `j`/`k`, `h`/`l` fold or open, `⏎` open, `n` new agent (Scion group) or session (local), `a` archive, `u` undo an archive (10 s) |
| Thread | `j`/`k`, `e` fold/open, `⏎` reply (`@name` in a room) or open a tool call, `c` copy, `g` latest, `t`/`m` tabs |
| Needs you | `o` `s` `a` `d`, `1`–`3` pick an answer, `r` write a reply, `Esc` later |
| Composer | `⌘⏎` send, `@` address one member (rooms), `↑` bring back your last message, `Esc` back |
| Archive confirm | `⌘⏎` archive, `⌘⌫` archive anyway (not done), `Esc` cancel |
| Archived (read-only) | `⌘G` / `⇧⌘G` next or previous match, `Esc` back |
| Undelivered (08) | unreachable: `⏎` start it again and send, `f` another agent, `Esc` discard; failed: `⏎` retry, `e` edit, `Esc` cancel |

Single-letter keys are off while typing.

## Settings › Agents

Section 6 (Developer is 7). A live preview, then one row per key: `agents.accept_edits`; `agents.colors.you|tool|message|hub|needs_you` (presets, `←`/`→`); `agents.names.color_by` (agent, template, off) and `agents.names.palette` (unique within a group, reused across groups, assigned in order of creation and kept in `agent_colors`); `agents.names.self_in_transcript`; `agents.boxes.strength` (subtle .07/.35, normal .10/.45, strong .14/.6) and `agents.boxes.fold_pairs`. `⌫` resets a row. `agents.harnesses` is settings.json only.

## Development

`scripts/fake-acp.mjs` is a scripted ACP agent shaped like claude-agent-acp (thinking, reads, a subagent, an edit and a shell command that ask, output; honours `acceptEdits`). Point a harness at it in a dev data dir:

```json
"agents": { "harnesses": [{ "key": "claude", "label": "Claude Code", "command": "node", "args": ["<repo>/app/scripts/fake-acp.mjs"] }] }
```

Recorded claude-agent-acp traffic for the mapping tests is in `src-tauri/tests/fixtures/acp/`.

## Archive

Design "Agents Archive" 01–05 (`project/Helm Agents Archive.dc.html`, spec in `project/design_handoff_helm/README.md` → Archiving agents). Code: `src-tauri/src/agents/archive.rs` (rules), `Store::archive` in `agents/store.rs`, the repository in `store/agents.rs` (table `agent_archive`, migration `0033`), `sync/connectors/scion.rs` (the Hub delete), and in the UI `ArchiveModal.svelte`, `ArchivedPane.svelte` and the tree (`Tree.svelte`).

- **What:** `a`, or right-click › Archive, archives the selected row and only that row. A session takes its subagents with it and leaves its group. A group, or its room row, takes every member.
- **When:** nothing running or waiting for you (an open approval or ask) can be archived; a group with such a member says Can't archive and names how many are going. A **Scion** group or session that stopped without finishing needs **Archive anyway** (`⌘⌫`, the one red button), since deleting its Hub agents loses their clones. Why it isn't done is listed: unpushed commits (from the branch fact; the Hub doesn't report them), PRs not merged, tickets not in Jira's done category, failed agents, agents that stopped before completing. **Local** sessions never ask (user, 2026-10-05: allowed to archive half-done work; there's no reliable "done" for them yet, see ISSUES.md).
- **Confirm (02):** `agents.archive_plan { target }` returns the tree as it will change (rows lit and tagged), the reasons and the counts. `⌘⏎` archives, `Esc` cancels, `⏎` never archives.
- **Undo (03):** the row stays struck through on the hatch for 10 s with "deleting N agents in 8s · u undo"; `u` takes back the newest. The window lives in the UI: only when it ends does `agents.archive { target, force }` run, and it checks the rules again.
- **Archiving** stops local sessions (their process in the agent host), then in one unit of work: pending changes are flushed, each group and session moves from `agent_groups` / `agent_sessions` (and `agent_hosts`) into `agent_archive` as it is now, a Scion session's thread and a Scion group's room are copied into `agent_rows` from memory (they're never stored otherwise), and one `scion.delete_agent` per Hub agent goes into the outbox (`DELETE /api/v1/agents/{id}`, the Hub's defaults: clone and branch go too; a 404 counts as deleted, so it's safe to resend). It runs under the store's gate, so no event lands between the copy and the removal. Archived ids are kept in the store, and `put_group`, `put_session`, `put_request` and `set_rows` ignore them, so a Hub that still lists an agent can't bring it back.
- **A failed Hub delete** leaves the item archived (user, 2026-10-05). The outbox row stays failed (`⌘R` in the sync status retries it) and the tree's footer says "N Hub deletes failed".
- **Incoming:** an archived session's open signals leave Incoming (marked `auto_seen`, as for any signal whose cause is gone). Filed ones stay filed; replying to one fails, since an archived session can't be restarted.
- **Done nudge (01):** a green frame around a Scion group whose every session completed and whose PR merged (or ticket is Done), with **Archive group** and **Not now** (hides it for 7 days, `agent_nudges`). Local groups never get one. Nothing is archived automatically.
- **Finding archived items (04):** the tree filter (`/`) also searches the archive (`agents.archived { q }`: names, goals, titles, and the text, tool targets and tool results of their rows), shown under an "Archived" divider, faded until the pointer or the selection is in it. `⏎` opens one read-only (`agents.archived_open { id }`): an "archived <day>" chip, the rows with the filter's matches highlighted (CSS Custom Highlight API; `⌘G` / `⇧⌘G` next and previous), a group's members to open, and a footer with what was kept and Back (`Esc`). No restart, no reuse, no unarchive. ⌘K doesn't list archived items.
- **Right-click (05):** selects the row and opens its menu: Open, Stop, New agent in <group>, then Archive session… / Archive group… (off with "stop it first" while something under it is going).

## Not built

- Renaming or deleting a local group from the UI (`agents.group_update` / `group_delete` exist), and moving a session other than through the Unsorted guess (`agents.move`).
- Elicitation in URL mode (declined), and pushing an elicitation's `_meta` previews (Claude's option mockups) into the card.
- Native ACP subagent sessions (`clientCapabilities.subagents`): the legacy Task shape is used.
- The cache state is an estimate (ACP doesn't report it): `agents.harnesses[].cache_ttl_min` minutes after the last turn, 5 for Claude Code when unset, nothing for other harnesses.
