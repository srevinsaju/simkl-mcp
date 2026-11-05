//! Which MCP clients may register, and what they may ask for.
//!
//! Dynamic Client Registration (RFC 7591) is open: a registered client
//! reaches nobody's data until a user signs in with Simkl and approves it on
//! our consent page. Client ID Metadata Documents come in a later PR.

use oauth_as::client::{Client, ClientId};
use oauth_as::registration::{RegistrationAttempt, RegistrationDecision, RegistrationPolicy};
use oauth_as::scope::ScopeSet;
use oauth_as::store::{Storage, StorageError};

use crate::auth::store::Store;

/// Read the user's library, and the catalog. Maps to Simkl's `media:read`.
pub const READ_SCOPE: &str = "media:read";
/// Change the user's library. Maps to Simkl's `media:write`.
pub const WRITE_SCOPE: &str = "media:write";

/// Every scope a client may hold.
pub fn all_scopes() -> ScopeSet {
    ScopeSet::from_tokens([READ_SCOPE, WRITE_SCOPE]).unwrap()
}

/// Allows every registration; see the module docs.
pub struct OpenRegistration;

impl RegistrationPolicy for OpenRegistration {
    fn authorize(&self, _: &RegistrationAttempt<'_>) -> RegistrationDecision {
        RegistrationDecision::Allow
    }
}

/// Lets a dynamically registered client ask for read and write, defaulting
/// to read, the first time it starts a sign-in.
///
/// `oauth-as` stores the scope a client registered with as both its ceiling
/// and its default, so a client that registered none could ask for nothing,
/// and one that registered read could never step up to write. Registration
/// management is off, so nothing else rewrites a client afterwards.
pub async fn widen_registered_scopes(
    store: &Store,
    client_id: &ClientId,
) -> Result<(), StorageError> {
    let Some(client) = store.get_client(client_id).await? else {
        return Ok(());
    };
    let read = ScopeSet::from_tokens([READ_SCOPE]).unwrap();
    if client.registration.is_none()
        || (client.allowed_scopes == all_scopes() && client.default_scopes == read)
    {
        return Ok(());
    }
    let mut widened = Client::clone(&client);
    widened.allowed_scopes = all_scopes();
    widened.default_scopes = read;
    // Losing the race to another sign-in is fine: it widened the same way.
    store.compare_and_swap_client(&client, widened).await?;
    Ok(())
}
