use rstest::rstest;
use serde_json::{Value, json};
use simkl_api::media::{ExternalId, Ids, SimklId};

use super::TitleInput;

fn input(value: Value) -> TitleInput {
    serde_json::from_value(value).unwrap()
}

/// A title needs an ID before anything is sent: writes and lookups have
/// nothing to match a bare kind against.
#[rstest]
#[case::simkl_id(
    json!({"kind": "show", "simkl_id": 17465}),
    Some(Ids::simkl(SimklId(17465))),
)]
#[case::external_id(
    json!({"kind": "movie", "imdb": "tt1375666"}),
    Some(Ids { imdb: Some(ExternalId::from("tt1375666")), ..Ids::default() }),
)]
#[case::no_id(json!({"kind": "anime"}), None)]
fn title_refs(#[case] value: Value, #[case] expected: Option<Ids>) {
    let ids = input(value).into_ref().ok().map(|title| title.ids);
    assert_eq!(ids, expected);
}
