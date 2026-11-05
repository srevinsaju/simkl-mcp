//! Small macros: a map literal, and the declarations that keep endpoints and
//! `Kind`'s spellings in one place each.

/// A `BTreeMap` literal: `btreemap! { "a" => 1, "b" => 2 }`.
macro_rules! btreemap {
    ($($key:expr => $value:expr),* $(,)?) => {
        ::std::collections::BTreeMap::from([$(($key, $value)),*])
    };
}

/// Implements `Endpoint`: the access rules, response type and path, plus a
/// custom `parse` when the response isn't plain JSON.
///
/// ```ignore
/// endpoint!(GetEpisodes: Catalog => Vec<Episode>, path = |self| format!("/tv/episodes/{}", self.id));
/// endpoint!(Search: Read => Page<SearchResult>, path = |self| ..., parse = |_, reply| Page::from_reply(reply));
/// ```
macro_rules! endpoint {
    (
        $ty:ty: $access:ident => $response:ty,
        path = |$this:ident| $path:expr
        $(, parse = $parse:expr)? $(,)?
    ) => {
        impl $crate::client::Endpoint for $ty {
            type Access = $crate::client::$access;
            type Response = $response;

            fn path(&$this) -> ::std::string::String {
                ($path).into()
            }

            fn parse(&$this, reply: $crate::client::Reply) -> $crate::error::Result<$response> {
                endpoint!(@parse $this, reply $(, $parse)?)
            }
        }
    };
    (@parse $this:ident, $reply:ident) => {
        $reply.json()
    };
    (@parse $this:ident, $reply:ident, $parse:expr) => {
        ($parse)($this, $reply)
    };
}

/// One method per vocabulary Simkl uses for kinds, one row per vocabulary:
/// `method: movie, show, anime;`.
macro_rules! kind_names {
    ($($(#[$doc:meta])* $method:ident: $movie:literal, $show:literal, $anime:literal;)+) => {
        impl $crate::media::Kind {
            $(
                $(#[$doc])*
                pub fn $method(self) -> &'static str {
                    match self {
                        Self::Movie => $movie,
                        Self::Show => $show,
                        Self::Anime => $anime,
                    }
                }
            )+
        }
    };
}
