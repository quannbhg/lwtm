/** Group processes sharing the same .exe into one "application" (like Task Manager). */
export function groupProcesses(snap, meta) {
  if (!snap) return [];
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
      };
      map.set(key, g);
    }
    const disk = p.disk_read + p.disk_write;
    g.procs.push({ ...p, disk });
    g.cpu += p.cpu;
    g.memory += p.memory;
    g.disk += disk;
  }
  return [...map.values()];
}
