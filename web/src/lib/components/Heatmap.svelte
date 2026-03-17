<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { HeatmapDay } from '$lib/api';

  interface Props {
    title: string;
    data: HeatmapDay[];
    range: string;
    valueExtractor: (day: HeatmapDay) => number;
    colorScale: string[];
    tooltipFormatter: (day: HeatmapDay) => string;
  }

  let { title, data, range, valueExtractor, colorScale, tooltipFormatter }: Props = $props();

  const CELL_SIZE = 11;
  const CELL_GAP = 2;
  const CELL_STEP = CELL_SIZE + CELL_GAP;
  const DAY_LABEL_WIDTH = 28;
  const MONTH_LABEL_HEIGHT = 16;
  const DAY_LABELS = ['', 'Mon', '', 'Wed', '', 'Fri', ''];
  const MONTH_NAMES = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

  let scrollContainer: HTMLDivElement = $state(null!);
  let tooltipText = $state('');
  let tooltipX = $state(0);
  let tooltipY = $state(0);
  let tooltipVisible = $state(false);

  interface CellData {
    date: string;
    value: number;
    day: HeatmapDay | null;
    col: number;
    row: number;
    color: string;
  }

  let cells: CellData[] = $state([]);
  let monthLabels: { text: string; x: number }[] = $state([]);
  let svgWidth = $state(0);
  let svgHeight = MONTH_LABEL_HEIGHT + 7 * CELL_STEP;

  function computeQuantiles(values: number[]): number[] {
    const nonZero = values.filter(v => v > 0).sort((a, b) => a - b);
    if (nonZero.length === 0) return [0, 0, 0, 0];
    const q = (p: number) => {
      const idx = Math.floor(p * (nonZero.length - 1));
      return nonZero[idx];
    };
    return [q(0.25), q(0.5), q(0.75), nonZero[nonZero.length - 1]];
  }

  function getColor(value: number, quantiles: number[]): string {
    if (value === 0) return colorScale[0];
    if (value <= quantiles[0]) return colorScale[1];
    if (value <= quantiles[1]) return colorScale[2];
    if (value <= quantiles[2]) return colorScale[3];
    return colorScale[4];
  }

  function buildGrid(data: HeatmapDay[]) {
    const lookup = new Map<string, HeatmapDay>();
    for (const day of data) {
      lookup.set(day.date, day);
    }

    // Build date range based on the selected range
    const today = new Date();
    today.setHours(0, 0, 0, 0);

    let rangeStart = new Date(today);
    switch (range) {
      case '3m': rangeStart.setDate(rangeStart.getDate() - 90); break;
      case '6m': rangeStart.setDate(rangeStart.getDate() - 180); break;
      case 'all': {
        if (data.length > 0) {
          const dates = data.map(d => new Date(d.date + 'T00:00:00'));
          rangeStart = new Date(Math.min(...dates.map(d => d.getTime())));
        } else {
          rangeStart.setFullYear(rangeStart.getFullYear() - 1);
        }
        break;
      }
      default: rangeStart.setFullYear(rangeStart.getFullYear() - 1); break; // 1y
    }
    // Align to Sunday
    rangeStart.setDate(rangeStart.getDate() - rangeStart.getDay());
    const startDate = rangeStart;

    // Generate all dates from startDate to today
    const allCells: CellData[] = [];
    const values: number[] = [];
    const current = new Date(startDate);
    let col = 0;

    while (current <= today) {
      const row = current.getDay(); // 0=Sun, 6=Sat
      const dateStr = current.toISOString().slice(0, 10);
      const dayData = lookup.get(dateStr) ?? null;
      const value = dayData ? valueExtractor(dayData) : 0;
      values.push(value);

      allCells.push({ date: dateStr, value, day: dayData, col, row, color: '' });

      // Move to next day
      current.setDate(current.getDate() + 1);
      if (current.getDay() === 0) col++; // New week on Sunday
    }

    // Assign colors
    const quantiles = computeQuantiles(values);
    for (const cell of allCells) {
      cell.color = getColor(cell.value, quantiles);
    }

    // Month labels (with minimum spacing to prevent overlap)
    const MIN_LABEL_GAP = 30;
    const labels: { text: string; x: number }[] = [];
    let lastMonth = -1;
    let lastLabelX = -MIN_LABEL_GAP;
    for (const cell of allCells) {
      const d = new Date(cell.date + 'T00:00:00');
      const month = d.getMonth();
      const x = DAY_LABEL_WIDTH + cell.col * CELL_STEP;
      if (month !== lastMonth && cell.row <= 1 && x - lastLabelX >= MIN_LABEL_GAP) {
        labels.push({ text: MONTH_NAMES[month], x });
        lastMonth = month;
        lastLabelX = x;
      }
    }

    cells = allCells;
    monthLabels = labels;
    svgWidth = DAY_LABEL_WIDTH + (col + 1) * CELL_STEP;
  }

  function handleMouseEnter(cell: CellData, event: MouseEvent) {
    if (!cell.day && cell.value === 0) {
      const d = new Date(cell.date + 'T00:00:00');
      tooltipText = `${d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })}: No activity`;
    } else if (cell.day) {
      tooltipText = tooltipFormatter(cell.day);
    } else {
      return;
    }
    const rect = (event.target as SVGElement).getBoundingClientRect();
    const container = scrollContainer.getBoundingClientRect();
    tooltipX = rect.left - container.left + rect.width / 2;
    tooltipY = rect.top - container.top - 8;
    tooltipVisible = true;
  }

  function handleMouseLeave() {
    tooltipVisible = false;
  }

  $effect(() => {
    buildGrid(data);
  });

  onMount(async () => {
    await tick();
    if (scrollContainer) {
      scrollContainer.scrollLeft = scrollContainer.scrollWidth;
    }
  });
</script>

<div class="heatmap-scroll" bind:this={scrollContainer}>
  <svg width={svgWidth} height={svgHeight} class="heatmap-svg">
    <!-- Month labels -->
    {#each monthLabels as label}
      <text x={label.x} y={10} class="month-label">{label.text}</text>
    {/each}

    <!-- Day-of-week labels -->
    {#each DAY_LABELS as label, i}
      {#if label}
        <text x={0} y={MONTH_LABEL_HEIGHT + i * CELL_STEP + CELL_SIZE - 1} class="day-label">{label}</text>
      {/if}
    {/each}

    <!-- Cells -->
    {#each cells as cell}
      <rect
        x={DAY_LABEL_WIDTH + cell.col * CELL_STEP}
        y={MONTH_LABEL_HEIGHT + cell.row * CELL_STEP}
        width={CELL_SIZE}
        height={CELL_SIZE}
        rx={2}
        ry={2}
        fill={cell.color}
        class="heatmap-cell"
        onmouseenter={(e) => handleMouseEnter(cell, e)}
        onmouseleave={handleMouseLeave}
      />
    {/each}
  </svg>

  {#if tooltipVisible}
    <div class="heatmap-tooltip" style="left: {tooltipX}px; top: {tooltipY}px;">
      {tooltipText}
    </div>
  {/if}
</div>

<style>
  .heatmap-scroll {
    overflow-x: auto;
    position: relative;
    padding-bottom: 4px;
  }
  .heatmap-svg {
    display: block;
  }
  .month-label {
    fill: #888;
    font-size: 10px;
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
  }
  .day-label {
    fill: #888;
    font-size: 10px;
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
  }
  .heatmap-cell {
    cursor: pointer;
    outline: none;
  }
  .heatmap-cell:hover {
    stroke: #fff;
    stroke-width: 1;
  }
  .heatmap-tooltip {
    position: absolute;
    background: #1a1a1a;
    border: 1px solid #444;
    color: #ddd;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 11px;
    white-space: nowrap;
    pointer-events: none;
    transform: translate(-50%, -100%);
    z-index: 10;
  }
</style>
