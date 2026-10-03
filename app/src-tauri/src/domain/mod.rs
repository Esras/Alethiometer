//! The application's view of its data: models and the ports (traits) that
//! load and save them (`app/docs/db-plan.md`, Phase 1). No SQL here and no
//! storage format; `crate::store` implements the ports on SQLite.
//!
//! - **Repositories** (`ports::Events`, …) take and return models, inside a
//!   unit of work the edge opened.
//! - **Read models** (`views`) are what a screen lists: one query per screen,
//!   not an aggregate per row.

pub mod ports;
pub mod views;
