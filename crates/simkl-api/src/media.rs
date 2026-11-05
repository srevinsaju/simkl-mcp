//! Catalog types and endpoints: `Kind`, titles, episodes, search, and
//! resolving external IDs.
//!
//! Simkl names the same ID `simkl` or `simkl_id` depending on the endpoint,
//! and sends external IDs as strings or numbers; both are normalised here.
//! Every title exposes its simkl.com URL, which Simkl requires wherever its
//! data is shown.
//!
//! See [standard media objects](https://api.simkl.org/conventions/standard-media-objects).

use std::collections::BTreeMap;
use std::str::FromStr;

use derive_more::{AsRef, Display, From, Into};
use garde::Validate;
use jiff::Timestamp;
use jiff::civil::Date;
use schemars::JsonSchema;
use serde::de::{DeserializeOwned, IntoDeserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{
    DeserializeFromStr, DisplayFromStr, NoneAsEmptyString, PickFirst, SerializeDisplay, serde_as,
    skip_serializing_none,
};
use strum::{Display as StrumDisplay, EnumString};

use crate::client::Reply;
use crate::error::{Error, Result};

/// Simkl's own ID for a title or episode. The only stable identifier.
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
    Display,
    From,
    Into,
)]
#[serde(transparent)]
pub struct SimklId(pub u64);

/// An external ID (IMDb, TMDB, MAL, ...). Always a string, though Simkl
/// sometimes sends numbers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, From, AsRef)]
#[serde(from = "StringOrNumber")]
#[from(forward)]
#[as_ref(forward)]
pub struct ExternalId(String);

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrNumber {
    String(String),
    Number(serde_json::Number),
}

impl From<StringOrNumber> for String {
    fn from(value: StringOrNumber) -> Self {
        match value {
            StringOrNumber::String(s) => s,
            StringOrNumber::Number(n) => n.to_string(),
        }
    }
}

/// A release year. Simkl sometimes sends it as a string.
#[serde_as]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Display, From, Into,
)]
#[serde(transparent)]
pub struct Year(#[serde_as(as = "PickFirst<(_, DisplayFromStr)>")] pub u16);

/// Movie, TV show or anime.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, StrumDisplay,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Kind {
    #[serde(alias = "movies")]
    Movie,
    /// Simkl calls these `show` on summaries and `tv` elsewhere.
    #[serde(alias = "tv", alias = "shows")]
    Show,
    Anime,
}

kind_names! {
    /// Path segment for catalog endpoints and simkl.com URLs: `/movies/{id}`.
    section: "movies", "tv", "anime";
    /// Path segment for `/search/{type}`, and `type` on `/redirect`.
    search_type: "movie", "tv", "anime";
    /// Directory of trending files on data.simkl.in.
    trending_type: "movies", "tv", "anime";
    /// Name of calendar files on data.simkl.in.
    calendar_type: "movie_release", "tv", "anime";
}

/// Parses any spelling Simkl uses for a kind: `movie`, `movies`, `show`,
/// `shows`, `tv`, `anime`.
impl FromStr for Kind {
    type Err = serde::de::value::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::deserialize(s.into_deserializer())
    }
}

impl Kind {
    /// Movies have no episodes, and no `watching` or `hold` status.
    pub fn is_episodic(self) -> bool {
        self != Kind::Movie
    }
}

/// The format of an anime. Simkl spells music videos two ways.
#[derive(
    Debug, Clone, PartialEq, Eq, StrumDisplay, EnumString, SerializeDisplay, DeserializeFromStr,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum AnimeType {
    Tv,
    Movie,
    Special,
    Ova,
    Ona,
    #[strum(to_string = "music video", serialize = "music")]
    MusicVideo,
}

/// Whether a show is still airing. Older records capitalise these.
#[derive(
    Debug, Clone, PartialEq, Eq, StrumDisplay, EnumString, SerializeDisplay, DeserializeFromStr,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum ShowStatus {
    Airing,
    Ended,
    Tba,
}

/// A title's IDs. Only `simkl` is a number. Keys we don't model
/// (`letterboxd`, `traktslug`, ...) are ignored.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ids {
    #[serde(alias = "simkl_id")]
    pub simkl: Option<SimklId>,
    /// Response only, and not unique; never sent back.
    #[serde(skip_serializing)]
    pub slug: Option<String>,
    pub imdb: Option<ExternalId>,
    pub tmdb: Option<ExternalId>,
    pub tvdb: Option<ExternalId>,
    pub mal: Option<ExternalId>,
    pub anidb: Option<ExternalId>,
    pub anilist: Option<ExternalId>,
    pub kitsu: Option<ExternalId>,
}

impl Ids {
    pub fn simkl(id: SimklId) -> Self {
        Self {
            simkl: Some(id),
            ..Self::default()
        }
    }

    /// The simkl.com page for a title of `kind`. Simkl's API rules require
    /// linking every title shown to its page.
    pub fn url(&self, kind: Kind) -> Option<String> {
        let id = self.simkl?;
        let slug = self.slug.as_deref().unwrap_or_default();
        Some(format!("https://simkl.com/{}/{id}/{slug}", kind.section()))
    }

    /// The external IDs `/redirect` accepts, in the order worth trying.
    pub fn external(&self) -> impl Iterator<Item = (&'static str, &ExternalId)> {
        [
            ("imdb", &self.imdb),
            ("tvdb", &self.tvdb),
            ("tmdb", &self.tmdb),
            ("mal", &self.mal),
            ("anidb", &self.anidb),
            ("anilist", &self.anilist),
            ("kitsu", &self.kitsu),
        ]
        .into_iter()
        .filter_map(|(key, value)| Some((key, value.as_ref()?)))
    }
}

/// Simkl and third-party ratings, keyed by source (`simkl`, `imdb`, `mal`).
pub type Ratings = BTreeMap<String, Rating>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    pub rating: Option<f64>,
    pub votes: Option<u64>,
}

/// A movie, show or anime.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Title {
    pub title: String,
    pub year: Option<Year>,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub ids: Ids,
    /// English title of an anime, when `title` is romaji.
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub en_title: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub overview: Option<String>,
    pub poster: Option<String>,
    pub fanart: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Minutes; for shows, a typical episode.
    pub runtime: Option<u32>,
    pub certification: Option<String>,
    pub country: Option<String>,
    pub network: Option<String>,
    pub status: Option<ShowStatus>,
    pub released: Option<Date>,
    /// `YYYY-MM-DD` or a timestamp, depending on the record.
    pub first_aired: Option<String>,
    pub last_aired: Option<String>,
    pub total_episodes: Option<u32>,
    pub anime_type: Option<AnimeType>,
    pub rank: Option<u32>,
    #[serde(default)]
    pub ratings: Ratings,
    /// Titles in the same genres.
    #[serde(default)]
    pub similar: Vec<RelatedTitle>,
    /// What people who watched this also watched.
    #[serde(default)]
    pub users_recommendations: Vec<RelatedTitle>,
}

impl Title {
    pub fn url(&self) -> Option<String> {
        self.ids.url(self.kind)
    }
}

/// A title listed inside another: similar titles and recommendations.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct RelatedTitle {
    pub title: String,
    pub year: Option<Year>,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub ids: Ids,
    pub poster: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub en_title: Option<String>,
    pub anime_type: Option<AnimeType>,
}

/// One episode of a show or anime.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub title: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub description: Option<String>,
    /// Absent on specials and on most anime episodes.
    pub season: Option<u32>,
    /// Absent on some specials.
    pub episode: Option<u32>,
    /// Minutes.
    pub runtime: Option<u32>,
    #[serde(rename = "type")]
    pub episode_type: EpisodeType,
    #[serde(default)]
    pub aired: bool,
    pub img: Option<String>,
    pub date: Option<Timestamp>,
    /// The episode's own IDs, not the show's.
    #[serde(default)]
    pub ids: Ids,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpisodeType {
    Episode,
    Special,
}

/// One search result.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub title: String,
    /// English title of an anime, when `title` is romaji.
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub title_en: Option<String>,
    pub year: Option<Year>,
    #[serde(rename = "endpoint_type")]
    pub kind: Kind,
    pub ids: Ids,
    pub poster: Option<String>,
    /// Only anime results carry `type`: their format.
    #[serde(rename = "type")]
    pub anime_type: Option<AnimeType>,
    pub status: Option<ShowStatus>,
    #[serde(rename = "ep_count")]
    pub total_episodes: Option<u32>,
    pub rank: Option<u32>,
    #[serde(default)]
    pub ratings: Ratings,
}

impl SearchResult {
    pub fn url(&self) -> Option<String> {
        self.ids.url(self.kind)
    }
}

/// One page of a paginated list.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub page_count: u32,
    pub item_count: u32,
}

impl<T: DeserializeOwned> Page<T> {
    fn from_reply(reply: Reply) -> Result<Self> {
        let header = |name| reply.header(name).and_then(|v| v.parse().ok());
        let page = header("x-pagination-page").unwrap_or(1);
        let page_count = header("x-pagination-page-count").unwrap_or(page);
        let item_count = header("x-pagination-item-count");
        let items: Vec<T> = reply.json()?;
        Ok(Self {
            item_count: item_count.unwrap_or(items.len() as u32),
            items,
            page,
            page_count,
        })
    }

    pub fn has_more(&self) -> bool {
        self.page < self.page_count
    }
}

/// A movie, show or anime by Simkl ID. `None` if Simkl doesn't know it.
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct GetTitle {
    #[serde(skip)]
    pub kind: Kind,
    #[serde(skip)]
    pub id: SimklId,
}

endpoint!(GetTitle: Catalog => Option<Title>,
    path = |self| format!("/{}/{}", self.kind.section(), self.id),
    parse = Self::matching);

impl GetTitle {
    /// Simkl answers `200 []` for an unknown ID, and some IDs return an
    /// unrelated title.
    fn matching(&self, reply: Reply) -> Result<Option<Title>> {
        let body: Value = reply.json()?;
        if body.as_array().is_some_and(Vec::is_empty) {
            return Ok(None);
        }
        let title: Title = serde_json::from_value(body)?;
        match title.ids.simkl {
            Some(id) if id == self.id => Ok(Some(title)),
            received => Err(Error::UnexpectedTitle {
                requested: self.id,
                received,
            }),
        }
    }
}

/// Every episode of a show or anime, specials last.
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct GetEpisodes {
    #[serde(skip)]
    #[garde(custom(episodic))]
    pub kind: Kind,
    #[serde(skip)]
    pub id: SimklId,
}

endpoint!(GetEpisodes: Catalog => Vec<Episode>,
    path = |self| format!("/{}/episodes/{}", self.kind.section(), self.id));

fn episodic(kind: &Kind, _: &()) -> garde::Result {
    match kind.is_episodic() {
        true => Ok(()),
        false => Err(garde::Error::new("movies have no episodes")),
    }
}

/// Search titles of one kind by text.
#[derive(Debug, Clone, Serialize, Validate, bon::Builder)]
#[garde(allow_unvalidated)]
pub struct Search {
    #[serde(skip)]
    pub kind: Kind,
    #[builder(into)]
    #[garde(length(min = 1))]
    pub q: String,
    /// From 1; Simkl stops at 20.
    #[builder(default = 1)]
    #[garde(range(min = 1, max = 20))]
    pub page: u32,
    #[builder(default = 10)]
    #[garde(range(min = 1, max = 50))]
    pub limit: u32,
    #[builder(skip = "full")]
    extended: &'static str,
}

endpoint!(Search: Read => Page<SearchResult>,
    path = |self| format!("/search/{}", self.kind.search_type()),
    parse = |_, reply| Page::from_reply(reply));

/// Find the Simkl ID for a title known by external IDs, using the first one
/// present. `kind` narrows the match; TMDB IDs need it, being shared between
/// movies and shows.
#[derive(Debug, Clone, Serialize, Validate)]
#[serde(into = "BTreeMap<&'static str, String>")]
#[garde(allow_unvalidated)]
pub struct Resolve {
    pub kind: Option<Kind>,
    #[garde(custom(has_external_id))]
    pub ids: Ids,
}

endpoint!(Resolve: Redirect => Option<(Kind, SimklId)>,
    path = |self| "/redirect",
    parse = |_, reply: Reply| parse_simkl_location(reply.header("location")));

impl From<Resolve> for BTreeMap<&'static str, String> {
    fn from(Resolve { kind, ids }: Resolve) -> Self {
        let mut query = btreemap! { "to" => "simkl".to_owned() };
        query.extend(
            ids.external()
                .take(1)
                .map(|(key, id)| (key, id.to_string())),
        );
        query.extend(kind.map(|kind| ("type", kind.search_type().to_owned())));
        query
    }
}

fn has_external_id(ids: &Ids, _: &()) -> garde::Result {
    match ids.external().next() {
        Some(_) => Ok(()),
        None => Err(garde::Error::new("no external ID to resolve")),
    }
}

/// Where `/redirect` pointed: `https://simkl.com/tv/17465/game-of-thrones`,
/// or protocol-relative `//simkl.com/...`. A bare `//simkl.com` means no
/// match ([null values](https://api.simkl.org/conventions/null-values)).
fn parse_simkl_location(location: Option<&str>) -> Result<Option<(Kind, SimklId)>> {
    let unexpected = || Error::UnexpectedRedirect {
        location: location.map(str::to_owned),
    };
    let location = location.ok_or_else(unexpected)?;
    let path = location.strip_prefix("https:").unwrap_or(location);
    let path = path.strip_prefix("//simkl.com").ok_or_else(unexpected)?;
    if path.is_empty() || path == "/" {
        return Ok(None);
    }
    let mut segments = path.strip_prefix('/').ok_or_else(unexpected)?.split('/');
    let kind = segments.next().and_then(|s| s.parse().ok());
    let id = segments.next().and_then(|s| s.parse().ok());
    match (kind, id) {
        (Some(kind), Some(id)) => Ok(Some((kind, SimklId(id)))),
        _ => Err(unexpected()),
    }
}
