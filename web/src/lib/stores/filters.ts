import { writable } from 'svelte/store';

export interface Filters {
  intent: string | null;
  dateFrom: string | null;
  dateTo: string | null;
  minEngagement: number;
  searchQuery: string;
}

export const filters = writable<Filters>({
  intent: null,
  dateFrom: null,
  dateTo: null,
  minEngagement: 0,
  searchQuery: '',
});

export function resetFilters() {
  filters.set({
    intent: null,
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    searchQuery: '',
  });
}
