import { writable } from 'svelte/store';
import { MultiGraph } from 'graphology';
import type Graph from 'graphology';
import { api, type TagGraphData } from '$lib/api';

export const tagGraphData = writable<TagGraphData | null>(null);
export const tagGraphInstance = writable<Graph | null>(null);
export const selectedTagNode = writable<string | null>(null);
export const tagLoading = writable(false);
export const tagError = writable<string | null>(null);

export async function loadTagGraph() {
  tagLoading.set(true);
  tagError.set(null);
  try {
    const data = await api.getTagGraph();
    tagGraphData.set(data);

    const graph = new MultiGraph();
    for (const node of data.nodes) {
      const size = Math.log(node.post_count + 1) * 3 + 2;
      graph.addNode(node.id, {
        label: node.label,
        size,
        post_count: node.post_count,
        total_engagement: node.total_engagement,
        post_ids: node.post_ids,
        category_name: node.category_name,
        category_color: node.category_color,
        x: Math.random() * 100,
        y: Math.random() * 100,
      });
    }
    for (const edge of data.edges) {
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.addEdge(edge.source, edge.target, {
          weight: edge.weight,
          shared_posts: edge.shared_posts,
        });
      }
    }
    tagGraphInstance.set(graph);
  } catch (e) {
    tagError.set(e instanceof Error ? e.message : 'Failed to load tag graph');
  } finally {
    tagLoading.set(false);
  }
}
