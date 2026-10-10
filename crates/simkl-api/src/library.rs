//! The user's library: watchlist statuses, entries with progress and
//! `next_to_watch`, ratings, and the `Target` of a history write (whole title,
//! season, or episodes).
//!
//! Movies cannot be `watching` or `hold`; the types reject that rather than
//! relying on Simkl's silent upgrade to `completed`
//! ([statuses](https://api.simkl.org/conventions/list-statuses)).

#[cfg(test)]
#[path = "library_test.rs"]
mod tests;
