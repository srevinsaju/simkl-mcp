use rmcp::model::ResourceContents;

use super::{MIME_TYPE, View};
use crate::tools::router;

/// Every view a tool points at is served as an MCP App, so a renamed or
/// unwired view fails here rather than as a blank frame in Claude.
#[test]
fn tool_views_are_served() {
    let uris: Vec<String> = router()
        .list_all()
        .into_iter()
        .filter_map(|tool| {
            let ui = tool.meta?.0.get("ui")?.clone();
            Some(ui["resourceUri"].as_str()?.to_owned())
        })
        .collect();
    assert!(uris.contains(&"ui://simkl/titles".to_owned()));
    for uri in uris {
        let contents = View::read(&uri).unwrap().contents;
        let [ResourceContents::TextResourceContents { mime_type, .. }] = contents.as_slice() else {
            panic!("{uri} is not one text resource");
        };
        assert_eq!(mime_type.as_deref(), Some(MIME_TYPE), "{uri}");
    }
}
