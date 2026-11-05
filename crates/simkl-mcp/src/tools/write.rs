//! Writes (`media:write`): `set_status`, `mark_watched`, `unmark_watched`,
//! `rate`, `unrate`.
//!
//! Every write names its titles by ID; a name alone is searched first, so
//! nothing is guessed. Each tool takes a batch, sent as one request, since
//! Simkl allows one write per second. Simkl answers success even for items it
//! couldn't match, so each reply lists those; a write that matched nothing is
//! an error.

use std::fmt::Write;

use garde::Validate;
use jiff::Timestamp;
use rmcp::handler::server::tool::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simkl_api::library::{
    EpisodeNumber, HistoryItem, MarkWatched, Rate, RatingItem, SetStatus, Status, StatusChange,
    Target, TitleRef, UnmarkWatched, Unrate, WriteOutcome,
};
use simkl_api::media::{Ids, Kind};

use crate::server::SimklServer;
use crate::tools::title::TitleInput;
use crate::tools::{Output, ToolError, ToolResult, User, reply};

#[tool_router(router = write_router, vis = "pub")]
impl SimklServer {
    /// Move titles to a status in the user's library, adding them if needed.
    /// Movies can only be `plantowatch`, `completed` or `dropped`.
    #[tool(
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<WriteOutput>()
    )]
    async fn set_status(
        &self,
        User(user): User,
        Parameters(input): Parameters<SetStatusInput>,
    ) -> ToolResult {
        input.validate()?;
        let items = input
            .items
            .into_iter()
            .map(|item| {
                Ok(StatusChange {
                    title: item.title.into_ref()?,
                    status: item.status,
                })
            })
            .collect::<Result<Vec<_>, ToolError>>()?;
        let titles = items.iter().map(|item| item.title.clone()).collect();
        let written = user.call(SetStatus(items)).await?;
        Ok(reply(outcome(Action::SetStatus, titles, written)?))
    }

    /// Record titles, seasons or episodes as watched. A whole show marks every
    /// aired episode.
    #[tool(
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<WriteOutput>()
    )]
    async fn mark_watched(
        &self,
        User(user): User,
        Parameters(input): Parameters<MarkWatchedInput>,
    ) -> ToolResult {
        input.validate()?;
        let items = input
            .items
            .into_iter()
            .map(|item| {
                Ok(HistoryItem::builder()
                    .title(item.title.into_ref()?)
                    .target(target(item.seasons, item.episodes)?)
                    .maybe_watched_at(item.watched_at)
                    .build())
            })
            .collect::<Result<Vec<_>, ToolError>>()?;
        let titles = items.iter().map(|item| item.title.clone()).collect();
        let written = user.call(MarkWatched(items)).await?;
        Ok(reply(outcome(Action::MarkWatched, titles, written)?))
    }

    /// Remove watches from the user's history. Removing a whole title also
    /// removes it from the library, rating included.
    #[tool(
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<WriteOutput>()
    )]
    async fn unmark_watched(
        &self,
        User(user): User,
        Parameters(input): Parameters<UnmarkWatchedInput>,
    ) -> ToolResult {
        input.validate()?;
        let items = input
            .items
            .into_iter()
            .map(|item| {
                Ok(HistoryItem::builder()
                    .title(item.title.into_ref()?)
                    .target(target(item.seasons, item.episodes)?)
                    .build())
            })
            .collect::<Result<Vec<_>, ToolError>>()?;
        let titles = items.iter().map(|item| item.title.clone()).collect();
        let written = user.call(UnmarkWatched(items)).await?;
        Ok(reply(outcome(Action::UnmarkWatched, titles, written)?))
    }

    /// Rate titles from 1 to 10. Rating a title not in the library adds it.
    #[tool(
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<WriteOutput>()
    )]
    async fn rate(&self, User(user): User, Parameters(input): Parameters<RateInput>) -> ToolResult {
        input.validate()?;
        let items = input
            .items
            .into_iter()
            .map(|item| {
                Ok(RatingItem {
                    title: item.title.into_ref()?,
                    score: item.score,
                })
            })
            .collect::<Result<Vec<_>, ToolError>>()?;
        let titles = items.iter().map(|item| item.title.clone()).collect();
        let written = user.call(Rate(items)).await?;
        Ok(reply(outcome(Action::Rate, titles, written)?))
    }

    /// Clear the user's ratings of titles.
    #[tool(
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<WriteOutput>()
    )]
    async fn unrate(
        &self,
        User(user): User,
        Parameters(input): Parameters<UnrateInput>,
    ) -> ToolResult {
        input.validate()?;
        let titles = input
            .titles
            .into_iter()
            .map(TitleInput::into_ref)
            .collect::<Result<Vec<_>, _>>()?;
        let written = user.call(Unrate(titles.clone())).await?;
        Ok(reply(outcome(Action::Unrate, titles, written)?))
    }
}

/// Which part of a title a history item covers: the whole title, some
/// seasons, or some episodes, never both.
fn target(
    seasons: Option<Vec<u32>>,
    episodes: Option<Vec<EpisodeInput>>,
) -> Result<Target, ToolError> {
    match (seasons, episodes) {
        (None, None) => Ok(Target::Title),
        (Some(seasons), None) if !seasons.is_empty() => Ok(Target::Seasons(seasons)),
        (None, Some(episodes)) if !episodes.is_empty() => Ok(Target::Episodes(
            episodes
                .into_iter()
                .map(|EpisodeInput { season, episode }| EpisodeNumber { season, episode })
                .collect(),
        )),
        (Some(_), Some(_)) => Err(ToolError::Input(
            "give `seasons` or `episodes`, not both".into(),
        )),
        _ => Err(ToolError::Input(
            "`seasons` and `episodes` need at least one entry; leave both out for the whole title"
                .into(),
        )),
    }
}

/// Splits a write's titles into those Simkl applied and those it echoed back
/// as `not_found`. Simkl also reports unmatched episodes of a matched title;
/// those are listed without un-applying the title. Nothing applied is an
/// error, so the model notices.
fn outcome(
    action: Action,
    titles: Vec<TitleRef>,
    written: WriteOutcome,
) -> Result<WriteOutput, ToolError> {
    let applied = titles.into_iter().filter(|title| {
        !written
            .not_found
            .iter()
            .any(|not_found| not_found.ids == title.ids)
    });
    let output = WriteOutput {
        action,
        applied: applied
            .map(|title| WrittenTitle {
                kind: title.kind,
                url: title.ids.url(title.kind),
                ids: TitleIds::from(&title.ids),
            })
            .collect(),
        not_found: written
            .not_found
            .iter()
            .map(|not_found| Unmatched {
                kind: not_found.kind,
                title: not_found.title.clone(),
                ids: TitleIds::from(&not_found.ids),
            })
            .collect(),
    };
    match output.applied.is_empty() {
        true => Err(ToolError::NothingApplied {
            summary: output.summary(),
        }),
        false => Ok(output),
    }
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
struct SetStatusInput {
    /// 1 to 50 titles.
    #[garde(length(min = 1, max = 50))]
    #[schemars(length(min = 1, max = 50))]
    items: Vec<StatusItem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StatusItem {
    title: TitleInput,
    status: Status,
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
struct MarkWatchedInput {
    /// 1 to 50 titles.
    #[garde(length(min = 1, max = 50))]
    #[schemars(length(min = 1, max = 50))]
    items: Vec<WatchItem>,
}

/// Leave out `seasons` and `episodes` for the whole title.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WatchItem {
    title: TitleInput,
    /// Every episode of these seasons.
    seasons: Option<Vec<u32>>,
    /// These episodes. Anime episodes are numbered within season 1.
    episodes: Option<Vec<EpisodeInput>>,
    /// When it was watched, RFC 3339. Default now.
    #[schemars(with = "Option<String>")]
    watched_at: Option<Timestamp>,
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
struct UnmarkWatchedInput {
    /// 1 to 50 titles.
    #[garde(length(min = 1, max = 50))]
    #[schemars(length(min = 1, max = 50))]
    items: Vec<UnwatchItem>,
}

/// Leave out `seasons` and `episodes` to remove the whole title.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UnwatchItem {
    title: TitleInput,
    seasons: Option<Vec<u32>>,
    /// Anime episodes are numbered within season 1.
    episodes: Option<Vec<EpisodeInput>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EpisodeInput {
    season: u32,
    episode: u32,
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
struct RateInput {
    /// 1 to 50 titles.
    #[garde(length(min = 1, max = 50))]
    #[schemars(length(min = 1, max = 50))]
    items: Vec<RateItem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RateItem {
    title: TitleInput,
    /// 1 to 10.
    #[schemars(range(min = 1, max = 10))]
    score: u8,
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
struct UnrateInput {
    /// 1 to 50 titles.
    #[garde(length(min = 1, max = 50))]
    #[schemars(length(min = 1, max = 50))]
    titles: Vec<TitleInput>,
}

/// What a write did, for the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    SetStatus,
    MarkWatched,
    UnmarkWatched,
    Rate,
    Unrate,
}

#[derive(Debug, Serialize, JsonSchema)]
struct WriteOutput {
    #[serde(skip)]
    action: Action,
    /// The titles Simkl applied the write to.
    applied: Vec<WrittenTitle>,
    /// Items Simkl couldn't match, as it echoed them: titles, or episodes of
    /// an applied title.
    not_found: Vec<Unmatched>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct WrittenTitle {
    kind: Kind,
    #[serde(flatten)]
    ids: TitleIds,
    /// The title's simkl.com page, when its Simkl ID was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct Unmatched {
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(flatten)]
    ids: TitleIds,
}

/// A title's IDs, as a tool input names them.
#[derive(Debug, Default, Serialize, JsonSchema)]
struct TitleIds {
    #[serde(skip_serializing_if = "Option::is_none")]
    simkl_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    imdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tmdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tvdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anidb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anilist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kitsu: Option<String>,
}

impl From<&Ids> for TitleIds {
    fn from(ids: &Ids) -> Self {
        let external = |id: &Option<_>| id.as_ref().map(ToString::to_string);
        Self {
            simkl_id: ids.simkl.map(u64::from),
            imdb: external(&ids.imdb),
            tmdb: external(&ids.tmdb),
            tvdb: external(&ids.tvdb),
            mal: external(&ids.mal),
            anidb: external(&ids.anidb),
            anilist: external(&ids.anilist),
            kitsu: external(&ids.kitsu),
        }
    }
}

impl TitleIds {
    /// `simkl 17465`, or the first external ID: `imdb tt0903747`.
    fn label(&self) -> String {
        [
            ("simkl", self.simkl_id.map(|id| id.to_string())),
            ("imdb", self.imdb.clone()),
            ("tmdb", self.tmdb.clone()),
            ("tvdb", self.tvdb.clone()),
            ("mal", self.mal.clone()),
            ("anidb", self.anidb.clone()),
            ("anilist", self.anilist.clone()),
            ("kitsu", self.kitsu.clone()),
        ]
        .into_iter()
        .find_map(|(name, id)| Some(format!("{name} {}", id?)))
        .unwrap_or_else(|| "no IDs".to_owned())
    }
}

impl Output for WriteOutput {
    fn summary(&self) -> String {
        let verb = match self.action {
            Action::SetStatus => "Set the status of",
            Action::MarkWatched => "Marked watched",
            Action::UnmarkWatched => "Removed from history",
            Action::Rate => "Rated",
            Action::Unrate => "Cleared the rating of",
        };
        let mut text = format!("{verb} {} titles.", self.applied.len());
        for title in &self.applied {
            let link = title.url.as_deref().unwrap_or_default();
            write!(text, "\n- {} {} {link}", title.kind, title.ids.label()).unwrap();
        }
        if !self.not_found.is_empty() {
            text.push_str("\nSimkl couldn't match:");
            for item in &self.not_found {
                let name = item.title.as_deref().unwrap_or_default();
                write!(text, "\n- {name} {}", item.ids.label()).unwrap();
            }
        }
        text
    }
}

#[cfg(test)]
#[path = "write_test.rs"]
mod tests;
