import type { SimklClient } from './client.js';

export function getCurrentUserSettings(client: SimklClient, token: string): Promise<any> {
  return client.request('/users/settings', { method: 'GET', token });
}

export function getUserStats(client: SimklClient, userId: number, token: string): Promise<any> {
  return client.request(`/users/${userId}/stats`, { method: 'GET', token });
}
