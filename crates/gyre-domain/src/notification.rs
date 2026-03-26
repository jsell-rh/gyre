//! Notification types — canonical definitions now live in `gyre-common` (HSI §2).
//!
//! Re-exported here so existing `use gyre_domain::Notification` import paths compile.

pub use gyre_common::notification::{Notification, NotificationType};
