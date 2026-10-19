//! OpenDisk core: finds what fills a disk and frees space safely.
//!
//! The pieces, in the order a cleaning goes through them:
//!
//! * [`scan`] walks a folder in parallel and builds a directory tree with
//!   allocated sizes (hardlinks counted once, symlinks never followed, never
//!   crossing into another volume).
//! * [`rules`] holds the declarative YAML rules that say what is safe to clean
//!   and why; [`findings`] expands them against a home folder.
//! * [`clean`] turns a selection into a [`clean::Plan`], previews it, executes
//!   it (Trash by default) and reports the real freed space from `statfs`.
//!
//! Nothing in this crate talks to the network.

pub mod clean;
pub mod fda;
pub mod findings;
pub mod format;
pub mod index;
pub mod journal;
pub mod paths;
pub mod rules;
pub mod scan;
pub mod size;
pub mod snapshots;
pub mod system;

pub use clean::{CleanOptions, Plan, Report};
pub use findings::{Finding, Item};
pub use rules::{Rule, RuleSet, Safety};
pub use scan::{DirNode, ScanOptions, ScanResult};
