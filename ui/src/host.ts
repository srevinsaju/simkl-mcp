// The connection to the MCP Apps host: theme, style variables and fonts from
// the host, and the tool result that the view renders.

import {
  App,
  applyDocumentTheme,
  applyHostFonts,
  applyHostStyleVariables,
  type McpUiHostContext,
} from "@modelcontextprotocol/ext-apps";
import "./style.css";

/** Connects to the host and calls `render` with each tool result. */
export async function connect<T>(
  name: string,
  render: (app: App, output: T) => void,
): Promise<App> {
  const app = new App({ name, version: "1.0.0" });
  app.onhostcontextchanged = applyContext;
  app.ontoolresult = ({ structuredContent, isError }) => {
    if (isError || !structuredContent) return;
    render(app, structuredContent as T);
  };
  await app.connect();
  applyContext(app.getHostContext() ?? {});
  return app;
}

function applyContext(context: McpUiHostContext) {
  if (context.theme) applyDocumentTheme(context.theme);
  if (context.styles?.variables) applyHostStyleVariables(context.styles.variables);
  if (context.styles?.css?.fonts) applyHostFonts(context.styles.css.fonts);
}

/** Builds an element. Children are text or elements; text is never parsed as HTML. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { class?: string } = {},
  ...children: (Node | string | null | undefined | false)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  const { class: className, ...rest } = props;
  if (className) node.className = className;
  Object.assign(node, rest);
  for (const child of children) {
    if (child) node.append(child);
  }
  return node;
}

/** A poster, or a plain tile when Simkl has none. */
export function poster(card: { poster?: string; title: string }, ...overlay: Node[]): HTMLElement {
  const frame = el("div", { class: "poster" });
  if (card.poster) {
    frame.append(el("img", { src: card.poster, alt: "", loading: "lazy" }));
  } else {
    frame.append(el("span", { class: "poster-fallback" }, card.title));
  }
  frame.append(...overlay);
  return frame;
}

export const KIND_LABEL = { movie: "Movie", show: "TV", anime: "Anime" } as const;
