use rstest::rstest;
use serde_json::{Value, json};

use super::{
    EpisodeNumber, HistoryItem, MarkWatched, SetStatus, Status, StatusChange, Target, TitleRef,
};
use crate::media::{Kind, SimklId};

fn body(write: impl serde::Serialize) -> Value {
    serde_json::to_value(write).unwrap()
}

fn episode(season: u32, episode: u32) -> EpisodeNumber {
    EpisodeNumber { season, episode }
}

/// Simkl's write bodies ([mark as watched](https://api.simkl.org/guides/mark-as-watched)):
/// anime goes under `shows`, which `/sync/history/remove` requires, and
/// episodes are grouped by season.
#[rstest]
#[case::anime_episodes_grouped_under_shows(
    body(MarkWatched(vec![HistoryItem::builder()
        .title(TitleRef::simkl(Kind::Anime, SimklId(39687)))
        .target(Target::Episodes(vec![episode(2, 1), episode(1, 3), episode(1, 1)]))
        .build()])),
    json!({"shows": [{"ids": {"simkl": 39687}, "seasons": [
        {"number": 1, "episodes": [{"number": 1}, {"number": 3}]},
        {"number": 2, "episodes": [{"number": 1}]},
    ]}]}),
)]
#[case::kinds_split_into_movies_and_shows(
    body(SetStatus(vec![
        StatusChange { title: TitleRef::simkl(Kind::Movie, SimklId(53536)), status: Status::Completed },
        StatusChange { title: TitleRef::simkl(Kind::Show, SimklId(17465)), status: Status::Watching },
    ])),
    json!({
        "movies": [{"ids": {"simkl": 53536}, "to": "completed"}],
        "shows": [{"ids": {"simkl": 17465}, "to": "watching"}],
    }),
)]
fn write_bodies(#[case] body: Value, #[case] expected: Value) {
    assert_eq!(body, expected);
}
