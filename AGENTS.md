# Working on Alethiometer

For agents and people. Read `HANDOFF.md` next: state of the work, user decisions, how to run things.

Priorities: **accuracy over performance, maintainability over speed of development.**

## Engineering rules

- **Use battle-tested libraries instead of hand-rolling them.** Time zones, parsing (shell, HTML, Markdown, ICS), crypto and signing, HTTP, retries, connection pools, protocols. If a well-maintained crate or package does it, use it. Write your own only when nothing fits, and say why in a comment. Examples already decided: `jiff` for time, `tree-sitter-bash` for shell commands.
- **Time** (`app/docs/db-plan.md`, Phase 0):
  - An instant is stored and handled in UTC. A calendar day is a local date.
  - Only the app's `Clock` turns one into the other. Nothing else reads the system time zone or local time.
- **Database** (`app/docs/db-plan.md`):
  - Every write that changes more than one row runs in one transaction, opened and committed by the edge: a command, a sync batch or an outbox send. It is never opened by business logic, and never committed by a repository.
  - Derived values are computed by the database (views, triggers), never written twice by the application.
  - The target is SQL only in `store/`, with repositories that take and return models. New code goes there, not into new SQL elsewhere.
- **Never hold the database across a network call or a file read.**
- **Tests:** a fix comes with a test that fails without it. Races get a test that runs the competing paths at once. Fixtures replace live accounts.
- **Keep each change minimal and say what it doesn't do.** Record decisions in `HANDOFF.md` and open problems in `ISSUES.md`.
