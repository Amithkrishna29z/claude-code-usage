//! Pure, testable Claude Code usage logic with no UI dependency.
//!
//! Two sources feed the same [`UsageSnapshot`] shape:
//!
//! * [`oauth`] — Anthropic's own utilization endpoint, giving official session and
//!   weekly percentages.
//! * [`reader`] + [`calculator`] — a local estimate derived from Claude Code's
//!   session logs, used whenever the endpoint is unavailable.

pub mod calculator;
pub mod config;
pub mod models;
pub mod oauth;
pub mod parser;
pub mod reader;

pub use config::ConfigService;
pub use models::{
    AppConfig, OfficialUsage, UsageEvent, UsageSnapshot, UsageSource, UsageState, UsageWindow,
    WidgetStyle,
};
pub use oauth::OAuthUsageClient;
