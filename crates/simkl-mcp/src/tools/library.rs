//! The user's own data (`media:read`): `get_library`, `up_next`, `get_stats`.
//!
//! The library is read whole through [`UserClient::library`], which only
//! re-reads what `/sync/activities` says changed, and filtered here.
//!
//! [`UserClient::library`]: simkl_api::UserClient::library

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use garde::Validate;
use jiff::Timestamp;
use rmcp::handler::server::tool::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simkl_api::account::{GetSettings, GetStats, KindStats, StatusStats};
use simkl_api::library::{GetPlayback, Library, LibraryEntry, Status};
use simkl_api::media::Kind;

use crate::server::SimklServer;
use crate::tools::title::TitleCard;
use crate::tools::{Output, Page, ToolError, ToolResult, User, paginate, reply};

#[tool_router(router = library_router, vis = "pub")]
impl SimklServer {
    /// The user's library: what they watch, plan to watch, have finished or
    /// dropped, with progress and ratings.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<GetLibraryOutput>()
    )]
    async fn get_library(
        &self,
        User(user): User,
        Parameters(input): Parameters<GetLibraryInput>,
    ) -> ToolResult {
        input.validate()?;
        let library = user.library().await?;
        let items = select(&library, &input)
            .into_iter()
            .map(|(kind, entry)| LibraryItem::new(kind, entry))
            .collect::<Result<_, _>>()?;
        let (items, page) = paginate(items, input.page.unwrap_or(1), input.limit.unwrap_or(25));
        Ok(reply(GetLibraryOutput { items, page }))
    }

    /// What to watch next: the next episode of everything the user is
    /// watching, and paused movies and episodes. Most recent first.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<UpNextOutput>()
    )]
    async fn up_next(
        &self,
        User(user): User,
        Parameters(input): Parameters<UpNextInput>,
    ) -> ToolResult {
        input.validate()?;
        let library = user.library().await?;
        let playbacks = user.call(GetPlayback {}).await?;
        let wanted = |kind: Kind| input.kind.is_none_or(|wanted| wanted == kind);

        let mut items = Vec::new();
        for (kind, entry) in library.entries() {
            let Some(next) = &entry.next_to_watch_info else {
                continue;
            };
            if entry.status != Status::Watching || !wanted(kind) {
                continue;
            }
            items.push(UpNextItem {
                title: TitleCard::of_entry(kind, &entry.item, entry.anime_type.as_ref())?,
                next_episode: Some(NextEpisode {
                    season: next.season,
                    episode: next.episode,
                    title: next.title.clone(),
                    date: next.date,
                }),
                paused: None,
                last_activity: entry.last_watched_at,
            });
        }
        for playback in playbacks
            .into_iter()
            .filter(|playback| wanted(playback.kind))
        {
            let paused = Paused {
                progress: playback.progress,
                paused_at: playback.paused_at,
                season: playback.episode.as_ref().and_then(|episode| episode.season),
                episode: playback.episode.as_ref().map(|episode| episode.episode),
            };
            let simkl_id = playback.item.ids.simkl.map(u64::from);
            let existing = items.iter_mut().find(|item| {
                item.title.kind == playback.kind && Some(item.title.simkl_id) == simkl_id
            });
            match existing {
                Some(item) => {
                    item.last_activity = item.last_activity.max(Some(paused.paused_at));
                    item.paused = Some(paused);
                }
                None => items.push(UpNextItem {
                    title: TitleCard::of_entry(playback.kind, &playback.item, None)?,
                    next_episode: None,
                    last_activity: Some(paused.paused_at),
                    paused: Some(paused),
                }),
            }
        }
        items.sort_by_key(|item| Reverse(item.last_activity));
        items.truncate(input.limit.unwrap_or(20) as usize);
        Ok(reply(UpNextOutput { items }))
    }

    /// The user's watching stats: time spent and counts by status.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<StatsOutput>()
    )]
    async fn get_stats(&self, User(user): User) -> ToolResult {
        let settings = user.call(GetSettings {}).await?;
        let stats = user
            .call(GetStats {
                account_id: settings.account.id,
            })
            .await?;
        Ok(reply(StatsOutput {
            total_minutes: stats.total_mins,
            movies: KindStatsItem::from(stats.movies),
            shows: KindStatsItem::from(stats.tv),
            anime: KindStatsItem::from(stats.anime),
            last_week_minutes: stats.watched_last_week.map(|week| week.total_mins),
        }))
    }
}

/// The library entries `input` asks for, in its order.
fn select<'a>(library: &'a Library, input: &GetLibraryInput) -> Vec<(Kind, &'a LibraryEntry)> {
    let query = input.query.as_deref().map(str::to_lowercase);
    let mut entries: Vec<_> = library
        .entries()
        .filter(|(kind, entry)| {
            input.kind.is_none_or(|wanted| wanted == *kind)
                && input.status.is_none_or(|wanted| wanted == entry.status)
                && query
                    .as_deref()
                    .is_none_or(|query| entry.item.title.to_lowercase().contains(query))
        })
        .collect();
    // Newest and highest first; entries without a date or rating go last.
    match input.sort.unwrap_or_default() {
        LibrarySort::LastWatched => {
            entries.sort_by_key(|(_, entry)| Reverse(entry.last_watched_at))
        }
        LibrarySort::Added => {
            entries.sort_by_key(|(_, entry)| Reverse(entry.added_to_watchlist_at))
        }
        LibrarySort::Rating => entries.sort_by_key(|(_, entry)| Reverse(entry.user_rating)),
        LibrarySort::Title => {
            entries.sort_by_cached_key(|(_, entry)| entry.item.title.to_lowercase())
        }
    }
    entries
}

/// A title in the user's library.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryItem {
    title: TitleCard,
    status: Status,
    /// 1 to 10.
    user_rating: Option<u8>,
    watched_episodes: Option<u32>,
    total_episodes: Option<u32>,
    /// The next episode to watch, e.g. `S01E04`; absent when caught up.
    next_to_watch: Option<String>,
    #[schemars(with = "Option<String>")]
    last_watched_at: Option<Timestamp>,
    #[schemars(with = "Option<String>")]
    added_at: Option<Timestamp>,
}

impl LibraryItem {
    pub fn new(kind: Kind, entry: &LibraryEntry) -> Result<Self, ToolError> {
        Ok(Self {
            title: TitleCard::of_entry(kind, &entry.item, entry.anime_type.as_ref())?,
            status: entry.status,
            user_rating: entry.user_rating,
            watched_episodes: entry.watched_episodes_count,
            total_episodes: entry.total_episodes_count,
            next_to_watch: entry.next_to_watch.clone(),
            last_watched_at: entry.last_watched_at,
            added_at: entry.added_to_watchlist_at,
        })
    }

    /// Status, progress and rating: `watching, 5/10 episodes, next S01E06, rated 8/10`.
    pub fn progress(&self) -> String {
        let mut text = self.status.to_string();
        if let (Some(watched), Some(total)) = (self.watched_episodes, self.total_episodes) {
            write!(text, ", {watched}/{total} episodes").unwrap();
        }
        if let Some(next) = &self.next_to_watch {
            write!(text, ", next {next}").unwrap();
        }
        if let Some(rating) = self.user_rating {
            write!(text, ", rated {rating}/10").unwrap();
        }
        text
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
struct GetLibraryInput {
    kind: Option<Kind>,
    status: Option<Status>,
    /// Part of the title, ignoring case.
    query: Option<String>,
    /// Default `last_watched`.
    sort: Option<LibrarySort>,
    /// From 1. Default 1.
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    page: Option<u32>,
    /// 1 to 100. Default 25.
    #[garde(range(min = 1, max = 100))]
    #[schemars(range(min = 1, max = 100))]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum LibrarySort {
    /// Most recently watched first.
    #[default]
    LastWatched,
    /// Most recently added first.
    Added,
    /// Highest rated first.
    Rating,
    /// A to Z.
    Title,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetLibraryOutput {
    items: Vec<LibraryItem>,
    #[serde(flatten)]
    page: Page,
}

impl Output for GetLibraryOutput {
    fn summary(&self) -> String {
        let Page {
            page,
            page_count,
            total,
        } = self.page;
        if total == 0 {
            return "No titles in the library match.".to_owned();
        }
        let mut text = format!("{total} titles, page {page} of {page_count}:");
        for item in &self.items {
            write!(text, "\n- {} — {}", item.title.line(), item.progress()).unwrap();
        }
        text
    }
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
struct UpNextInput {
    kind: Option<Kind>,
    /// 1 to 100. Default 20.
    #[garde(range(min = 1, max = 100))]
    #[schemars(range(min = 1, max = 100))]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct UpNextOutput {
    items: Vec<UpNextItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct UpNextItem {
    title: TitleCard,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_episode: Option<NextEpisode>,
    /// Where the user paused, if they did.
    #[serde(skip_serializing_if = "Option::is_none")]
    paused: Option<Paused>,
    #[serde(skip)]
    last_activity: Option<Timestamp>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct NextEpisode {
    /// Absent for anime.
    season: Option<u32>,
    episode: Option<u32>,
    title: Option<String>,
    /// When it airs or aired.
    #[schemars(with = "Option<String>")]
    date: Option<Timestamp>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct Paused {
    /// Percent watched, 0 to 100.
    progress: f64,
    #[schemars(with = "String")]
    paused_at: Timestamp,
    season: Option<u32>,
    episode: Option<u32>,
}

impl Output for UpNextOutput {
    fn summary(&self) -> String {
        if self.items.is_empty() {
            return "Nothing to continue.".to_owned();
        }
        let mut text = "Up next:".to_owned();
        for item in &self.items {
            write!(text, "\n- {}", item.title.line()).unwrap();
            if let Some(next) = &item.next_episode {
                let number = match (next.season, next.episode) {
                    (Some(season), Some(episode)) => format!("S{season:02}E{episode:02}"),
                    (None, Some(episode)) => format!("E{episode}"),
                    (_, None) => "next episode".to_owned(),
                };
                write!(text, " — {number}").unwrap();
            }
            if let Some(paused) = &item.paused {
                write!(text, " (paused at {:.0}%)", paused.progress).unwrap();
            }
        }
        text
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct StatsOutput {
    total_minutes: u64,
    movies: KindStatsItem,
    shows: KindStatsItem,
    anime: KindStatsItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_week_minutes: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct KindStatsItem {
    total_minutes: u64,
    by_status: BTreeMap<Status, StatusStatsItem>,
}

/// Movies count minutes; shows and anime count episodes.
#[derive(Debug, Serialize, JsonSchema)]
struct StatusStatsItem {
    count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    minutes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    watched_episodes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_episodes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episodes_left: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minutes_left: Option<u64>,
}

impl From<KindStats> for KindStatsItem {
    fn from(stats: KindStats) -> Self {
        let by_status = stats
            .by_status
            .into_iter()
            .map(|(status, stats): (Status, StatusStats)| {
                let item = StatusStatsItem {
                    count: stats.count,
                    minutes: stats.mins,
                    watched_episodes: stats.watched_episodes_count,
                    total_episodes: stats.total_episodes_count,
                    episodes_left: stats.left_to_watch_episodes,
                    minutes_left: stats.left_to_watch_mins,
                };
                (status, item)
            })
            .collect();
        Self {
            total_minutes: stats.total_mins,
            by_status,
        }
    }
}

impl Output for StatsOutput {
    fn summary(&self) -> String {
        let hours = |minutes: u64| minutes / 60;
        let mut text = format!("{} hours watched in total.", hours(self.total_minutes));
        for (name, stats) in [
            ("Movies", &self.movies),
            ("Shows", &self.shows),
            ("Anime", &self.anime),
        ] {
            let counts: Vec<_> = stats
                .by_status
                .iter()
                .map(|(status, item)| format!("{} {status}", item.count))
                .collect();
            write!(
                text,
                "\n{name}: {} hours; {}",
                hours(stats.total_minutes),
                counts.join(", ")
            )
            .unwrap();
        }
        if let Some(minutes) = self.last_week_minutes {
            write!(text, "\nLast week: {} hours.", hours(minutes)).unwrap();
        }
        text
    }
}

#[cfg(test)]
#[path = "library_test.rs"]
mod tests;
