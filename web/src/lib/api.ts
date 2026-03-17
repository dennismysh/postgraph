async function fetchApi<T>(path: string): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

export interface SubjectNode {
  id: string;
  label: string;
  post_count: number;
  avg_engagement: number;
  color: string;
}

export interface SubjectGraphEdge {
  source: string;
  target: string;
  weight: number;
  shared_intents: number;
}

export interface IntentInfo {
  id: string;
  name: string;
  color: string;
  post_count: number;
}

export interface SubjectGraphData {
  nodes: SubjectNode[];
  edges: SubjectGraphEdge[];
  intents: IntentInfo[];
}

export interface SubjectPost {
  id: string;
  text: string | null;
  intent: string;
  engagement: number;
  views: number;
  timestamp: string;
}

export interface SubjectPostsResponse {
  subject: string;
  posts: SubjectPost[];
}

export interface AnalyticsData {
  total_posts: number;
  analyzed_posts: number;
  total_subjects: number;
  total_intents: number;
  subjects: SubjectSummary[];
  engagement_over_time: EngagementPoint[];
}

export interface SubjectSummary {
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

export interface PostEngagementPoint {
  date: string;
  views: number;
  likes: number;
  replies: number;
  reposts: number;
  quotes: number;
}

export interface HeatmapDay {
  date: string;
  posts: number;
  likes: number;
  replies: number;
  reposts: number;
  views: number;
  media_types: Record<string, number>;
}

export interface HeatmapResponse {
  days: HeatmapDay[];
}

export interface HistogramBucket {
  bucket_min: number;
  bucket_max: number;
  label: string;
  count: number;
}

export interface HistogramResponse {
  engagement: HistogramBucket[];
  views: HistogramBucket[];
}

export const api = {
  getGraph: (intent?: string) => {
    const params = intent ? `?intent=${encodeURIComponent(intent)}` : '';
    return fetchApi<SubjectGraphData>(`/api/graph${params}`);
  },
  getSubjectPosts: (subjectId: string, intent?: string) => {
    const params = intent ? `?intent=${encodeURIComponent(intent)}` : '';
    return fetchApi<SubjectPostsResponse>(`/api/subjects/${subjectId}/posts${params}`);
  },
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
  getPostEngagement: (id: string) => fetchApi<PostEngagementPoint[]>(`/api/posts/${id}/engagement`),
  getViews: (since?: string, grouping?: string) => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    if (grouping) params.set('grouping', grouping);
    const qs = params.toString();
    return fetchApi<ViewsPoint[]>(`/api/analytics/views${qs ? `?${qs}` : ''}`);
  },
  getEngagement: (since?: string, grouping?: string) => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    if (grouping) params.set('grouping', grouping);
    const qs = params.toString();
    return fetchApi<EngagementPoint[]>(`/api/analytics/engagement${qs ? `?${qs}` : ''}`);
  },
  getHistograms: (since?: string) => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    const qs = params.toString();
    return fetchApi<HistogramResponse>(`/api/analytics/histograms${qs ? `?${qs}` : ''}`);
  },
  getHeatmap: (range?: string) => {
    const params = new URLSearchParams();
    if (range) params.set('range', range);
    const qs = params.toString();
    return fetchApi<HeatmapResponse>(`/api/analytics/heatmap${qs ? `?${qs}` : ''}`);
  },
};
