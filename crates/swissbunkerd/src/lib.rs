//! SwissBunker Forge daemon.
//!
//! Lives ON the disk, never installed on the host machine. Its job is to turn content the
//! user puts on that disk into an index the browser-side Reader can query from `file://`.

pub mod api;
pub mod import;
pub mod index;
pub mod journal;
pub mod manifest;
