import { writable, derived } from 'svelte/store';

export interface Filters {
  topics: string[];
  category: string | null;
  dateFrom: string | null;
  dateTo: string | null;
  minEngagement: number;
  edgeTypes: string[];
  searchQuery: string;
}

export const filters = writable<Filters>({
  topics: [],
  category: null,
  dateFrom: null,
  dateTo: null,
  minEngagement: 0,
  edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
  searchQuery: '',
});

export function resetFilters() {
  filters.set({
    topics: [],
    category: null,
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
    searchQuery: '',
  });
}
