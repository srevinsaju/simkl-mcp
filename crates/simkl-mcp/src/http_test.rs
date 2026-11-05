use axum::body::Bytes;
use oauth_as::scope::ScopeSet;
use rstest::rstest;
use serde_json::json;

use super::needs_step_up;
use crate::tools::write_tool_names;

fn call(name: &str) -> serde_json::Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": {}}})
}

/// The step-up decision, against the real tool list: a token without
/// `media:write` may call read tools but must step up for every tool not
/// marked read-only, so a new write tool cannot slip through as a read.
#[rstest]
#[case::read_tool_with_read_token("media:read", call("search"), false)]
#[case::library_read_with_read_token("media:read", call("get_library"), false)]
#[case::write_tool_with_read_token("media:read", call("mark_watched"), true)]
#[case::destructive_tool_with_read_token("media:read", call("unrate"), true)]
#[case::write_tool_with_write_token("media:read media:write", call("set_status"), false)]
#[case::write_call_inside_a_batch("media:read", json!([call("search"), call("rate")]), true)]
#[case::not_a_tool_call("media:read", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}), false)]
fn steps_up_for_write_tools(
    #[case] scope: &str,
    #[case] body: serde_json::Value,
    #[case] expected: bool,
) {
    let scope = ScopeSet::parse(scope).unwrap();
    let body = Bytes::from(serde_json::to_vec(&body).unwrap());
    assert_eq!(needs_step_up(&scope, &body, &write_tool_names()), expected);
}
