// The structuredContent of the tools that show a view. Mirrors the Rust
// output structs in crates/simkl-mcp/src/tools (catalog.rs, library.rs,
// title.rs); their outputSchema is authoritative.

export type Kind = "movie" | "show" | "anime";

export type Status = "watching" | "plantowatch" | "hold" | "completed" | "dropped";

export interface TitleCard {
  kind: Kind;
  title: string;
  en_title?: string;
  year?: number;
  simkl_id: number;
  url: string;
  anime_type?: string;
  poster?: string;
}

export interface LibraryItem {
  title: TitleCard;
  status: Status;
  user_rating: number | null;
  watched_episodes: number | null;
  total_episodes: number | null;
  next_to_watch: string | null;
}

export interface Page {
  page: number;
  page_count: number;
  total: number;
}

export interface SearchOutput {
  query: string;
  results: TitleCard[];
}

export interface TrendingOutput {
  attribution: string;
  kind: Kind;
  period: string;
  genre?: string | null;
  items: (TitleCard & { rank: number })[];
}

export interface CalendarOutput {
  items: {
    date: string;
    title: TitleCard;
    season: number | null;
    episode: number | null;
    episode_title: string | null;
  }[];
  total: number;
}

export interface GetLibraryOutput extends Page {
  items: LibraryItem[];
}

export interface UpNextOutput {
  items: {
    title: TitleCard;
    next_episode?: { season: number | null; episode: number | null; title: string | null };
    paused?: { progress: number; season: number | null; episode: number | null };
  }[];
}

export type TitlesOutput =
  | SearchOutput
  | TrendingOutput
  | CalendarOutput
  | GetLibraryOutput
  | UpNextOutput;

export interface GetTitleOutput {
  title: TitleCard & {
    overview: string | null;
    genres: string[];
    runtime: number | null;
    network: string | null;
    status: string | null;
    total_episodes: number | null;
    rating: number | null;
  };
  related: TitleCard[];
  my_entry?: LibraryItem;
}

export interface EpisodeItem {
  season: number | null;
  episode: number | null;
  title: string | null;
  special: boolean;
  aired: boolean;
  watched: boolean;
}

export interface GetEpisodesOutput extends Page {
  title: TitleCard;
  episodes: EpisodeItem[];
}

export interface StatusStats {
  count: number;
  minutes?: number;
  watched_episodes?: number;
  total_episodes?: number;
  episodes_left?: number;
  minutes_left?: number;
}

export interface KindStats {
  total_minutes: number;
  by_status: Partial<Record<Status, StatusStats>>;
}

export interface StatsOutput {
  total_minutes: number;
  movies: KindStats;
  shows: KindStats;
  anime: KindStats;
  last_week_minutes?: number;
}
