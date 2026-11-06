// ui://simkl/titles: the poster grid shared by search, trending, calendar,
// get_library and up_next. Each card opens the title on simkl.com.

import type { App } from "@modelcontextprotocol/ext-apps";
import { connect, el, KIND_LABEL, poster } from "./host";
import type { Kind, TitleCard, TitlesOutput } from "./output";

interface Tile {
  card: TitleCard;
  /** Top-left: a trending rank. */
  rank?: number;
  /** Bottom-left: the next episode, or an air date. */
  pill?: string;
  /** 0 to 1. */
  progress?: number;
}

interface Grid {
  heading: string;
  meta: string;
  tiles: Tile[];
}

const app = document.getElementById("app")!;
void connect<TitlesOutput>("simkl-titles", render);

function render(host: App, output: TitlesOutput) {
  const grid = toGrid(output);
  const kinds = [...new Set(grid.tiles.map((tile) => tile.card.kind))];
  const list = el("div", { class: "grid" });
  const show = (kind: Kind | null) => {
    const tiles = grid.tiles.filter((tile) => kind === null || tile.card.kind === kind);
    list.replaceChildren(...tiles.map((tile) => card(host, tile)));
  };

  const head = el("div", { class: "head" }, el("h1", {}, grid.heading), el("span", { class: "meta" }, grid.meta));
  if (kinds.length > 1) head.append(filter(kinds, show));
  show(null);

  app.replaceChildren(
    el(
      "div",
      { class: "view" },
      head,
      grid.tiles.length ? list : el("p", { class: "empty" }, "Nothing here."),
    ),
  );
}

/** Each tool's output as a grid. Told apart by the fields only that tool returns. */
function toGrid(output: TitlesOutput): Grid {
  if ("results" in output) {
    return {
      heading: `Results for “${output.query}”`,
      meta: count(output.results.length),
      tiles: output.results.map((card) => ({ card })),
    };
  }
  if ("attribution" in output) {
    const { kind, period, genre, items } = output;
    return {
      heading: `Trending ${genre ? `${genre} ` : ""}${KIND_LABEL[kind].toLowerCase()}`,
      meta: period,
      tiles: items.map(({ rank, ...card }) => ({ card, rank })),
    };
  }
  if ("page_count" in output) {
    return {
      heading: "Your library",
      meta: `${count(output.total)} · page ${output.page} of ${output.page_count}`,
      tiles: output.items.map((item) => ({
        card: item.title,
        pill: item.next_to_watch ?? undefined,
        progress:
          item.watched_episodes != null && item.total_episodes
            ? item.watched_episodes / item.total_episodes
            : undefined,
      })),
    };
  }
  if ("total" in output) {
    return {
      heading: "Airing soon",
      meta: count(output.total),
      tiles: output.items.map((item) => ({
        card: item.title,
        pill: `${shortDate(item.date)}${item.episode != null ? ` · ${episodeCode(item.season, item.episode)}` : ""}`,
      })),
    };
  }
  return {
    heading: "Up next",
    meta: count(output.items.length),
    tiles: output.items.map((item) => ({
      card: item.title,
      pill: item.next_episode?.episode != null
        ? episodeCode(item.next_episode.season, item.next_episode.episode)
        : undefined,
      progress: item.paused ? item.paused.progress / 100 : undefined,
    })),
  };
}

function card(host: App, tile: Tile): HTMLElement {
  const { card, rank, pill, progress } = tile;
  const overlay: Node[] = [];
  if (rank != null) overlay.push(el("span", { class: "pill rank" }, `#${rank}`));
  if (pill) overlay.push(el("span", { class: "pill" }, pill));
  if (progress != null) {
    const fill = el("i");
    fill.style.width = `${Math.round(Math.min(progress, 1) * 100)}%`;
    overlay.push(el("span", { class: "bar" }, fill));
  }
  const sub = [KIND_LABEL[card.kind], card.year].filter(Boolean).join(" · ");
  const button = el(
    "button",
    { class: "card", type: "button", title: card.en_title ?? card.title },
    poster(card, ...overlay),
    el("span", { class: "name" }, card.title),
    el("span", { class: "sub" }, sub),
  );
  button.addEventListener("click", () => void host.openLink({ url: card.url }));
  return button;
}

function filter(kinds: Kind[], show: (kind: Kind | null) => void): HTMLElement {
  const seg = el("div", { class: "seg", role: "group" });
  const options: [string, Kind | null][] = [["All", null], ...kinds.map((kind): [string, Kind] => [KIND_LABEL[kind], kind])];
  const buttons = options.map(([label, kind]) => {
    const button = el("button", { type: "button" }, label);
    button.setAttribute("aria-pressed", String(kind === null));
    button.addEventListener("click", () => {
      for (const other of buttons) other.setAttribute("aria-pressed", String(other === button));
      show(kind);
    });
    return button;
  });
  seg.append(...buttons);
  return seg;
}

function count(n: number): string {
  return `${n} ${n === 1 ? "title" : "titles"}`;
}

function episodeCode(season: number | null, episode: number): string {
  return season != null ? `S${pad(season)}E${pad(episode)}` : `E${episode}`;
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function shortDate(date: string): string {
  return new Date(date).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
