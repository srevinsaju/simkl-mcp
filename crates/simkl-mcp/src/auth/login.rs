//! The subject resolver: `/authorize` sends the user to Simkl, and the Simkl
//! callback establishes who they are (their Simkl account ID) before
//! `oauth-as` issues the authorization code.
