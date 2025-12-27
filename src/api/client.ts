// simkl api http client

export interface SimklClientOptions {
  baseUrl: string;
  clientId: string;
}

export interface RequestOptions {
  method: 'GET' | 'POST' | 'DELETE';
  token?: string;
  body?: unknown;
  query?: Record<string, string | number | undefined>;
}

export interface PaginationOptions {
  page?: number;
  limit?: number;
  fetchAllPages?: boolean;
  maxPages?: number;
}

export interface PaginationInfo {
  page: number;
  limit: number;
  pageCount: number;
  itemCount: number;
}

export interface PaginatedResult<T> {
  data: T;
  pagination: PaginationInfo | null;
}

export class SimklClient {
  private baseUrl: string;
  private clientId: string;

  constructor(options: SimklClientOptions) {
    this.baseUrl = options.baseUrl;
    this.clientId = options.clientId;
  }

  async request<T = unknown>(endpoint: string, options: RequestOptions): Promise<T> {
    const { data } = await this.doRequest<T>(endpoint, options);
    return data;
  }

  async requestPaginated<T = unknown>(
    endpoint: string,
    options: RequestOptions & { pagination?: PaginationOptions }
  ): Promise<PaginatedResult<T>> {
    const { pagination: paginationOptions, ...baseOptions } = options;
    const baseQuery: Record<string, string | number | undefined> = {
      ...baseOptions.query,
    };

    if (paginationOptions?.page !== undefined) {
      baseQuery.page = paginationOptions.page;
    }

    if (paginationOptions?.limit !== undefined) {
      baseQuery.limit = paginationOptions.limit;
    }

    const firstPage = await this.doRequest<T>(endpoint, { ...baseOptions, query: baseQuery });
    const firstPagination = firstPage.pagination;

    if (!paginationOptions?.fetchAllPages || !firstPagination) {
      return firstPage;
    }

    if (!Array.isArray(firstPage.data)) {
      throw new SimklApiError('simkl api error: fetchAllPages requires array response', 0, null);
    }

    const items = [...firstPage.data];
    const maxPages = paginationOptions.maxPages ?? firstPagination.pageCount;
    const lastPage = Math.min(maxPages, firstPagination.pageCount);

    for (let page = firstPagination.page + 1; page <= lastPage; page++) {
      const query = { ...baseQuery, page };
      const nextPage = await this.doRequest<T>(endpoint, { ...baseOptions, query });
      if (!Array.isArray(nextPage.data)) {
        throw new SimklApiError('simkl api error: fetchAllPages requires array response', 0, null);
      }
      items.push(...nextPage.data);
    }

    return {
      data: items as unknown as T,
      pagination: {
        ...firstPagination,
        pageCount: lastPage,
      },
    };
  }

  private async doRequest<T = unknown>(endpoint: string, options: RequestOptions): Promise<PaginatedResult<T>> {
    const url = this.buildUrl(endpoint, options.query);
    const headers = this.buildHeaders(options.token);

    const init: RequestInit = {
      method: options.method,
      headers,
    };

    if (options.body && options.method !== 'GET') {
      init.body = JSON.stringify(options.body);
    }

    let response: Response;
    try {
      response = await fetch(url, init);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'unknown fetch error';
      throw new SimklApiError(`simkl api request failed: ${message}`, 0, null);
    }

    if (response.status === 204) {
      return { data: {} as T, pagination: null };
    }

    if (response.status === 302) {
      const location = response.headers.get('Location') || response.headers.get('location');
      return { data: { redirectUrl: location } as T, pagination: null };
    }

    const responseText = await this.readResponseBody(response);

    if (!response.ok) {
      throw new SimklApiError(
        `simkl api error: ${response.status} ${response.statusText}`,
        response.status,
        responseText
      );
    }

    if (!responseText) {
      return { data: null as T, pagination: this.parsePaginationInfo(response) };
    }

    try {
      const data = JSON.parse(responseText);
      return {
        data,
        pagination: this.parsePaginationInfo(response),
      };
    } catch {
      throw new SimklApiError('simkl api error: invalid json response', response.status, responseText);
    }
  }

  private buildUrl(endpoint: string, query?: Record<string, string | number | undefined>): string {
    const path = endpoint.startsWith('/') ? endpoint : `/${endpoint}`;
    const url = new URL(path, this.baseUrl);

    url.searchParams.set('client_id', this.clientId);

    if (query) {
      Object.entries(query).forEach(([key, value]) => {
        if (value === undefined) return;
        url.searchParams.set(key, String(value));
      });
    }

    return url.toString();
  }

  private buildHeaders(token?: string): Record<string, string> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };

    if (token) {
      headers['Authorization'] = `Bearer ${token}`;
    }

    return headers;
  }

  private async readResponseBody(response: Response, limit = 500_000): Promise<string> {
    const reader = response.body?.getReader();
    if (!reader) {
      const text = await response.text();
      if (text.length > limit) {
        throw new SimklApiError('simkl api error: response too large', response.status, text.slice(0, 1024));
      }
      return text;
    }

    const decoder = new TextDecoder();
    let result = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      result += decoder.decode(value, { stream: true });
      if (result.length > limit) {
        reader.cancel().catch(() => {});
        throw new SimklApiError('simkl api error: response too large', response.status, result.slice(0, 1024));
      }
    }

    result += decoder.decode();
    return result;
  }

  private parsePaginationInfo(response: Response): PaginationInfo | null {
    const page = this.parsePaginationHeader(response, 'X-Pagination-Page');
    const limit = this.parsePaginationHeader(response, 'X-Pagination-Limit');
    const pageCount = this.parsePaginationHeader(response, 'X-Pagination-Page-Count');
    const itemCount = this.parsePaginationHeader(response, 'X-Pagination-Item-Count');

    if (
      page === null ||
      limit === null ||
      pageCount === null ||
      itemCount === null
    ) {
      return null;
    }

    return { page, limit, pageCount, itemCount };
  }

  private parsePaginationHeader(response: Response, name: string): number | null {
    const value = response.headers.get(name) || response.headers.get(name.toLowerCase());
    if (!value) return null;
    const parsed = Number.parseInt(value, 10);
    return Number.isNaN(parsed) ? null : parsed;
  }
}

export class SimklApiError extends Error {
  constructor(
    message: string,
    public statusCode: number,
    public responseBody: string | null
  ) {
    super(message);
    this.name = 'SimklApiError';
  }
}
