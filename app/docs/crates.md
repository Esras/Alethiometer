# Crates and layering

Written 2026-10-08 on branch `split-core` (not merged). Why it was done, what exists, what is left.

## Why

The Mac `tauri dev` rebuild after a one-line edit was about 5.7 s; the main crate was one 64k-line lib. Measured on a 10-core Mac, same machine, one-line edit:

- `[profile.dev] debug = "line-tables-only"` (in `src-tauri/Cargo.toml`): 5.7 s → 3.7 s. Same as `debug = 0`. File:line in panics and backtraces stays; local variables in lldb don't. This was the whole win.
- Splitting out `core`: another ~0.5 s for edits in `queries`/`api`, nothing for edits in `core` itself (3.05 s → 2.5 s). Not measured since the later splits.
- Tried and dropped: `debug = false` for dependencies (no change for incremental rebuilds), `-Zthreads=8` via `RUSTC_BOOTSTRAP=1` (within noise, unsupported).

The split was kept for the architecture (dependency direction, tests apart from code), not for speed.

## The crates

Path dependencies like `agentd`, not a Cargo workspace (a workspace would move `target/` and the bundle path above). Each has its own `Cargo.lock` and `.cargo/config.toml` (the SQLite flag, see HANDOFF).

| Crate | Path | Holds |
|---|---|---|
| `alethiometer-core` | `app/core` | `time`, `clock`, `error`, `domain` (models and port traits, one per concept), `store` (the SQLite adapter), `migrations/` |
| `alethiometer-settings` | `app/settings` | the typed `Settings`, validation, per-connector config structs, the Settings page schema, the connector default secret names |
| `alethiometer-platform` | `app/platform` | `fsx` (atomic and checked file writes), `platform` (secrets file, opening URLs) |
| `alethiometer` | `app/src-tauri` | everything else: `queries`, `sync`, `ai`, `agents`, `glean`, `embed`, `outbox`, `live`, `api`, `state::App`, the Tauri shell |
| `alethiometer-agentd` | `app/agentd` | the agent host daemon (unchanged) |

The main crate re-exports `clock`, `domain`, `error`, `store`, `time`, `fsx`, `platform` and (via `settings/mod.rs`) the settings crate, so `crate::store::…` paths still work. Rewriting them to `alethiometer_core::…` is mechanical and not done. `settings/api.rs` and `settings/live.rs` (they need `App`) stay in the main crate.

## Tests

- A module's tests are a sibling `tests.rs` (`foo.rs` + `foo/tests.rs`, `mod.rs` + `tests.rs`), as `sync/connectors/slack/tests.rs` already was. Not inline `mod tests { … }`.
- Tests that need more than one layer are integration tests in `src-tauri/tests/`: `architecture.rs`, `maintained_values.rs` (needs the sample data from `seed`), `settings_match_connectors.rs`.
- Core's test helpers (`time::t`, `store::over`, `Db::legacy`, `schema::open_in_memory_at`, and `Clock::new` choosing UTC) are behind `#[cfg(any(test, feature = "test-support"))]`; `cfg(test)` doesn't cross crates. The main crate turns the feature on as a dev-dependency only. `Clock::new` picking UTC under that switch is old behaviour and still a smell.
- Run: `cargo test` in `app/core`, `app/settings`, `app/platform`; `cargo test --no-default-features` in `app/src-tauri`. 561 tests in all (core 19, settings 3, platform 7, main 532). `cargo fmt --check` in each.

## Architecture rules (`src-tauri/tests/architecture.rs`)

Text scans over production code (everything before a file's first `#[cfg(test)] mod`; code after one isn't scanned). The allowlists are exact: a new violation fails, and so does a fixed one nobody removed from the list.

- `no_sql_outside_store`: SQL only in `core/src/store/` (all four crates scanned).
- `nothing_else_reads_local_time`: only `Clock` reads the local time.
- `services_do_not_take_app`: no `App` in a function signature outside `api.rs`, `state.rs`, `lib.rs`, `main.rs`, `bin/`. About 270 signatures are allowlisted, over half in `agents`.
- `connectors_do_not_reach_into_services`: `sync/connectors/**` may not import `queries`, `state`, `agents`, `ai`. Allowlist: the `agents` and `scion` connectors (outbox adapters for the agents module), and `App` in `gcal/oauth`, `obsidian`, `slack/socket`.
- `queries_do_not_call_sync_ai_or_agents`: allowlist is empty.

When you fix one, lower its list; the failure message prints the new counts.

## Decisions made on the way

- The Google Calendar event fingerprint is copied into `store/utc_migration.rs`, frozen. A migration shouldn't change when the connector does.
- `Category` (with `JsonSchema`) lives in `core::domain::categories` with its `check`/`color`/`label` rules, so core depends on `schemars`.
- Connector default names (`SLACK_USER_TOKEN`, `JIRA_DEFAULT_JQL`, …) are defined in `settings::sources`; the connectors `pub use` them, reversing the old settings → sync direction.
- Moved to where their callers are: command-argument helpers to `api`; `PEOPLE_DIR` and the note-save error strings (`CONFLICT`, `NOT_OPEN`) to `core::domain`; note saving to `queries/note_save.rs`; `Notes`, `note_gist`, `clip` to `queries/note_text.rs` (`ai::context` re-exports them).

## What is left

The knot is `state::App`: it owns a field from nearly every module and is passed to services as a locator. In the hexagonal reading (models and ports in the core, adapters per source, services composed from the pieces they use), the targets are:

1. `agents` (108 `&App` functions, 3 narrow ones): give it store, clock, settings, platform and its own state. Needs a decision on where agent state lives.
2. `queries`, `ai`, `glean`, `embed`, `sync`, `outbox`, `live`: same, smaller. `sync/mod.rs` still calls `queries::people::rescan_vault` and `queries::incoming::move_to`; `glean` reaches `embed` through `app.embed`.
3. `App` ends up in the binary crate only, as the wiring.
4. Then `queries`, `sync`, `ai`, `agents` can become crates. Not before: cutting along today's call graph would freeze accidental seams as traits.

Not decided: connectors as true repositories behind a port. Today the local database is the system of record and sources are replicated peers (writes go through the outbox), per `db-plan.md`; the split keeps that.
