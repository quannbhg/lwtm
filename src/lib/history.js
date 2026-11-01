// Helpers for the load history recorded by the Rust watcher.
// A sample is { t, cpu, ram, disk, net, top: [{ n, c, m }] } (see src-tauri/src/watch.rs).

/** A gap longer than this between two samples means the app was not recording (closed / history off). */
export const GAP_SECS = 45;

/** Keep at most `maxPoints` points, taking the peak of each bucket so spikes are not smoothed away. */
export function downsample(samples, maxPoints = 500) {
  if (samples.length <= maxPoints) return samples;
  const size = Math.ceil(samples.length / maxPoints);
  const out = [];
  for (let i = 0; i < samples.length; i += size) {
    const bucket = samples.slice(i, i + size);
    const peak = bucket.reduce((a, b) => (b.cpu + b.ram > a.cpu + a.ram ? b : a));
    out.push(peak);
  }
  return out;
}

/**
 * Find the periods when the machine was overloaded.
 * @returns {{ start:number, end:number, kind:"cpu"|"ram"|"both", avgCpu:number, avgRam:number,
 *             culprits:{ name:string, cpu:number, mem:number }[] }[]} newest first
 */
export function findSlowPeriods(samples, { cpu = 85, ram = 90 } = {}) {
  const periods = [];
  let cur = null;
  const close = () => {
    if (cur && cur.items.length >= 2) periods.push(summarize(cur.items, cpu, ram));
    cur = null;
  };
  for (const s of samples) {
    const hot = s.cpu >= cpu || s.ram >= ram;
    if (!hot) {
      // tolerate a short dip so one calm sample does not split a long episode in two
      if (cur && s.t - cur.last > GAP_SECS) close();
      continue;
    }
    if (cur && s.t - cur.last > GAP_SECS) close();
    if (!cur) cur = { items: [], last: s.t };
    cur.items.push(s);
    cur.last = s.t;
  }
  close();
  return periods.reverse();
}

function summarize(items, cpuLimit, ramLimit) {
  const n = items.length;
  const avgCpu = items.reduce((a, s) => a + s.cpu, 0) / n;
  const avgRam = items.reduce((a, s) => a + s.ram, 0) / n;
  const cpuHot = avgCpu >= cpuLimit;
  const ramHot = avgRam >= ramLimit;
  const kind = cpuHot && ramHot ? "both" : ramHot ? "ram" : "cpu";

  const by = new Map();
  for (const s of items) {
    for (const t of s.top) {
      const e = by.get(t.n) ?? { name: t.n, cpuSum: 0, mem: 0 };
      e.cpuSum += t.c;
      e.mem = Math.max(e.mem, t.m);
      by.set(t.n, e);
    }
  }
  const culprits = [...by.values()]
    .map((e) => ({ name: e.name, cpu: e.cpuSum / n, mem: e.mem }))
    // rank by what actually caused the problem
    .sort((a, b) => (kind === "ram" ? b.mem - a.mem : b.cpu - a.cpu))
    .slice(0, 3);

  return { start: items[0].t, end: items[n - 1].t + 10, kind, avgCpu, avgRam, culprits };
}
