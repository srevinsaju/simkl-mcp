//! Titles in tool inputs and outputs: [`TitleInput`] names a title by any ID,
//! and [`TitleCard`] shows one with its simkl.com link, which Simkl requires
//! wherever its data is shown.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simkl_api::UserClient;
use simkl_api::data::DataTitle;
use simkl_api::library::{EntryTitle, TitleRef};
use simkl_api::media::{
    AnimeType, ExternalId, Ids, Kind, RelatedTitle, Resolve, SearchResult, SimklId, Title, Year,
};

use crate::tools::ToolError;

/// A title by its Simkl ID or any external ID. Give at least one.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TitleInput {
    pub kind: Kind,
    pub simkl_id: Option<u64>,
    /// IMDb ID, e.g. `tt0903747`.
    pub imdb: Option<String>,
    /// TMDB IDs are shared between movies and shows, so `kind` matters.
    pub tmdb: Option<String>,
    pub tvdb: Option<String>,
    /// MyAnimeList.
    pub mal: Option<String>,
    pub anidb: Option<String>,
    pub anilist: Option<String>,
    pub kitsu: Option<String>,
}

impl TitleInput {
    /// The title as Simkl writes take it. Fails if no ID was given.
    pub fn into_ref(self) -> Result<TitleRef, ToolError> {
        let external = |id: Option<String>| id.map(ExternalId::from);
        let ids = Ids {
            simkl: self.simkl_id.map(SimklId),
            slug: None,
            imdb: external(self.imdb),
            tmdb: external(self.tmdb),
            tvdb: external(self.tvdb),
            mal: external(self.mal),
            anidb: external(self.anidb),
            anilist: external(self.anilist),
            kitsu: external(self.kitsu),
        };
        if ids.simkl.is_none() && ids.external().next().is_none() {
            return Err(ToolError::Input(
                "give the title's `simkl_id` or an external ID; search for it first".into(),
            ));
        }
        Ok(TitleRef::builder().kind(self.kind).ids(ids).build())
    }

    /// The title's kind and Simkl ID, looking up external IDs on Simkl.
    /// Simkl's answer decides the kind, since a show may be filed as anime.
    pub async fn resolve(self, user: &UserClient) -> Result<(Kind, SimklId), ToolError> {
        let TitleRef { kind, ids, .. } = self.into_ref()?;
        if let Some(id) = ids.simkl {
            return Ok((kind, id));
        }
        user.call(Resolve {
            kind: Some(kind),
            ids,
        })
        .await?
        .ok_or(ToolError::NotFound { kind })
    }
}

/// A title, linked to its simkl.com page.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TitleCard {
    pub kind: Kind,
    pub title: String,
    /// English title of an anime, when `title` is romaji.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub en_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    pub simkl_id: u64,
    /// The title's simkl.com page. Link it wherever the title is shown.
    pub url: String,
    /// `tv`, `movie`, `ova`, ...: the format of an anime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anime_type: Option<String>,
}

impl TitleCard {
    fn new(
        kind: Kind,
        title: &str,
        ids: &Ids,
        year: Option<Year>,
        en_title: Option<&str>,
        anime_type: Option<&AnimeType>,
    ) -> Result<Self, ToolError> {
        let no_id = || ToolError::NoSimklId {
            title: title.to_owned(),
        };
        let simkl_id = ids.simkl.ok_or_else(no_id)?;
        Ok(Self {
            kind,
            title: title.to_owned(),
            en_title: en_title.map(str::to_owned),
            year: year.map(u16::from),
            simkl_id: simkl_id.into(),
            url: ids.url(kind).ok_or_else(no_id)?,
            anime_type: anime_type.map(ToString::to_string),
        })
    }

    pub fn of_title(title: &Title) -> Result<Self, ToolError> {
        let Title {
            kind, ids, year, ..
        } = title;
        Self::new(
            *kind,
            &title.title,
            ids,
            *year,
            title.en_title.as_deref(),
            title.anime_type.as_ref(),
        )
    }

    pub fn of_search(result: &SearchResult) -> Result<Self, ToolError> {
        let SearchResult {
            kind, ids, year, ..
        } = result;
        Self::new(
            *kind,
            &result.title,
            ids,
            *year,
            result.title_en.as_deref(),
            result.anime_type.as_ref(),
        )
    }

    pub fn of_related(related: &RelatedTitle) -> Result<Self, ToolError> {
        let RelatedTitle {
            kind, ids, year, ..
        } = related;
        Self::new(
            *kind,
            &related.title,
            ids,
            *year,
            related.en_title.as_deref(),
            related.anime_type.as_ref(),
        )
    }

    /// Trending and calendar files carry no kind or year of their own.
    pub fn of_data(kind: Kind, title: &DataTitle) -> Result<Self, ToolError> {
        Self::new(
            kind,
            &title.title,
            &title.ids,
            None,
            None,
            title.anime_type.as_ref(),
        )
    }

    /// Library and playback entries carry no kind of their own.
    pub fn of_entry(
        kind: Kind,
        title: &EntryTitle,
        anime_type: Option<&AnimeType>,
    ) -> Result<Self, ToolError> {
        Self::new(kind, &title.title, &title.ids, title.year, None, anime_type)
    }

    /// One line of a text summary: `Title (2008) · show · https://simkl.com/...`.
    pub fn line(&self) -> String {
        let year = self.year.map(|y| format!(" ({y})")).unwrap_or_default();
        format!("{}{year} · {} · {}", self.title, self.kind, self.url)
    }
}

#[cfg(test)]
#[path = "title_test.rs"]
mod tests;
