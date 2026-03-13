async function fetchApi<T>(path: string): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

export interface GraphNode {
  id: string;
  label: string;
  size: number;
  sentiment: number | null;
  topics: string[];
  timestamp: string | null;
  engagement: number;
}

export interface GraphEdge {
  source: string;
  target: string;
  weight: number;
  edge_type: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface AnalyticsData {
  total_posts: number;
  analyzed_posts: number;
  total_topics: number;
  topics: TopicSummary[];
  engagement_over_time: EngagementPoint[];
}

export interface TopicSummary {
  name: string;
  post_count: number;
  avg_engagement: number;
}

export interface EngagementPoint {
  date: string;
  likes: number;
  replies: number;
  reposts: number;
}

export interface Post {
  id: string;
  text: string | null;
  timestamp: string;
  views: number;
  likes: number;
  replies_count: number;
  reposts: number;
  quotes: number;
  shares: number;
  sentiment: number | null;
}

export interface PostDetail {
  id: string;
  text: string | null;
  media_type: string | null;
  media_url: string | null;
  timestamp: string;
  permalink: string | null;
  views: number;
  likes: number;
  replies_count: number;
  reposts: number;
  quotes: number;
  shares: number;
  sentiment: number | null;
  topics: string[];
  engagement_rate: number;
}

export interface SyncResult {
  posts_synced: number;
  metrics_refreshed: number;
}

export interface SyncStartResult {
  started: boolean;
  message: string;
}

export interface SyncStatus {
  running: boolean;
  message: string;
  synced: number;
  total: number;
}

export interface ReanalyzeResult {
  started: boolean;
  message: string;
}

export interface AnalyzeStartResult {
  started: boolean;
  message: string;
}

export interface AnalyzeStatus {
  running: boolean;
  analyzed: number;
  total: number;
}

export interface ViewsPoint {
  date: string;
  views: number;
}

export interface TagGraphNode {
  id: string;
  label: string;
  post_count: number;
  total_engagement: number;
  post_ids: string[];
}

export interface TagGraphEdge {
  source: string;
  target: string;
  weight: number;
  shared_posts: number;
}

export interface TagGraphData {
  nodes: TagGraphNode[];
  edges: TagGraphEdge[];
}

export const api = {
  getGraph: () => fetchApi<GraphData>('/api/graph'),
  getTagGraph: () => fetchApi<TagGraphData>('/api/graph/tags'),
  getPost: (id: string) => fetchApi<PostDetail>(`/api/posts/${id}`),
  getPosts: () => fetchApi<Post[]>('/api/posts'),
  getAnalytics: () => fetchApi<AnalyticsData>('/api/analytics'),
  triggerSync: () => fetch('/api/sync', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Sync failed (${r.status})` }));
      throw new Error(body.error ?? `Sync failed (${r.status})`);
    }
    return r.json() as Promise<SyncStartResult>;
  }),
  getSyncStatus: () => fetchApi<SyncStatus>('/api/sync/status'),
  triggerReanalyze: () => fetch('/api/reanalyze', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Reanalyze failed (${r.status})` }));
      throw new Error(body.error ?? `Reanalyze failed (${r.status})`);
    }
    return r.json() as Promise<ReanalyzeResult>;
  }),
  startAnalyze: () => fetch('/api/analyze', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Analyze failed (${r.status})` }));
      throw new Error(body.error ?? `Analyze failed (${r.status})`);
    }
    return r.json() as Promise<AnalyzeStartResult>;
  }),
  getAnalyzeStatus: () => fetchApi<AnalyzeStatus>('/api/analyze/status'),
  getViews: (since?: string, grouping?: string) => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    if (grouping) params.set('grouping', grouping);
    const qs = params.toString();
    return fetchApi<ViewsPoint[]>(`/api/analytics/views${qs ? `?${qs}` : ''}`);
  },
};
