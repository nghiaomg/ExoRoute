<script lang="ts">
  /**
   * Trend accent for a KPI tile.
   *
   * A sparkline inside a stat tile is the one form that carries no hover layer:
   * the tile's own figure is the value a reader takes away, and this only shows
   * its shape over the period. The spoken label therefore describes the trend,
   * and the exact numbers stay available in the traffic chart's table view.
   */
  export let values: number[];
  export let label: string;

  const HEIGHT = 28;
  const INSET = 3;

  let width = 132;

  // One reactive build rather than several chained statements, so the geometry
  // can never be computed against a stale scale.
  $: geometry = buildGeometry(values, width);

  function buildGeometry(values: number[], width: number): { points: string; lastX: number; lastY: number } {
    const max = Math.max(1, ...values);
    const x = (index: number): number =>
      values.length > 1 ? (index / (values.length - 1)) * width : width / 2;
    const y = (value: number): number =>
      HEIGHT - INSET - (value / max) * (HEIGHT - INSET * 2);
    return {
      points: values.map((value, index) => `${x(index).toFixed(2)},${y(value).toFixed(2)}`).join(' '),
      lastX: x(values.length - 1),
      lastY: y(values[values.length - 1] ?? 0),
    };
  }
</script>

{#if values.length > 1}
  <div class="sparkline" bind:clientWidth={width}>
    <svg viewBox={`0 0 ${width} ${HEIGHT}`} preserveAspectRatio="none" role="img" aria-label={label}>
      <polyline
        points={geometry.points}
        fill="none"
        stroke="var(--chart-series)"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
      />
      <!-- Endpoint marker with a surface ring, so the latest bucket reads as "now". -->
      <circle
        cx={geometry.lastX}
        cy={geometry.lastY}
        r="3"
        fill="var(--chart-series)"
        stroke="var(--paper)"
        stroke-width="2"
        vector-effect="non-scaling-stroke"
      />
    </svg>
  </div>
{/if}

<style>
  .sparkline { width: 100%; height: 28px; margin-top: 6px; }
  .sparkline svg { display: block; width: 100%; height: 100%; overflow: visible; }
</style>
