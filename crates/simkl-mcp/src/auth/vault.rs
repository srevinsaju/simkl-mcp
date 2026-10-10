//! Each grant's Simkl access and refresh tokens, encrypted at rest and keyed
//! by grant. Refreshes the Simkl token before its 7-day expiry, coalescing
//! concurrent refreshes, and deletes it when the grant is revoked.
