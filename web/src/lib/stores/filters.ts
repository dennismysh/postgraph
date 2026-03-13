import { writable, derived } from 'svelte/store';

export interface Filters {
  topics: string[];
  dateFrom: string | null;
  dateTo: string | null;
  minEngagement: number;
  edgeTypes: string[];
  searchQuery: string;
}

export const filters = writable<Filters>({
  topics: [],
  dateFrom: null,
  dateTo: null,
  minEngagement: 0,
  edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
  searchQuery: '',
});

export function resetFilters() {
  filters.set({
    topics: [],
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
    searchQuery: '',
  });
}
