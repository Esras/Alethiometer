//! The application's view of its data: models and the ports (traits) that
//! load and save them (`app/docs/db-plan.md`, Phase 1). No SQL here and no
//! storage format; `crate::store` implements the ports on SQLite.
//!
//! - **Repositories** take and return models, inside a unit of work the edge
//!   opened (`ports::UnitOfWork`).
//! - **Read models** are what a screen lists: one query per screen, not an
//!   aggregate per row (`ports::Reads`).

pub mod daystrip;
pub mod incoming;
pub mod people;
pub mod ports;
pub mod sources;
pub mod todos;
