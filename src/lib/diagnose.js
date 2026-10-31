import { assess } from "./critical.js";
import { fmtBytes, fmtRate } from "./format.js";

const IGNORED = new Set(["system idle process", "idle"]);

/**
 * Answers "Why is my PC slow?" in plain language.
 * @returns {{ level: "good" | "busy" | "slow", headline: string, causes: string[], culprits: object[] }}
 */
export function diagnose(snap, groups) {
  const ram = (snap.memory_used / snap.memory_total) * 100;
  const disk = snap.disk_read + snap.disk_write;
  const cpu = snap.cpu;

  const causes = [];
  if (cpu > 85) causes.push("CPU đang quá tải nên mọi thao tác bị chậm lại.");
  else if (cpu > 60) causes.push("CPU đang khá bận.");
  if (ram > 90) causes.push("RAM gần đầy: máy phải dùng ổ đĩa thay cho RAM nên rất dễ bị giật, đơ.");
  else if (ram > 80) causes.push("RAM đã dùng hơn 80%, mở thêm ứng dụng sẽ làm máy chậm hẳn.");
  if (disk > 50 << 20) causes.push("Ổ đĩa đang đọc/ghi liên tục, các thao tác mở file hoặc ứng dụng sẽ bị trễ.");

  const level = cpu > 85 || ram > 92 ? "slow" : causes.length ? "busy" : "good";
  const headline = {
    good: "Máy đang chạy ổn, không có gì bất thường.",
    busy: "Máy đang hơi bận.",
    slow: "Máy đang bị quá tải.",
  }[level];

  // Weights: the more strained a resource is, the higher culprits using that resource score
  const wc = cpu > 60 ? 3 : 1;
  const wm = ram > 80 ? 3 : 1;
  const wd = disk > 30 << 20 ? 2 : 0.5;

  const culprits = groups
    .filter((g) => !IGNORED.has(g.name.toLowerCase()))
    .map((g) => {
      const cpuPart = (g.cpu / 100) * wc;
      const memPart = (g.memory / snap.memory_total) * wm;
      const diskPart = (g.disk / (50 << 20)) * wd;
      const score = cpuPart + memPart + diskPart;
      const top = Math.max(cpuPart, memPart, diskPart);
      let why;
      if (top === cpuPart) why = `đang chiếm ${g.cpu.toFixed(0)}% CPU — nó tính toán rất nhiều`;
      else if (top === memPart) why = `đang giữ ${fmtBytes(g.memory)} RAM (${((g.memory / snap.memory_total) * 100).toFixed(0)}% bộ nhớ máy)`;
      else why = `đang đọc/ghi đĩa ${fmtRate(g.disk)}`;

      const risk = g.procs.map(assess).find((a) => a.level);
      const tip = risk
        ? "Đây là thành phần hệ thống, đừng tắt nó."
        : top === memPart
          ? "Nếu không dùng, đóng ứng dụng này sẽ giải phóng RAM ngay."
          : "Thử bật Chế độ tiết kiệm để nó bớt giành tài nguyên, hoặc đóng nếu không cần.";
      return { key: g.key, name: g.name, pids: g.procs.map((p) => p.pid), score, why, tip, system: !!risk };
    })
    .filter((c) => c.score > 0.04)
    .sort((a, b) => b.score - a.score)
    .slice(0, 3);

  return { level, headline, causes, culprits };
}
