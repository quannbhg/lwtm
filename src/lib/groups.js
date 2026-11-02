/**
 * Group processes sharing the same .exe into one "application" (like Task Manager).
 * `io` (optional) carries per-process GPU use and network rates: { gpu: { usage }, net: { rates } }.
 */
export function groupProcesses(snap, meta, io) {
  if (!snap) return [];
  const gpuUsage = io?.gpu?.usage ?? {};
  const netRates = io?.net?.rates ?? {};
  const map = new Map();
  for (const p of snap.processes) {
    const key = p.exe ? p.exe.toLowerCase() : "n:" + p.name.toLowerCase();
    let g = map.get(key);
    if (!g) {
      g = {
        key,
        exe: p.exe,
        name: meta[p.exe]?.description ?? p.name.replace(/\.exe$/i, ""),
        procs: [],
        cpu: 0,
        memory: 0,
        disk: 0,
        net: 0,
        gpu: 0,
      };
      map.set(key, g);
    }
    const disk = p.disk_read + p.disk_write;
    const rate = netRates[p.pid];
    const net = rate ? rate[0] + rate[1] : 0;
    const gpu = gpuUsage[p.pid] ?? 0;
    g.procs.push({ ...p, disk, net, gpu });
    g.cpu += p.cpu;
    g.memory += p.memory;
    g.disk += disk;
    g.net += net;
    g.gpu += gpu;
  }
  return [...map.values()].map((g) => ({ ...g, gpu: Math.min(100, g.gpu) }));
}
