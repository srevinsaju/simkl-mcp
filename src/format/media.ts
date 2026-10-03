// plain-text formatters for simkl media items returned by list endpoints

export interface MediaItem {
  title?: string;
  year?: number;
  date?: string;
  ids?: { simkl_id?: number; simkl?: number; imdb?: string };
  ratings?: { simkl?: { rating?: number } };
  episode?: { episode?: number };
}

export type ItemFormatter = (item: MediaItem, index: number) => string;

const DEFAULT_LIMIT = 20;

// `[SIMKL #1] - Dune (2021)`
export const formatItem: ItemFormatter = (item, index) =>
  `${index}: [SIMKL #${simklId(item)}] - ${item.title} (${item.year || 'N/A'})`;

export const withRating: ItemFormatter = (item, index) =>
  `${formatItem(item, index)} - rating: ${item.ratings?.simkl?.rating || 'N/A'}`;

export const withImdb: ItemFormatter = (item, index) =>
  `${formatItem(item, index)} - imdb:${item.ids?.imdb || 'N/A'}`;

// airing entries describe an episode slot, so the year is replaced by episode and date
export const asAiring: ItemFormatter = (item, index) =>
  `${index}: [SIMKL #${simklId(item)}] - ${item.title} - ep ${item.episode?.episode || '?'} at ${item.date || 'TBA'}`;

export function formatList(
  items: MediaItem[],
  format: ItemFormatter = formatItem,
  limit = DEFAULT_LIMIT
): string[] {
  return items.slice(0, limit).map(format);
}

function simklId(item: MediaItem) {
  return item.ids?.simkl_id || item.ids?.simkl;
}
