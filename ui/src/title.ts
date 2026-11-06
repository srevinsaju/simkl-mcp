// ui://simkl/title: one title's details with its episodes by season, for
// get_title and get_episodes.
// Ticking an episode calls mark_watched (or unmark_watched) for that episode.

import type { App } from "@modelcontextprotocol/ext-apps";
import { connect, el, KIND_LABEL, poster } from "./host";
import type { EpisodeItem, GetEpisodesOutput, GetTitleOutput, LibraryItem, TitleCard } from "./output";

const STATUS_LABEL = {
  watching: "Watching",
  plantowatch: "Plan to watch",
  hold: "On hold",
  completed: "Completed",
  dropped: "Dropped",
} as const;

const NO_WRITE_ACCESS =
  "Claude can't change your Simkl library yet. Reconnect Simkl in Claude's connector settings and allow changes.";

const app = document.getElementById("app")!;
void connect<GetTitleOutput | GetEpisodesOutput>("simkl-title", render);

/** get_title loads every episode itself; get_episodes brings the page the model asked for. */
function render(host: App, output: GetTitleOutput | GetEpisodesOutput) {
  if ("episodes" in output) {
    const { page, page_count } = output;
    const meta = page_count > 1 ? `page ${page} of ${page_count}` : null;
    renderTitle(host, output.title, { meta, episodes: Promise.resolve(regular(output.episodes)) });
    return;
  }
  const { title, my_entry } = output;
  renderTitle(host, title, {
    overview: title.overview,
    network: title.network,
    entry: my_entry,
    episodes: title.kind === "movie" ? null : loadEpisodes(host, title),
  });
}

interface TitleParts {
  overview?: string | null;
  network?: string | null;
  entry?: LibraryItem;
  meta?: string | null;
  /** Null for movies. */
  episodes: Promise<EpisodeItem[]> | null;
}

function renderTitle(host: App, title: TitleCard, parts: TitleParts) {
  const { overview, network, entry, meta, episodes } = parts;
  const chips = el("div", { class: "chips" });
  if (entry) chips.append(el("span", { class: "chip status" }, STATUS_LABEL[entry.status]));
  if (entry?.user_rating) chips.append(el("span", { class: "chip" }, `★ ${entry.user_rating}`));
  const link = el("button", { class: "chip", type: "button" }, "simkl.com ↗");
  link.addEventListener("click", () => void host.openLink({ url: title.url }));
  chips.append(link);

  const sub = [title.en_title, title.year, network ?? KIND_LABEL[title.kind], meta].filter(Boolean).join(" · ");
  const status = el("p", { class: "error", role: "status" });
  const strip = el("div", { class: "view", ariaBusy: "true" });
  app.replaceChildren(
    el(
      "div",
      { class: "view" },
      el(
        "div",
        { class: "detail" },
        poster(title),
        el(
          "div",
          {},
          el("h1", {}, title.title),
          el("div", { class: "meta" }, sub),
          overview && el("p", { class: "overview" }, overview),
          chips,
        ),
      ),
      episodes && strip,
      status,
    ),
  );

  episodes
    ?.then((items) => {
      strip.ariaBusy = "false";
      strip.replaceChildren(...seasons(host, title, items, entry?.next_to_watch ?? null, status));
    })
    .catch(() => strip.replaceChildren(el("p", { class: "error" }, "Couldn't load the episodes.")));
}

/** Every page of get_episodes. */
async function loadEpisodes(host: App, title: TitleCard): Promise<EpisodeItem[]> {
  const items: EpisodeItem[] = [];
  for (let page = 1, pageCount = 1; page <= pageCount; page++) {
    const result = await host.callServerTool({
      name: "get_episodes",
      arguments: { title: { kind: title.kind, simkl_id: title.simkl_id }, page },
    });
    if (result.isError) throw new Error("get_episodes failed");
    const output = result.structuredContent as unknown as GetEpisodesOutput;
    pageCount = output.page_count;
    items.push(...output.episodes);
  }
  return regular(items);
}

/** Specials left out: they have no place in a season's strip. */
function regular(episodes: EpisodeItem[]): EpisodeItem[] {
  return episodes.filter((episode) => !episode.special && episode.episode != null);
}

function seasons(
  host: App,
  title: TitleCard,
  episodes: EpisodeItem[],
  nextToWatch: string | null,
  status: HTMLElement,
): HTMLElement[] {
  const bySeason = new Map<number | null, EpisodeItem[]>();
  for (const episode of episodes) {
    bySeason.set(episode.season, [...(bySeason.get(episode.season) ?? []), episode]);
  }
  return [...bySeason].map(([season, items]) => {
    const watched = items.filter((episode) => episode.watched).length;
    const counter = el("span", {}, `${watched} / ${items.length}`);
    const recount = () => {
      const pressed = strip.querySelectorAll('[aria-pressed="true"]').length;
      counter.textContent = `${pressed} / ${items.length}`;
    };
    const strip = el(
      "div",
      { class: "eps" },
      ...items.map((episode) => episodeButton(host, title, episode, nextToWatch, status, recount)),
    );
    return el(
      "div",
      { class: "season" },
      el("div", { class: "season-head" }, el("span", {}, season != null ? `Season ${season}` : "Episodes"), counter),
      strip,
    );
  });
}

function episodeButton(
  host: App,
  title: TitleCard,
  episode: EpisodeItem,
  nextToWatch: string | null,
  status: HTMLElement,
  recount: () => void,
): HTMLButtonElement {
  const number = episode.episode!;
  const code = episode.season != null ? `S${pad(episode.season)}E${pad(number)}` : `E${number}`;
  const button = el(
    "button",
    {
      class: code === nextToWatch ? "ep next" : "ep",
      type: "button",
      disabled: !episode.aired,
      title: [code, episode.title].filter(Boolean).join(" · "),
    },
    String(number),
  );
  button.setAttribute("aria-pressed", String(episode.watched));
  button.addEventListener("click", async () => {
    const watched = button.getAttribute("aria-pressed") === "true";
    button.ariaBusy = "true";
    status.textContent = "";
    try {
      const result = await host.callServerTool({
        name: watched ? "unmark_watched" : "mark_watched",
        arguments: {
          items: [
            {
              title: { kind: title.kind, simkl_id: title.simkl_id },
              // Anime episodes are numbered within season 1.
              episodes: [{ season: episode.season ?? 1, episode: number }],
            },
          ],
        },
      });
      if (result.isError) {
        const text = result.content.find((block) => block.type === "text");
        status.textContent = text?.type === "text" ? text.text : "Couldn't save that change.";
        return;
      }
      button.setAttribute("aria-pressed", String(!watched));
      button.classList.remove("next");
      recount();
    } catch {
      // The server answers a write without media:write with a step-up 403.
      status.textContent = NO_WRITE_ACCESS;
    } finally {
      button.ariaBusy = "false";
    }
  });
  return button;
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}
