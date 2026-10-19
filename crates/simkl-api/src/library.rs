//! The user's library: watchlist statuses, entries with progress and
//! `next_to_watch`, paused playbacks, and the writes that change them. Read
//! the library itself with [`UserClient::library`](crate::UserClient::library).
//!
//! Movies cannot be `watching` or `hold`, and have no seasons; writes are
//! validated rather than relying on Simkl's silent rewrites
//! ([statuses](https://api.simkl.org/conventions/list-statuses)).
//!
//! Anime is always written under `shows`: `/sync/history/remove` ignores the
//! `anime` key, and every write endpoint accepts anime in `shows`.

use garde::Validate;
use indexmap::IndexMap;
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use strum::Display;

use crate::client::Reply;
use crate::error::Result;
use crate::media::{AnimeType, Ids, Kind, SimklId, Year};

/// Where a title sits in the user's library.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    Display,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Status {
    Watching,
    #[serde(rename = "plantowatch")]
    #[strum(serialize = "plantowatch")]
    PlanToWatch,
    Hold,
    Completed,
    /// Older client IDs read this as `notinteresting`.
    #[serde(alias = "notinteresting")]
    Dropped,
}

impl Status {
    /// Movies can only be planned, completed or dropped.
    pub fn applies_to(self, kind: Kind) -> bool {
        kind.is_episodic() || !matches!(self, Status::Watching | Status::Hold)
    }
}

/// The user's library, by Simkl ID.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "LibraryLists")]
pub struct Library {
    pub shows: IndexMap<SimklId, LibraryEntry>,
    pub movies: IndexMap<SimklId, LibraryEntry>,
    pub anime: IndexMap<SimklId, LibraryEntry>,
}

/// The library as Simkl sends it. An empty library is `{}`
/// ([null values](https://api.simkl.org/conventions/null-values)).
#[derive(Default, Deserialize)]
#[serde(default)]
struct LibraryLists {
    shows: Vec<LibraryEntry>,
    movies: Vec<LibraryEntry>,
    anime: Vec<LibraryEntry>,
}

impl TryFrom<LibraryLists> for Library {
    type Error = String;

    fn try_from(
        LibraryLists {
            shows,
            movies,
            anime,
        }: LibraryLists,
    ) -> Result<Self, String> {
        let by_id = |entries: Vec<LibraryEntry>| {
            entries
                .into_iter()
                .map(|entry| match entry.item.ids.simkl {
                    Some(id) => Ok((id, entry)),
                    None => Err(format!(
                        "library entry without a Simkl ID: {:?}",
                        entry.item.title
                    )),
                })
                .collect::<Result<_, _>>()
        };
        Ok(Self {
            shows: by_id(shows)?,
            movies: by_id(movies)?,
            anime: by_id(anime)?,
        })
    }
}

impl Library {
    pub fn get(&self, kind: Kind, id: SimklId) -> Option<&LibraryEntry> {
        self.of(kind).get(&id)
    }

    pub fn entries(&self) -> impl Iterator<Item = (Kind, &LibraryEntry)> {
        [Kind::Show, Kind::Movie, Kind::Anime]
            .into_iter()
            .flat_map(|kind| self.of(kind).values().map(move |entry| (kind, entry)))
    }

    pub fn len(&self) -> usize {
        self.shows.len() + self.movies.len() + self.anime.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn of(&self, kind: Kind) -> &IndexMap<SimklId, LibraryEntry> {
        match kind {
            Kind::Movie => &self.movies,
            Kind::Show => &self.shows,
            Kind::Anime => &self.anime,
        }
    }
}

/// One title in the library.
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryEntry {
    /// The title, under `show` (shows and anime) or `movie` in the JSON.
    #[serde(rename = "show", alias = "movie")]
    pub item: EntryTitle,
    pub status: Status,
    pub user_rating: Option<u8>,
    pub added_to_watchlist_at: Option<Timestamp>,
    pub last_watched_at: Option<Timestamp>,
    pub user_rated_at: Option<Timestamp>,
    /// Last episode watched, e.g. `S05E16`.
    pub last_watched: Option<String>,
    /// Next episode to watch, e.g. `S08E03`; `None` when caught up.
    pub next_to_watch: Option<String>,
    pub next_to_watch_info: Option<NextEpisode>,
    pub watched_episodes_count: Option<u32>,
    /// Includes unaired episodes, and can lag behind new episodes.
    pub total_episodes_count: Option<u32>,
    pub not_aired_episodes_count: Option<u32>,
    pub anime_type: Option<AnimeType>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntryTitle {
    pub title: String,
    pub year: Option<Year>,
    pub poster: Option<String>,
    pub runtime: Option<u32>,
    pub ids: Ids,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NextEpisode {
    pub title: Option<String>,
    /// Absent for anime.
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub date: Option<Timestamp>,
}

/// A paused playback, for resuming.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "WirePlayback")]
pub struct Playback {
    pub id: u64,
    /// Percent watched, 0 to 100.
    pub progress: f64,
    pub paused_at: Timestamp,
    pub kind: Kind,
    /// The movie, or the show the episode belongs to.
    pub item: EntryTitle,
    pub episode: Option<PlaybackEpisode>,
}

/// A playback as Simkl sends it: the title under the key naming its kind.
#[derive(Deserialize)]
struct WirePlayback {
    id: u64,
    progress: f64,
    paused_at: Timestamp,
    show: Option<EntryTitle>,
    movie: Option<EntryTitle>,
    anime: Option<EntryTitle>,
    episode: Option<PlaybackEpisode>,
}

impl TryFrom<WirePlayback> for Playback {
    type Error = String;

    fn try_from(wire: WirePlayback) -> Result<Self, String> {
        let (kind, item) = match (wire.show, wire.movie, wire.anime) {
            (Some(item), None, None) => (Kind::Show, item),
            (None, Some(item), None) => (Kind::Movie, item),
            (None, None, Some(item)) => (Kind::Anime, item),
            _ => return Err(format!("playback {} needs exactly one title", wire.id)),
        };
        Ok(Self {
            id: wire.id,
            progress: wire.progress,
            paused_at: wire.paused_at,
            kind,
            item,
            episode: wire.episode,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaybackEpisode {
    pub season: Option<u32>,
    #[serde(alias = "number")]
    pub episode: u32,
    pub title: Option<String>,
}

/// Paused movies and episodes, for "continue watching".
#[derive(Debug, Clone, Serialize, Validate)]
pub struct GetPlayback {}

endpoint!(GetPlayback: Read => Vec<Playback>, path = |self| "/sync/playback");

/// A title to write to, identified by any IDs Simkl can resolve. Writes
/// accept external IDs directly; no lookup is needed first.
#[derive(Debug, Clone, Validate, bon::Builder)]
#[builder(on(String, into))]
#[garde(allow_unvalidated)]
pub struct TitleRef {
    pub kind: Kind,
    pub ids: Ids,
    pub title: Option<String>,
    pub year: Option<Year>,
}

impl TitleRef {
    pub fn simkl(kind: Kind, id: SimklId) -> Self {
        Self::builder().kind(kind).ids(Ids::simkl(id)).build()
    }
}

/// Which part of a title a history write covers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Target {
    /// The whole movie or show.
    #[default]
    Title,
    /// Every episode of these seasons.
    Seasons(Vec<u32>),
    /// These episodes. Anime episodes are numbered within season 1.
    Episodes(Vec<EpisodeNumber>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EpisodeNumber {
    pub season: u32,
    pub episode: u32,
}

impl Target {
    /// The `seasons` array Simkl expects; `None` means the whole title.
    fn seasons(&self) -> Option<Vec<WireSeason>> {
        match self {
            Target::Title => None,
            Target::Seasons(numbers) => Some(
                numbers
                    .iter()
                    .map(|&number| WireSeason {
                        number,
                        episodes: None,
                    })
                    .collect(),
            ),
            Target::Episodes(episodes) => {
                let mut episodes = episodes.clone();
                episodes.sort();
                let seasons = episodes
                    .chunk_by(|a, b| a.season == b.season)
                    .map(|season| WireSeason {
                        number: season[0].season,
                        episodes: Some(
                            season
                                .iter()
                                .map(|e| WireEpisode { number: e.episode })
                                .collect(),
                        ),
                    });
                Some(seasons.collect())
            }
        }
    }
}

/// Move a title to a status, adding it to the library if needed.
#[derive(Debug, Clone, Validate)]
pub struct StatusChange {
    #[garde(skip)]
    pub title: TitleRef,
    #[garde(custom(status_fits(self.title.kind)))]
    pub status: Status,
}

/// One history entry.
#[derive(Debug, Clone, Validate, bon::Builder)]
pub struct HistoryItem {
    #[garde(skip)]
    pub title: TitleRef,
    #[builder(default)]
    #[garde(custom(target_fits(self.title.kind)))]
    pub target: Target,
    /// When it was watched; Simkl uses the current time when absent. Ignored
    /// when removing.
    #[garde(skip)]
    pub watched_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Validate)]
pub struct RatingItem {
    #[garde(skip)]
    pub title: TitleRef,
    #[garde(range(min = 1, max = 10))]
    pub score: u8,
}

fn status_fits(kind: Kind) -> impl FnOnce(&Status, &()) -> garde::Result {
    move |status, _| match status.applies_to(kind) {
        true => Ok(()),
        false => Err(garde::Error::new(format!(
            "a movie cannot be `{status}`; movies are plantowatch, completed or dropped"
        ))),
    }
}

fn target_fits(kind: Kind) -> impl FnOnce(&Target, &()) -> garde::Result {
    move |target, _| match kind.is_episodic() || *target == Target::Title {
        true => Ok(()),
        false => Err(garde::Error::new("movies have no seasons or episodes")),
    }
}

/// Declares a `/sync` write: a list of items, each turned into the
/// `(Kind, WireItem)` it is sent as.
macro_rules! writes {
    ($(
        $(#[$doc:meta])*
        $name:ident($item:ty) => $path:literal, |$it:ident| $wire:expr;
    )+) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Validate)]
        #[serde(into = "WireBody")]
        pub struct $name(#[garde(dive)] pub Vec<$item>);

        endpoint!($name: Write => WriteOutcome, path = |self| $path,
            parse = |_, reply| WriteOutcome::from_reply(reply));

        impl From<$name> for WireBody {
            fn from($name(items): $name) -> Self {
                items.into_iter().map(|$it: $item| $wire).collect()
            }
        }
    )+};
}

writes! {
    /// Move titles to a status, adding them to the library if needed.
    SetStatus(StatusChange) => "/sync/add-to-list",
        |it| (it.title.kind, WireItem { to: Some(it.status), ..WireItem::from(it.title) });
    /// Record watches. A whole show marks every aired episode.
    MarkWatched(HistoryItem) => "/sync/history",
        |it| (it.title.kind, WireItem {
            watched_at: it.watched_at,
            seasons: it.target.seasons(),
            ..WireItem::from(it.title)
        });
    /// Remove watches. A whole title is removed from the library, rating
    /// included.
    UnmarkWatched(HistoryItem) => "/sync/history/remove",
        |it| (it.title.kind, WireItem { seasons: it.target.seasons(), ..WireItem::from(it.title) });
    /// Rate titles. Rating a title not in the library adds it.
    Rate(RatingItem) => "/sync/ratings",
        |it| (it.title.kind, WireItem { rating: Some(it.score), ..WireItem::from(it.title) });
    /// Clear ratings.
    Unrate(TitleRef) => "/sync/ratings/remove",
        |it| (it.kind, WireItem::from(it));
}

/// What a write couldn't apply. Simkl answers `2xx` even when some items
/// didn't match a title, so check this.
#[derive(Debug, Clone)]
pub struct WriteOutcome {
    pub not_found: Vec<NotFound>,
}

impl WriteOutcome {
    fn from_reply(reply: Reply) -> Result<Self> {
        #[derive(Deserialize)]
        struct Response {
            not_found: Buckets,
        }
        /// Anime lands in `shows`; only history writes report `episodes`.
        #[derive(Deserialize)]
        struct Buckets {
            movies: Vec<NotFound>,
            shows: Vec<NotFound>,
            #[serde(default)]
            episodes: Vec<NotFound>,
        }

        let Buckets {
            movies,
            shows,
            episodes,
        } = reply.json::<Response>()?.not_found;
        let not_found = movies.into_iter().chain(shows).chain(episodes).collect();
        Ok(Self { not_found })
    }
}

/// An item from a write that Simkl couldn't match to a title: the item as
/// sent, echoed back.
#[derive(Debug, Clone, Deserialize)]
pub struct NotFound {
    /// Echoed as `movie` or `show` on most writes; absent on some.
    #[serde(rename = "type")]
    pub kind: Option<Kind>,
    pub title: Option<String>,
    #[serde(default)]
    pub ids: Ids,
}

/// The body every `/sync` write takes.
#[derive(Serialize)]
struct WireBody {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    movies: Vec<WireItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    shows: Vec<WireItem>,
}

impl FromIterator<(Kind, WireItem)> for WireBody {
    fn from_iter<I: IntoIterator<Item = (Kind, WireItem)>>(items: I) -> Self {
        let (movies, shows): (Vec<_>, Vec<_>) = items
            .into_iter()
            .partition(|(kind, _)| *kind == Kind::Movie);
        let items = |list: Vec<(Kind, WireItem)>| list.into_iter().map(|(_, item)| item).collect();
        Self {
            movies: items(movies),
            shows: items(shows),
        }
    }
}

#[skip_serializing_none]
#[derive(Default, Serialize)]
struct WireItem {
    ids: Ids,
    title: Option<String>,
    year: Option<Year>,
    to: Option<Status>,
    watched_at: Option<Timestamp>,
    rating: Option<u8>,
    seasons: Option<Vec<WireSeason>>,
}

impl From<TitleRef> for WireItem {
    fn from(title: TitleRef) -> Self {
        Self {
            ids: title.ids,
            title: title.title,
            year: title.year,
            ..Self::default()
        }
    }
}

#[skip_serializing_none]
#[derive(Serialize)]
struct WireSeason {
    number: u32,
    episodes: Option<Vec<WireEpisode>>,
}

#[derive(Serialize)]
struct WireEpisode {
    number: u32,
}

#[cfg(test)]
#[path = "library_test.rs"]
mod tests;
