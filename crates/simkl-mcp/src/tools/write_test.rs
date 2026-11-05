use rstest::rstest;
use simkl_api::library::{EpisodeNumber, NotFound, Target, TitleRef, WriteOutcome};
use simkl_api::media::{Ids, Kind, SimklId};

use super::{Action, EpisodeInput, outcome, target};
use crate::tools::ToolError;

fn episode(season: u32, episode: u32) -> EpisodeInput {
    EpisodeInput { season, episode }
}

/// A history item covers the whole title, some seasons or some episodes;
/// both, or an empty list, is refused before anything is sent.
#[rstest]
#[case::whole_title(None, None, Some(Target::Title))]
#[case::seasons(Some(vec![1, 2]), None, Some(Target::Seasons(vec![1, 2])))]
#[case::episodes(
    None,
    Some(vec![episode(1, 3)]),
    Some(Target::Episodes(vec![EpisodeNumber { season: 1, episode: 3 }])),
)]
#[case::both(Some(vec![1]), Some(vec![episode(1, 3)]), None)]
#[case::no_seasons(Some(vec![]), None, None)]
#[case::no_episodes(None, Some(vec![]), None)]
fn history_targets(
    #[case] seasons: Option<Vec<u32>>,
    #[case] episodes: Option<Vec<EpisodeInput>>,
    #[case] expected: Option<Target>,
) {
    assert_eq!(target(seasons, episodes).ok(), expected);
}

fn title(id: u64) -> TitleRef {
    TitleRef::simkl(Kind::Show, SimklId(id))
}

fn not_found(id: u64) -> NotFound {
    NotFound {
        kind: Some(Kind::Show),
        title: None,
        ids: Ids::simkl(SimklId(id)),
    }
}

/// Simkl answers success even for items it couldn't match. A title it echoed
/// back is not applied; an unmatched episode leaves its title applied; and a
/// write that applied nothing is an error.
#[rstest]
#[case::all_applied(vec![], Some((vec![1, 2], 0)))]
#[case::one_missed(vec![not_found(2)], Some((vec![1], 1)))]
#[case::unmatched_episode(vec![not_found(999)], Some((vec![1, 2], 1)))]
#[case::none_applied(vec![not_found(1), not_found(2)], None)]
fn write_outcomes(#[case] missed: Vec<NotFound>, #[case] expected: Option<(Vec<u64>, usize)>) {
    let written = WriteOutcome { not_found: missed };
    let result = outcome(Action::MarkWatched, vec![title(1), title(2)], written);
    let result = match result {
        Ok(output) => Some((
            output
                .applied
                .iter()
                .map(|title| title.ids.simkl_id.unwrap())
                .collect(),
            output.not_found.len(),
        )),
        Err(ToolError::NothingApplied { .. }) => None,
        Err(error) => panic!("unexpected error: {error}"),
    };
    assert_eq!(result, expected);
}
