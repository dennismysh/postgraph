import { writable } from 'svelte/store';
import { MultiGraph } from 'graphology';
import type Graph from 'graphology';
import { api, type SubjectGraphData } from '$lib/api';

export const graphData = writable<SubjectGraphData | null>(null);
export const graphInstance = writable<Graph | null>(null);
export const selectedNode = writable<string | null>(null);
export const loading = writable(false);
export const error = writable<string | null>(null);

export async function loadGraph(intent?: string, timeRange?: string) {
  loading.set(true);
  error.set(null);
  try {
    const data = await api.getGraph(intent, timeRange);
    graphData.set(data);

    const graph = new MultiGraph();
    const visibleNodes = data.nodes.filter(n => n.post_count > 0);
    for (const node of visibleNodes) {
      const size = Math.log(node.post_count + 1) * 4 + 3;
      graph.addNode(node.id, {
        label: node.label,
        size,
        post_count: node.post_count,
        avg_engagement: node.avg_engagement,
        color: node.color,
        x: Math.random() * 100,
        y: Math.random() * 100,
      });
    }
    for (const edge of data.edges) {
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.addEdge(edge.source, edge.target, {
          weight: edge.weight,
          shared_intents: edge.shared_intents,
        });
      }
    }
    graphInstance.set(graph);
  } catch (e) {
    error.set(e instanceof Error ? e.message : 'Failed to load graph');
  } finally {
    loading.set(false);
  }
}
