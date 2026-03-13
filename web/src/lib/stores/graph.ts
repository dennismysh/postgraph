import { writable } from 'svelte/store';
import { MultiGraph } from 'graphology';
import type Graph from 'graphology';
import { api, type GraphData } from '$lib/api';

export const graphData = writable<GraphData | null>(null);
export const graphInstance = writable<Graph | null>(null);
export const selectedNode = writable<string | null>(null);
export const loading = writable(false);
export const error = writable<string | null>(null);

export async function loadGraph() {
  loading.set(true);
  error.set(null);
  try {
    const data = await api.getGraph();
    graphData.set(data);

    const graph = new MultiGraph();
    for (const node of data.nodes) {
      graph.addNode(node.id, {
        label: node.label,
        size: node.size,
        sentiment: node.sentiment,
        topics: node.topics,
        timestamp: node.timestamp,
        engagement: node.engagement,
        x: Math.random() * 100,
        y: Math.random() * 100,
      });
    }
    for (const edge of data.edges) {
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.addEdge(edge.source, edge.target, {
          weight: edge.weight,
          edge_type: edge.edge_type,
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
