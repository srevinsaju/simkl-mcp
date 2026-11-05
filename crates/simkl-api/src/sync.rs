//! Reading the user's library, gated on `/sync/activities`
//! ([sync](https://api.simkl.org/guides/sync)).
//!
//! Each read checks activities first. If nothing changed, the snapshot is
//! returned as is. If entries changed, only those are re-read (`date_from`)
//! and merged in. Deltas don't report removals, so a removal triggers a full
//! re-read.
//!
//! Simkl may suspend clients that poll `/sync/all-items` without checking
//! activities, so these endpoints are private to this module and
//! [`UserClient::library`] is the only way in. The snapshot is a cache owned
//! by the client's per-user state; losing it costs one full read.

use std::sync::Arc;

use garde::Validate;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio::sync::Mutex;

use crate::client::UserClient;
use crate::error::Result;
use crate::library::Library;

/// The user's library as of `activities`.
pub struct Snapshot {
    activities: Activities,
    library: Arc<Library>,
}

/// Read the library through `slot`, the user's snapshot. One refresh per
/// user at a time; concurrent readers wait for it.
pub async fn read_library(
    user: &UserClient,
    slot: &Mutex<Option<Snapshot>>,
) -> Result<Arc<Library>> {
    let mut slot = slot.lock().await;
    let activities = user.call(GetActivities {}).await?;
    let seen = slot.as_ref().map(|snapshot| &snapshot.activities);

    let library = match refresh(seen, &activities) {
        Refresh::Keep => Arc::clone(&slot.as_ref().expect("Keep needs a snapshot").library),
        Refresh::Delta(since) => {
            let mut library = Arc::clone(&slot.as_ref().expect("Delta needs a snapshot").library);
            let changed = user.call(GetLibrary::since(since)).await?;
            merge(Arc::make_mut(&mut library), changed);
            library
        }
        Refresh::Full => Arc::new(user.call(GetLibrary::default()).await?),
    };

    *slot = Some(Snapshot {
        activities,
        library: Arc::clone(&library),
    });
    Ok(library)
}

/// What to re-read, given the activities of the last snapshot and now.
#[derive(Debug, PartialEq, Eq)]
enum Refresh {
    Keep,
    /// Entries changed strictly after this `/sync/activities` timestamp.
    Delta(String),
    Full,
}

fn refresh(seen: Option<&Activities>, now: &Activities) -> Refresh {
    let Some(seen) = seen else {
        return Refresh::Full;
    };
    if seen.all == now.all {
        return Refresh::Keep;
    }
    match &seen.all {
        Some(since) if !now.removed_since(seen) => Refresh::Delta(since.clone()),
        _ => Refresh::Full,
    }
}

/// Replace entries with their changed versions, adding new ones.
fn merge(library: &mut Library, changed: Library) {
    library.shows.extend(changed.shows);
    library.movies.extend(changed.movies);
    library.anime.extend(changed.anime);
}

/// When each part of the library last changed. Kept as Simkl's strings:
/// `all` is passed back verbatim as `date_from`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Activities {
    all: Option<String>,
    tv_shows: KindActivities,
    anime: KindActivities,
    movies: KindActivities,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct KindActivities {
    removed_from_list: Option<String>,
}

impl Activities {
    /// Whether anything was removed from the library since `previous`.
    fn removed_since(&self, previous: &Activities) -> bool {
        let removed = |a: &Activities| {
            [&a.tv_shows, &a.anime, &a.movies].map(|kind| kind.removed_from_list.clone())
        };
        removed(self) != removed(previous)
    }
}

#[derive(Debug, Clone, Serialize, Validate)]
struct GetActivities {}

endpoint!(GetActivities: Read => Activities, path = |self| "/sync/activities");

/// The whole library, or the entries changed since a timestamp.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
struct GetLibrary {
    #[serde(rename = "date_from")]
    changed_since: Option<String>,
    next_watch_info: &'static str,
}

impl Default for GetLibrary {
    fn default() -> Self {
        Self {
            changed_since: None,
            next_watch_info: "yes",
        }
    }
}

impl GetLibrary {
    fn since(changed_since: String) -> Self {
        Self {
            changed_since: Some(changed_since),
            ..Self::default()
        }
    }
}

endpoint!(GetLibrary: Read => Library, path = |self| "/sync/all-items");

#[cfg(test)]
#[path = "sync_test.rs"]
mod tests;
