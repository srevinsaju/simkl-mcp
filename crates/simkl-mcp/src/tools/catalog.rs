//! Catalog tools (`media:read`): `search`, `get_title`, `get_episodes`,
//! `trending`, `calendar`.

use std::fmt::Write;

use garde::Validate;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use rmcp::handler::server::tool::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simkl_api::data::{Calendar, Month, Period, Trending, TrendingGenres, TrendingInGenre};
use simkl_api::library::Status;
use simkl_api::media::{EpisodeType, GetEpisodes, GetTitle, Kind, Search};

use crate::server::SimklServer;
use crate::tools::library::LibraryItem;
use crate::tools::title::{TitleCard, TitleInput};
use crate::tools::{Output, Page, ToolError, ToolResult, User, paginate, reply};

const ALL_KINDS: [Kind; 3] = [Kind::Movie, Kind::Show, Kind::Anime];

/// Simkl requires this wherever trending data is shown.
const TRENDING_ATTRIBUTION: &str = "Trending on Simkl";

const EPISODES_PER_PAGE: u32 = 100;

#[tool_router(router = catalog_router, vis = "pub")]
impl SimklServer {
    /// Search Simkl for movies, TV shows or anime by name. Without `kind`,
    /// searches all three.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<SearchOutput>()
    )]
    async fn search(
        &self,
        User(user): User,
        Parameters(input): Parameters<SearchInput>,
    ) -> ToolResult {
        let kinds = input.kind.map_or(ALL_KINDS.to_vec(), |kind| vec![kind]);
        let mut results = Vec::new();
        for kind in kinds {
            let search = Search::builder()
                .kind(kind)
                .q(input.query.clone())
                .maybe_limit(input.limit)
                .build();
            for result in user.call(search).await?.items {
                results.push(TitleCard::of_search(&result)?);
            }
        }
        Ok(reply(SearchOutput {
            query: input.query,
            results,
        }))
    }

    /// A title's details, related titles, and where it sits in the user's
    /// library.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<GetTitleOutput>()
    )]
    async fn get_title(
        &self,
        User(user): User,
        Parameters(input): Parameters<GetTitleInput>,
    ) -> ToolResult {
        let (kind, id) = input.title.resolve(&user).await?;
        let title = user
            .call(GetTitle { kind, id })
            .await?
            .ok_or(ToolError::NotFound { kind })?;
        let my_entry = match user.library().await?.get(title.kind, id) {
            Some(entry) => Some(LibraryItem::new(title.kind, entry)?),
            None => None,
        };
        let related = title
            .similar
            .iter()
            .chain(&title.users_recommendations)
            .take(10)
            .map(TitleCard::of_related)
            .collect::<Result<_, _>>()?;
        Ok(reply(GetTitleOutput {
            title: TitleDetails {
                card: TitleCard::of_title(&title)?,
                overview: title.overview,
                genres: title.genres,
                runtime: title.runtime,
                certification: title.certification,
                country: title.country,
                network: title.network,
                status: title.status.map(|status| status.to_string()),
                released: title.released,
                first_aired: title.first_aired,
                last_aired: title.last_aired,
                total_episodes: title.total_episodes,
                rating: title.ratings.get("simkl").and_then(|r| r.rating),
            },
            related,
            my_entry,
        }))
    }

    /// Episodes of a show or anime, 100 per page, specials last.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<GetEpisodesOutput>()
    )]
    async fn get_episodes(
        &self,
        User(user): User,
        Parameters(input): Parameters<GetEpisodesInput>,
    ) -> ToolResult {
        input.validate()?;
        let (kind, id) = input.title.resolve(&user).await?;
        let (title, episodes) = tokio::join!(
            user.call(GetTitle { kind, id }),
            user.call(GetEpisodes { kind, id }),
        );
        let title = title?.ok_or(ToolError::NotFound { kind })?;
        let episodes = episodes?
            .into_iter()
            .filter(|episode| {
                input
                    .season
                    .is_none_or(|season| episode.season == Some(season))
            })
            .map(|episode| EpisodeItem {
                season: episode.season,
                episode: episode.episode,
                title: episode.title,
                special: episode.episode_type == EpisodeType::Special,
                aired: episode.aired,
                date: episode.date,
            })
            .collect();
        let (episodes, page) = paginate(episodes, input.page.unwrap_or(1), EPISODES_PER_PAGE);
        Ok(reply(GetEpisodesOutput {
            title: TitleCard::of_title(&title)?,
            episodes,
            page,
        }))
    }

    /// The most watched titles on Simkl right now, optionally in one genre.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<TrendingOutput>()
    )]
    async fn trending(
        &self,
        User(user): User,
        Parameters(input): Parameters<TrendingInput>,
    ) -> ToolResult {
        input.validate()?;
        let kind = input.kind;
        let period = input.period.unwrap_or(match input.genre {
            None => Period::Week,
            Some(_) => Period::Month,
        });
        let titles = match &input.genre {
            None => {
                user.call(Trending::builder().kind(kind).period(period).build())
                    .await?
            }
            Some(slug) => {
                if period != Period::Month {
                    return Err(ToolError::Input(
                        "trending by genre is only by `month`".into(),
                    ));
                }
                let genres = user.call(TrendingGenres { kind }).await?;
                if !genres.iter().any(|genre| genre.slug == *slug) {
                    let slugs: Vec<_> = genres.iter().map(|genre| genre.slug.as_str()).collect();
                    return Err(ToolError::Input(format!(
                        "no genre `{slug}`; use one of: {}",
                        slugs.join(", ")
                    )));
                }
                let slug = slug.clone();
                user.call(TrendingInGenre { kind, slug }).await?
            }
        };
        let items = titles
            .iter()
            .take(input.limit.unwrap_or(20) as usize)
            .zip(1..)
            .map(|(title, rank)| {
                Ok(TrendingItem {
                    rank,
                    title: TitleCard::of_data(kind, title)?,
                })
            })
            .collect::<Result<_, ToolError>>()?;
        Ok(reply(TrendingOutput {
            attribution: TRENDING_ATTRIBUTION.to_owned(),
            kind,
            period: <&str>::from(period).to_owned(),
            genre: input.genre,
            items,
        }))
    }

    /// What airs or releases in a date range: by default, titles in the
    /// user's library over the next 7 days.
    #[tool(
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = schema_for_output::<CalendarOutput>()
    )]
    async fn calendar(
        &self,
        User(user): User,
        Parameters(input): Parameters<CalendarInput>,
    ) -> ToolResult {
        input.validate()?;
        let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
        let from = input.from.unwrap_or(today);
        let until = from.saturating_add(i64::from(input.days.unwrap_or(7)).days());
        let library = match input.library_only.unwrap_or(true) {
            true => Some(user.library().await?),
            false => None,
        };

        let mut items = Vec::new();
        for kind in input.kind.map_or(ALL_KINDS.to_vec(), |kind| vec![kind]) {
            for month in calendar_files(today, from, until) {
                for entry in user.call(Calendar { kind, month }).await? {
                    let date = entry.date.to_zoned(TimeZone::UTC).date();
                    if date < from || date >= until {
                        continue;
                    }
                    let mine = library
                        .as_ref()
                        .map(|library| library.get(kind, entry.id))
                        .map(|entry| entry.filter(|entry| entry.status != Status::Dropped));
                    if mine.is_some_and(|entry| entry.is_none()) {
                        continue;
                    }
                    // Calendar files may lack a title's metadata; the library
                    // entry names it then.
                    let title = match (&entry.title, mine.flatten()) {
                        (Some(title), _) => TitleCard::of_data(kind, title)?,
                        (None, Some(mine)) => {
                            TitleCard::of_entry(kind, &mine.item, mine.anime_type.as_ref())?
                        }
                        (None, None) => continue,
                    };
                    let episode = entry.episode.as_ref();
                    items.push(CalendarItem {
                        date: entry.date,
                        title,
                        season: episode.and_then(|episode| episode.season),
                        episode: episode.and_then(|episode| episode.episode),
                        episode_title: episode.and_then(|episode| episode.title.clone()),
                    });
                }
            }
        }
        items.sort_by_key(|item| item.date);
        let total = items.len() as u32;
        items.truncate(input.limit.unwrap_or(50) as usize);
        Ok(reply(CalendarOutput { items, total }))
    }
}

/// The calendar files covering `[from, until)`: Simkl's rolling file
/// (`None`), which runs from yesterday to a month ahead, when it covers the
/// range, or else every month the range touches.
fn calendar_files(today: Date, from: Date, until: Date) -> Vec<Option<Month>> {
    let last = until.yesterday().unwrap();
    if from >= today.yesterday().unwrap() && last <= today.saturating_add(1.month()) {
        return vec![None];
    }
    let mut month = from.first_of_month();
    let mut months = Vec::new();
    while month <= last {
        months.push(Some(Month {
            year: month.year(),
            month: month.month() as u8,
        }));
        month = month.saturating_add(1.month());
    }
    months
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchInput {
    query: String,
    kind: Option<Kind>,
    /// Results per kind, 1 to 50. Default 10.
    #[schemars(range(min = 1, max = 50))]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct SearchOutput {
    query: String,
    results: Vec<TitleCard>,
}

impl Output for SearchOutput {
    fn summary(&self) -> String {
        if self.results.is_empty() {
            return format!("No titles found for {:?}.", self.query);
        }
        numbered(
            format!("Search results for {:?}:", self.query),
            self.results.iter().map(TitleCard::line),
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GetTitleInput {
    title: TitleInput,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetTitleOutput {
    title: TitleDetails,
    /// Similar titles, and what viewers of this title also watched.
    related: Vec<TitleCard>,
    /// The title in the user's library; absent if it isn't there.
    #[serde(skip_serializing_if = "Option::is_none")]
    my_entry: Option<LibraryItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct TitleDetails {
    #[serde(flatten)]
    card: TitleCard,
    overview: Option<String>,
    genres: Vec<String>,
    /// Minutes; for shows, a typical episode.
    runtime: Option<u32>,
    certification: Option<String>,
    country: Option<String>,
    network: Option<String>,
    /// `airing`, `ended` or `tba`.
    status: Option<String>,
    #[schemars(with = "Option<String>")]
    released: Option<Date>,
    first_aired: Option<String>,
    last_aired: Option<String>,
    total_episodes: Option<u32>,
    /// Simkl's rating, out of 10.
    rating: Option<f64>,
}

impl Output for GetTitleOutput {
    fn summary(&self) -> String {
        let mut text = self.title.card.line();
        if let Some(overview) = &self.title.overview {
            write!(text, "\n{overview}").unwrap();
        }
        match &self.my_entry {
            Some(entry) => write!(text, "\nIn your library: {}", entry.progress()).unwrap(),
            None => text.push_str("\nNot in your library."),
        }
        if !self.related.is_empty() {
            text.push_str("\n\n");
            text.push_str(&numbered(
                "Related:".into(),
                self.related.iter().map(TitleCard::line),
            ));
        }
        text
    }
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
struct GetEpisodesInput {
    title: TitleInput,
    /// Only this season. Anime episodes usually have no season; leave it out
    /// for anime.
    season: Option<u32>,
    /// From 1. Default 1.
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    page: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEpisodesOutput {
    title: TitleCard,
    episodes: Vec<EpisodeItem>,
    #[serde(flatten)]
    page: Page,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EpisodeItem {
    season: Option<u32>,
    episode: Option<u32>,
    title: Option<String>,
    special: bool,
    aired: bool,
    #[schemars(with = "Option<String>")]
    date: Option<Timestamp>,
}

impl Output for GetEpisodesOutput {
    fn summary(&self) -> String {
        let Page {
            page,
            page_count,
            total,
        } = self.page;
        let mut text = format!(
            "{}\n{total} episodes, page {page} of {page_count}:",
            self.title.line()
        );
        for episode in &self.episodes {
            let number = match (episode.season, episode.episode) {
                (Some(season), Some(number)) => format!("S{season:02}E{number:02}"),
                (None, Some(number)) => format!("E{number}"),
                (_, None) => "Special".to_owned(),
            };
            let title = episode.title.as_deref().unwrap_or("untitled");
            let aired = if episode.aired { "" } else { " (not aired)" };
            write!(text, "\n{number} {title}{aired}").unwrap();
        }
        text
    }
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
struct TrendingInput {
    kind: Kind,
    /// Default `week`, or `month` with a genre, which only supports `month`.
    period: Option<Period>,
    /// A genre slug such as `action`. An unknown slug is answered with the
    /// valid ones.
    genre: Option<String>,
    /// 1 to 100. Default 20.
    #[garde(range(min = 1, max = 100))]
    #[schemars(range(min = 1, max = 100))]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct TrendingOutput {
    /// Show this wherever the list is shown.
    attribution: String,
    kind: Kind,
    period: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    genre: Option<String>,
    items: Vec<TrendingItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct TrendingItem {
    rank: u32,
    #[serde(flatten)]
    title: TitleCard,
}

impl Output for TrendingOutput {
    fn summary(&self) -> String {
        let genre = self
            .genre
            .as_ref()
            .map(|genre| format!(" in {genre}"))
            .unwrap_or_default();
        numbered(
            format!(
                "{} ({}{genre}, this {}):",
                self.attribution, self.kind, self.period
            ),
            self.items.iter().map(|item| item.title.line()),
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
struct CalendarInput {
    /// All kinds if absent.
    kind: Option<Kind>,
    /// First day, `YYYY-MM-DD` in UTC. Default today.
    #[schemars(with = "Option<String>")]
    from: Option<Date>,
    /// Days from `from`, 1 to 31. Default 7.
    #[garde(range(min = 1, max = 31))]
    #[schemars(range(min = 1, max = 31))]
    days: Option<u32>,
    /// Only titles in the user's library, other than dropped ones. Default
    /// true.
    library_only: Option<bool>,
    /// 1 to 200. Default 50.
    #[garde(range(min = 1, max = 200))]
    #[schemars(range(min = 1, max = 200))]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CalendarOutput {
    /// Soonest first.
    items: Vec<CalendarItem>,
    /// Airings in the range, before `limit`.
    total: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CalendarItem {
    #[schemars(with = "String")]
    date: Timestamp,
    title: TitleCard,
    season: Option<u32>,
    episode: Option<u32>,
    episode_title: Option<String>,
}

impl Output for CalendarOutput {
    fn summary(&self) -> String {
        if self.items.is_empty() {
            return "Nothing airs in this range.".to_owned();
        }
        let mut text = format!("{} airings:", self.total);
        for item in &self.items {
            let date = item.date.to_zoned(TimeZone::UTC).date();
            let episode = match (item.season, item.episode) {
                (Some(season), Some(number)) => format!(" S{season:02}E{number:02}"),
                (None, Some(number)) => format!(" E{number}"),
                (_, None) => String::new(),
            };
            write!(text, "\n{date}{episode} {}", item.title.line()).unwrap();
        }
        text
    }
}

/// `heading` followed by `lines`, numbered from 1.
fn numbered(heading: String, lines: impl Iterator<Item = String>) -> String {
    let mut text = heading;
    for (line, number) in lines.zip(1..) {
        write!(text, "\n{number}. {line}").unwrap();
    }
    text
}

#[cfg(test)]
#[path = "catalog_test.rs"]
mod tests;
