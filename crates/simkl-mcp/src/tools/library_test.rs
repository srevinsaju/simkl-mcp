use rstest::rstest;
use serde_json::{Value, json};
use simkl_api::library::Library;

use super::{GetLibraryInput, select};
use crate::tools::{Page, paginate};

fn entry(key: &str, id: u64, title: &str, status: &str, extra: Value) -> Value {
    let mut entry = json!({
        key: {"title": title, "ids": {"simkl": id, "slug": title.to_lowercase()}},
        "status": status,
    });
    entry
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    entry
}

fn library() -> Library {
    serde_json::from_value(json!({
        "shows": [
            entry("show", 1, "Breaking Bad", "completed",
                json!({"last_watched_at": "2026-01-01T00:00:00Z", "user_rating": 10})),
            entry("show", 2, "Severance", "watching",
                json!({"last_watched_at": "2026-09-01T00:00:00Z", "user_rating": 8})),
            entry("show", 3, "Andor", "plantowatch", json!({})),
        ],
        "movies": [
            entry("movie", 4, "Inception", "completed",
                json!({"last_watched_at": "2025-06-01T00:00:00Z"})),
        ],
        "anime": [
            entry("show", 5, "Frieren", "watching",
                json!({"last_watched_at": "2026-10-01T00:00:00Z", "user_rating": 9})),
        ],
    }))
    .unwrap()
}

fn input(value: Value) -> GetLibraryInput {
    serde_json::from_value(value).unwrap()
}

/// `get_library` filters by kind, status and title, then sorts. Missing dates
/// and ratings sort last rather than first.
#[rstest]
#[case::recently_watched_first(json!({}), vec![5, 2, 1, 4, 3])]
#[case::by_status(json!({"status": "watching"}), vec![5, 2])]
#[case::by_kind_and_status(json!({"kind": "show", "status": "completed"}), vec![1])]
#[case::query_ignores_case(json!({"query": "SEVER"}), vec![2])]
#[case::highest_rated_first(json!({"sort": "rating"}), vec![1, 5, 2, 3, 4])]
#[case::by_title(json!({"sort": "title"}), vec![3, 1, 5, 4, 2])]
fn library_selection(#[case] value: Value, #[case] expected: Vec<u64>) {
    let library = library();
    let ids: Vec<u64> = select(&library, &input(value))
        .into_iter()
        .map(|(_, entry)| entry.item.ids.simkl.unwrap().into())
        .collect();
    assert_eq!(ids, expected);
}

#[test]
fn library_paging() {
    let library = library();
    let entries = select(&library, &GetLibraryInput::default());
    let (page, position) = paginate(entries, 3, 2);
    assert_eq!(page.len(), 1);
    assert_eq!(
        position,
        Page {
            page: 3,
            page_count: 3,
            total: 5
        }
    );
}
