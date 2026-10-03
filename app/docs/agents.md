# Agents

Coding-agent sessions from two kinds of source, in one tree (design "Agents Final" 01–08; spec in `project/design_handoff_helm/README.md` → Agents). Code: `src-tauri/src/agents/`, `src/views/Agents.svelte`, `src/views/agents/`. Tab 7 (`⌘7`), or `g a` from anywhere (`g i` back to Incoming). The tab's badge is the Needs you count.

- **Local ACP sessions.** Alethiometer is the ACP client and launches the agent itself (`agents.harnesses`). It sees only sessions it started; one Zed launched belongs to Zed.
- **Scion agents.** Alethiometer is a Hub client (`agents/scion.rs`, a `scion` source in Settings › Sources). Rooms, DMs and delivery states come from the Hub. There's no messaging layer here, and local sessions never join rooms.

## How it fits together

The core keeps the tab's state in memory (`agents/store.rs`) with a revision counter. The UI polls `agents.poll { rev, thread, thread_rev, tab }` every 400 ms while the tab is open and gets the tree when it changed and the open thread's changed rows. Local groups, sessions and transcripts are flushed to SQLite once a second (`0017_agents.sql`, rows as JSON), not per streamed chunk, so a busy agent doesn't contend for the one database connection. Scion state isn't stored; it comes back from the Hub.

`agents/model.rs` is the shared shape: `Group`, `Session`, `Row` (one row grammar for rooms and transcripts), `Request` (a pinned approval or ask). A thread is `s:<session id>` or `r:<room id>`.

## Local sessions (ACP)

`agents/acp.rs` speaks ACP v1 (JSON-RPC over stdio, one message per line), hand-written on std threads: the official `agent-client-protocol` crate needs an async runtime the core doesn't have.

- **Harnesses** (`agents.harnesses`, settings.json only): `claude` runs `npx -y @agentclientprotocol/claude-agent-acp`, `pi` runs `npx -y pi-acp`. Both need Node 22+; Pi also needs `pi` on the PATH. The command is looked up on the login shell's PATH (`$SHELL -ilc`), since apps started from the Dock get a minimal one. Each harness signs in its own way (Claude's login, Pi's providers); Alethiometer passes no credentials. A not-signed-in agent fails the first prompt; the transcript says so.
- **Client capabilities:** none. No `fs/*` or `terminal/*`, so agents use their own tools (both adapters work this way). Incoming requests other than `session/request_permission` get "method not found".
- **Mapping** (`apply`): `agent_message_chunk` → Output (chunks join until something else happens), `agent_thought_chunk` → Thinking (tokens are a ~4-chars estimate; ACP doesn't count them), `tool_call`/`tool_call_update` → one Tool row updated in place (name and target from claude-agent-acp's `_meta.claudeCode.toolName` and `rawInput`, or pi-acp's bare titles; a read shows "N lines", an edit "+a −r" and "applied"; pi's terminal output streams into the result), `plan` → one State row updated in place, `usage_update` → context %, model, cost (claude-agent-acp's running total), `session_info_update` → title.
- **Subagents** (Claude's Task tool, `_meta.claudeCode.subagent`): a message box in the parent ("to settings-scan · task", the folded "N tool calls · read ×3 …", "from settings-scan · result") and its own session under the parent in the tree. Inner updates carry `_meta.claudeCode.parentToolUseId` and go to the subagent's thread.
- **Prompts:** both adapters queue a prompt sent mid-turn, so the composer always sends. A message sent while the agent is busy shows "deferred", then "delivered HH:MM" when its turn starts. `⌘.` sends `session/cancel` and cancels any open approval.
- **Outliving the app:** closing the window hides it while any local agent runs (desktop); only Quit stops them, and Quit kills each adapter's process group. After a restart a session shows its stored transcript and isn't attached; sending to it starts the harness again and resumes the ACP session (`session/resume`, else `session/load` with the replay dropped, since the transcript is already stored). A harness that can do neither says so. A separate agent daemon would let sessions survive Quit too; the client is behind one `Host`, so it can come later.
- **Open in Zed** opens the session's directory; the terminal button opens a terminal there.

### Approvals

A `session/request_permission` pins a card above the composer and lists the session under Needs you. Its buttons are the **agent's own options**: `O` allow once, `S` an allow-always whose name says it's for the session, `A` any other allow-always (claude-agent-acp's "Yes, and don't ask again for …", which Claude Code saves in its own settings), `D` deny (`Deny…` takes an optional reason, sent as your next message). Alethiometer keeps no allowlist of its own (the user's call): whatever "Always" means, the agent keeps the rule.

- **Edits inside the project don't ask** (`agents.accept_edits`, on by default, Settings › Agents): after a session starts, Alethiometer switches it to the agent's own accept-edits mode (`session/set_mode` → Claude Code's `acceptEdits`). The agent applies the rule, so edits outside the project and every shell command still ask. Pi never asks for permission at all.
- **Parsed** (`agents/shell.rs`): a deterministic parse of the command with `tree-sitter-bash`: numbered steps (nested `sh -c`, `xargs … sh -c`, `find -exec`, `$(…)` re-parsed as lettered substeps), tags per step (reads files, reads git, writes files, writes git, deletes, network, shell per file, runs shell, local bin, installs, env, unknown command, sudo), and checks (writes, paths outside the repo, network, shells per file, code it can't see into, unknown commands, sudo, credentials read). The verdict badge: unknown (anything unparsed, unknown, or code it can't inspect such as `python x.py` or `npm test`), else network, else writes, else read-only. Reading `.env`, `~/.ssh/…`, tokens or dumping the environment stays read-only but warns "Reads credentials". The command table and checks are ours; no library classifies commands. `tests/fixtures/shell/corpus.jsonl` (387 labelled commands from godspede/construct-auto-classifier, Apache-2.0) checks it never calls a dangerous command read-only.
- **Explained** (`agents/explain.rs`): the small AI tier says in two or three sentences what the command does. It never suggests what to trust, since the buttons can't apply a pattern. Cached per command; "Off" when the small tier is off.
- The Needs you tag: `low` (read-only), `writes`, `network`, `unknown`, `outside repo`, `question` (asks).

## Tree

Projects (the repo of a local group, the Scion project), groups, and rows. Local groups are made in the New session form ("New group…", with a goal); a session with no group is under **Unsorted**, where the small tier guesses a group from its directory, title and first prompt ("Looks like alethiometer › Agents tab · Move · Not this", `agents/guess.rs`). Nothing moves until you press Move. A folded group shows its loudest status and count. Status marks: running dot, waiting half ring, idle ring, done check, failed x.

## Facts box

Ticket, PR, branch and todo for a session (`agents/facts.rs`), refreshed when a session starts and after each turn: the branch from git in its directory (and how far ahead of upstream), the ticket as a Jira key in the branch (any case), title or first prompt (upper case only, so "utf-8" isn't one) and that issue's status from the synced Jira items, the PR as a synced GitHub PR for that branch or key, the todo as an open todo naming the key. A room shows its members' facts.

## Thread

Rows follow the row grammar (icon column coloured by kind, mono header, body). A session's own rows drop its name (`agents.names.self_in_transcript` brings it back). A run of messages between the same two agents folds to its last message ("N earlier · E"); the latest run stays open in one box (`agents.boxes.fold_pairs`). Thinking folds to one italic line; long tool output to three lines (`e`). The legend chips filter kinds. Scion sessions have a Messages tab (`t`/`m`): you ↔ the agent, your mentions of it in rooms, its asks.

## Keys

`⏎` goes in, `Esc` comes out, `Tab` cycles tree → thread → composer. `?` shows the sheet (06).

| Where | Keys |
|---|---|
| Everywhere | `⌘K` search, `/` filter sessions, `g i` / `g a`, `⌘.` interrupt, `?` keys |
| Tree | `j`/`k`, `h`/`l` fold or open, `⏎` open, `n` new agent (Scion group) or session (local) |
| Thread | `j`/`k`, `e` fold/open, `⏎` reply (`@name` in a room) or open a tool call, `c` copy, `g` latest, `t`/`m` tabs |
| Needs you | `o` `s` `a` `d`, `1`–`3` pick an answer, `r` write a reply, `Esc` later |
| Composer | `⌘⏎` send, `@` address one member (rooms), `↑` bring back your last message, `Esc` back |
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

## Not built

- "Also in Incoming": Needs you, unreachable and failed deliveries don't show in Incoming yet (no design for how they stream there).
- Renaming or deleting a local group from the UI (`agents.group_update` / `group_delete` exist), and moving a session other than through the Unsorted guess (`agents.move`).
- Elicitation (`elicitation/*`, Claude's AskUserQuestion): not advertised, so Claude asks in text instead.
- Native ACP subagent sessions (`clientCapabilities.subagents`): the legacy Task shape is used.
- The cache state is an estimate for Claude (about five minutes after the last turn); ACP doesn't report it.
