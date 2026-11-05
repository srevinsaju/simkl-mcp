//! The signed-in user's account and watching stats.

use std::collections::BTreeMap;

use garde::Validate;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_with::{NoneAsEmptyString, serde_as};

use crate::library::Status;

/// The signed-in user's profile and account.
#[derive(Debug, Clone, Serialize, Validate)]
pub struct GetSettings {}

endpoint!(GetSettings: Read => Settings, path = |self| "/users/settings");

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub user: Profile,
    pub account: Account,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub name: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub avatar: Option<String>,
    pub joined_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    /// The ID other user endpoints take, such as stats.
    pub id: u64,
    pub timezone: Option<String>,
    #[serde(rename = "type")]
    pub plan: Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Free,
    Pro,
    Vip,
}

/// Watching stats. Simkl computes these on every call, so fetch them only
/// when asked.
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct GetStats {
    #[serde(skip)]
    pub account_id: u64,
}

endpoint!(GetStats: Read => Stats, path = |self| format!("/users/{}/stats", self.account_id));

#[derive(Debug, Clone, Deserialize)]
pub struct Stats {
    pub total_mins: u64,
    pub movies: KindStats,
    pub tv: KindStats,
    pub anime: KindStats,
    pub watched_last_week: Option<WeekStats>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KindStats {
    pub total_mins: u64,
    #[serde(flatten)]
    pub by_status: BTreeMap<Status, StatusStats>,
}

/// Which counts are present depends on the kind: movies have minutes,
/// shows and anime have episodes.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusStats {
    pub count: u64,
    pub mins: Option<u64>,
    pub watched_episodes_count: Option<u64>,
    pub total_episodes_count: Option<u64>,
    pub left_to_watch_episodes: Option<u64>,
    pub left_to_watch_mins: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeekStats {
    pub total_mins: u64,
    pub movies_mins: u64,
    pub tv_mins: u64,
    pub anime_mins: u64,
}
