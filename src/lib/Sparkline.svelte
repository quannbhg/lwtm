<script>
  import { HISTORY } from "./store.svelte.js";

  /** data: array of values; max: null = auto-scale */
  let { data, max = null, color = "var(--accent)" } = $props();

  let top = $derived(max ?? Math.max(1, ...data) * 1.15);
  let points = $derived(
    data.map((v, i) => `${((i + HISTORY - data.length) / (HISTORY - 1)) * 100},${40 - (v / top) * 40}`),
  );
  let line = $derived(points.join(" "));
  let area = $derived(points.length ? `${points[0].split(",")[0]},40 ${line} 100,40` : "");
</script>

<svg viewBox="0 0 100 40" preserveAspectRatio="none" class="spark">
  <polygon points={area} fill={color} opacity="0.18" />
  <polyline points={line} fill="none" stroke={color} stroke-width="1.2" vector-effect="non-scaling-stroke" />
</svg>

<style>
  .spark { width: 100%; height: 100%; display: block; background: color-mix(in srgb, var(--accent) 4%, var(--panel)); border: 1px solid var(--line); border-radius: 6px; }
</style>
