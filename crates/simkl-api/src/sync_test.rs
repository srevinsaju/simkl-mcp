use rstest::rstest;

use super::{Activities, KindActivities, Refresh, refresh};

fn activities(all: Option<&str>, removed: Option<&str>) -> Activities {
    let kind = |removed: Option<&str>| KindActivities {
        removed_from_list: removed.map(str::to_owned),
    };
    Activities {
        all: all.map(str::to_owned),
        tv_shows: kind(removed),
        anime: kind(None),
        movies: kind(None),
    }
}

/// Simkl's [sync rules](https://api.simkl.org/guides/sync): re-read nothing
/// when activities are unchanged, only the delta when entries changed, and
/// everything when something was removed, since deltas omit removals.
#[rstest]
#[case::first_read(None, activities(Some("t1"), None), Refresh::Full)]
#[case::unchanged(
    Some(activities(Some("t1"), None)),
    activities(Some("t1"), None),
    Refresh::Keep
)]
#[case::changed(
    Some(activities(Some("t1"), None)),
    activities(Some("t2"), None),
    Refresh::Delta("t1".to_owned())
)]
#[case::removed(
    Some(activities(Some("t1"), None)),
    activities(Some("t2"), Some("t2")),
    Refresh::Full
)]
#[case::first_activity(
    Some(activities(None, None)),
    activities(Some("t1"), None),
    Refresh::Full
)]
fn refresh_decision(
    #[case] seen: Option<Activities>,
    #[case] now: Activities,
    #[case] expected: Refresh,
) {
    assert_eq!(refresh(seen.as_ref(), &now), expected);
}
