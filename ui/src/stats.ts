// ui://simkl/stats: get_stats as time watched per kind, then each kind's
// titles by status as one stacked bar.

import { connect, el } from "./host";
import type { KindStats, Status, StatsOutput } from "./output";

/** Bar order: from done to not started, dropped last. */
const STATUSES: [Status, string][] = [
  ["completed", "Completed"],
  ["watching", "Watching"],
  ["hold", "On hold"],
  ["plantowatch", "Plan to watch"],
  ["dropped", "Dropped"],
];

const KINDS: ["movies" | "shows" | "anime", string][] = [
  ["shows", "TV"],
  ["anime", "Anime"],
  ["movies", "Movies"],
];

const app = document.getElementById("app")!;
void connect<StatsOutput>("simkl-stats", (_host, output) => render(output));

function render(output: StatsOutput) {
  const meta = output.last_week_minutes != null ? `${duration(output.last_week_minutes)} last week` : "";
  app.replaceChildren(
    el(
      "div",
      { class: "view" },
      el(
        "div",
        { class: "head" },
        el("h1", {}, `${duration(output.total_minutes)} watched`),
        el("span", { class: "meta" }, meta),
      ),
      el("div", { class: "totals" }, ...KINDS.map(([key, label]) => total(label, output[key]))),
      ...KINDS.map(([key, label]) => breakdown(label, output[key])).filter((node) => node !== null),
    ),
  );
}

function total(label: string, stats: KindStats): HTMLElement {
  const titles = Object.values(stats.by_status).reduce((sum, item) => sum + item.count, 0);
  return el(
    "div",
    { class: "total" },
    el("span", { class: "meta" }, label),
    el("strong", {}, duration(stats.total_minutes)),
    el("span", { class: "meta" }, `${titles} ${titles === 1 ? "title" : "titles"}`),
  );
}

/** One kind's titles by status. Null when the kind has none. */
function breakdown(label: string, stats: KindStats): HTMLElement | null {
  const present = STATUSES.filter(([status]) => (stats.by_status[status]?.count ?? 0) > 0);
  const titles = present.reduce((sum, [status]) => sum + stats.by_status[status]!.count, 0);
  if (titles === 0) return null;

  const bar = el("div", { class: "stack", role: "img" });
  bar.ariaLabel = present.map(([status, name]) => `${name}: ${stats.by_status[status]!.count}`).join(", ");
  for (const [status] of present) {
    const segment = el("i", { class: status });
    segment.style.flexGrow = String(stats.by_status[status]!.count);
    bar.append(segment);
  }
  const legend = el(
    "ul",
    { class: "legend" },
    ...present.map(([status, name]) =>
      el("li", {}, el("i", { class: status }), `${name} `, el("b", {}, String(stats.by_status[status]!.count))),
    ),
  );
  const watching = stats.by_status.watching;
  const left =
    watching?.episodes_left != null
      ? `${watching.episodes_left} episodes left in what you're watching` +
        (watching.minutes_left != null ? `, about ${duration(watching.minutes_left)}` : "")
      : null;
  return el(
    "section",
    { class: "kind" },
    el("div", { class: "season-head" }, el("span", {}, label), el("span", {}, `${titles} titles`)),
    bar,
    legend,
    left && el("p", { class: "meta" }, left),
  );
}

/** `45 min`, `12 h`, `3 d 4 h`. */
function duration(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} h`;
  const days = Math.floor(hours / 24);
  const rest = hours % 24;
  return rest ? `${days} d ${rest} h` : `${days} d`;
}
