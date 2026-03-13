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
  likes: number;
  replies_count: number;
  reposts: number;
  quotes: number;
  sentiment: number | null;
}

export interface SyncResult {
  posts_synced: number;
  posts_analyzed: number;
  edges_computed: number;
}

export const api = {
  getGraph: () => fetchApi<GraphData>('/api/graph'),
  getPosts: () => fetchApi<Post[]>('/api/posts'),
  getAnalytics: () => fetchApi<AnalyticsData>('/api/analytics'),
  triggerSync: () => fetch('/api/sync', {
    method: 'POST',
  }).then(r => r.json() as Promise<SyncResult>),
};
