import { writable } from 'svelte/store';

export interface Filters {
  intent: string | null;
  timeRange: string;
  minEngagement: number;
  searchQuery: string;
}

export const filters = writable<Filters>({
  intent: null,
  timeRange: '30d',
  minEngagement: 0,
  searchQuery: '',
});

export function resetFilters() {
  filters.set({
    intent: null,
    timeRange: '30d',
    minEngagement: 0,
    searchQuery: '',
  });
}
