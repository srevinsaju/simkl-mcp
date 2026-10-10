//! Catalog types: `Kind` (movie, show, anime with `anime_type`),
//! titles, episodes, and `TitleRef`, a reference by any supported ID
//! (simkl, imdb, tmdb, tvdb, mal, anidb, anilist, kitsu, ...).
//!
//! Simkl names the same ID `simkl` or `simkl_id` depending on the endpoint;
//! that is normalised here. Every title exposes its simkl.com URL, which Simkl
//! requires wherever its data is shown.
//!
//! See [standard media objects](https://api.simkl.org/conventions/standard-media-objects).
