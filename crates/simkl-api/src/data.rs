//! Static data files on `data.simkl.in`: trending (including by genre) and
//! calendar. Public, edge-cached, and exempt from quota.
//!
//! Simkl requires attribution wherever trending data is shown ("Trending on
//! Simkl").

use std::collections::HashMap;

use garde::Validate;
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::{NoneAsEmptyString, serde_as};
use strum::IntoStaticStr;

use crate::client::Reply;
use crate::error::Result;
use crate::media::{AnimeType, Ids, Kind, Ratings, SimklId};

/// How far back trending counts views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, IntoStaticStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Period {
    Today,
    Week,
    Month,
}

/// A title in a trending or calendar file.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct DataTitle {
    pub title: String,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub title_romaji: Option<String>,
    pub ids: Ids,
    pub poster: Option<String>,
    pub fanart: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// `MM/DD/YYYY` on trending files, RFC 3339 on calendars.
    pub release_date: Option<String>,
    pub rank: Option<u32>,
    #[serde(default)]
    pub ratings: Ratings,
    /// Display string, e.g. `2h 37m`.
    pub runtime: Option<String>,
    /// `ongoing`, `ended`, `premiere`, `soon`, ...
    pub status: Option<String>,
    pub network: Option<String>,
    pub total_episodes: Option<u32>,
    pub anime_type: Option<AnimeType>,
    /// In trending files: viewers within the file's period.
    pub watched: Option<u64>,
    pub watching: Option<u64>,
    pub completed: Option<u64>,
    pub plan_to_watch: Option<u64>,
}

/// The most watched titles of a kind.
#[derive(Debug, Clone, Serialize, Validate, bon::Builder)]
#[garde(allow_unvalidated)]
pub struct Trending {
    #[serde(skip)]
    pub kind: Kind,
    #[serde(skip)]
    pub period: Period,
    /// The top 500 instead of the top 100.
    #[serde(skip)]
    #[builder(default)]
    pub top_500: bool,
}

endpoint!(Trending: Data => Vec<DataTitle>, path = |self| format!(
    "/discover/trending/{}/{}_{}.json",
    self.kind.trending_type(),
    <&str>::from(self.period),
    if self.top_500 { 500 } else { 100 },
));

/// A genre with trending titles this month.
#[derive(Debug, Clone, Deserialize)]
pub struct Genre {
    pub name: String,
    pub slug: String,
    pub count: u32,
}

/// Genres with trending titles this month. Slugs differ between kinds; use
/// them with [`TrendingInGenre`].
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct TrendingGenres {
    #[serde(skip)]
    pub kind: Kind,
}

endpoint!(TrendingGenres: Data => Vec<Genre>,
    path = |self| format!("/discover/trending/{}/genre/index.json", self.kind.trending_type()));

/// This month's most watched titles in one genre, up to 500.
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct TrendingInGenre {
    #[serde(skip)]
    pub kind: Kind,
    #[serde(skip)]
    #[garde(pattern(r"^[a-z0-9-]+$"))]
    pub slug: String,
}

endpoint!(TrendingInGenre: Data => Vec<DataTitle>, path = |self| format!(
    "/discover/trending/{}/genre/{}_month_500.json",
    self.kind.trending_type(),
    self.slug,
));

/// Airings and releases: yesterday through the next month, or one calendar
/// month (Simkl publishes up to three months ahead).
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct Calendar {
    #[serde(skip)]
    pub kind: Kind,
    #[serde(skip)]
    #[garde(dive)]
    pub month: Option<Month>,
}

#[derive(Debug, Clone, Copy, Validate)]
pub struct Month {
    #[garde(skip)]
    pub year: i16,
    #[garde(range(min = 1, max = 12))]
    pub month: u8,
}

endpoint!(Calendar: Data => Vec<CalendarEntry>,
    path = |self| match self.month {
        None => format!("/calendar/v2/{}.json", self.kind.calendar_type()),
        Some(Month { year, month }) => format!("/calendar/v2/{year}/{month}/{}.json", self.kind.calendar_type()),
    },
    parse = |_, reply| CalendarEntry::join(reply));

/// One airing or release, with its title.
#[derive(Debug, Clone)]
pub struct CalendarEntry {
    pub id: SimklId,
    pub date: Timestamp,
    pub episode: Option<CalendarEpisode>,
    /// 1 mid-season, 2 season, 3 series finale.
    pub finale_type: Option<u8>,
    /// Missing if the file had no metadata for this title.
    pub title: Option<DataTitle>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalendarEpisode {
    /// Absent for anime.
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub title: Option<String>,
    pub runtime: Option<u32>,
}

impl CalendarEntry {
    /// Calendar files list entries, with each title's metadata kept apart
    /// under its Simkl ID as a string.
    fn join(reply: Reply) -> Result<Vec<Self>> {
        #[derive(Deserialize)]
        struct File {
            calendar: Vec<Entry>,
            #[serde(default)]
            metadata: HashMap<String, DataTitle>,
        }
        #[derive(Deserialize)]
        struct Entry {
            simkl_id: SimklId,
            date: Timestamp,
            episode: Option<CalendarEpisode>,
            finale_type: Option<u8>,
        }

        let File { calendar, metadata } = reply.json()?;
        Ok(calendar
            .into_iter()
            .map(|entry| Self {
                title: metadata.get(&entry.simkl_id.to_string()).cloned(),
                id: entry.simkl_id,
                date: entry.date,
                episode: entry.episode,
                finale_type: entry.finale_type,
            })
            .collect())
    }
}
