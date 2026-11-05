use std::time::Duration;

use super::PendingSignIns;

/// A pending sign-in is handed out once, and only to the browser holding its
/// cookie; any attempt, right or wrong, uses it up. This is what stops a
/// replayed consent form or callback, and someone else's browser finishing
/// your sign-in.
#[tokio::test]
async fn pending_sign_ins_are_single_use_and_bound_to_their_browser() {
    let pending = PendingSignIns::new(Duration::from_secs(60));

    pending
        .put("a".to_owned(), "browser-1".to_owned(), "first")
        .await;
    assert_eq!(pending.take("a", Some("browser-1")).await, Some("first"));
    assert_eq!(pending.take("a", Some("browser-1")).await, None);

    pending
        .put("b".to_owned(), "browser-1".to_owned(), "second")
        .await;
    assert_eq!(pending.take("b", Some("browser-2")).await, None);
    assert_eq!(pending.take("b", Some("browser-1")).await, None);

    pending
        .put("c".to_owned(), "browser-1".to_owned(), "third")
        .await;
    assert_eq!(pending.take("c", None).await, None);
}
