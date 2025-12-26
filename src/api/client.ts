// simkl api http client

export interface SimklClientOptions {
  baseUrl: string;
  clientId: string;
}

export interface RequestOptions {
  method: 'GET' | 'POST' | 'DELETE';
  token?: string;
  body?: unknown;
  query?: Record<string, string>;
}

export class SimklClient {
  private baseUrl: string;
  private clientId: string;

  constructor(options: SimklClientOptions) {
    this.baseUrl = options.baseUrl;
    this.clientId = options.clientId;
  }

  async request<T = unknown>(endpoint: string, options: RequestOptions): Promise<T> {
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
      return {} as T;
    }

    if (response.status === 302) {
      const location = response.headers.get('Location') || response.headers.get('location');
      return { redirectUrl: location } as T;
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
      return null as T;
    }

    try {
      return JSON.parse(responseText);
    } catch {
      throw new SimklApiError('simkl api error: invalid json response', response.status, responseText);
    }
  }

  private buildUrl(endpoint: string, query?: Record<string, string>): string {
    const path = endpoint.startsWith('/') ? endpoint : `/${endpoint}`;
    const url = new URL(path, this.baseUrl);

    url.searchParams.set('client_id', this.clientId);

    if (query) {
      Object.entries(query).forEach(([key, value]) => {
        url.searchParams.set(key, value);
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
