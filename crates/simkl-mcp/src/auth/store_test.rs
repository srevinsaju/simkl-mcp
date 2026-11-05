use oauth_as::storage_conformance::StorageConformance;

use super::Store;
use crate::auth::open_database;

#[tokio::test(flavor = "multi_thread")]
async fn passes_storage_conformance() {
    let violations = StorageConformance::new(|| async {
        Store::new(open_database("sqlite::memory:").await.unwrap())
    })
    .with_spawn(|task| {
        tokio::spawn(task);
    })
    .run()
    .await;
    assert!(violations.is_empty(), "{violations:#?}");
}
