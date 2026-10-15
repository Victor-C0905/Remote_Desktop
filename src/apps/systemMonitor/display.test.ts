import { describe, expect, it } from "vitest";
import {
  formatBytes,
  swapDisplay,
  toFsRow,
  toggleProcessSort,
  sortProcesses,
  DEFAULT_PROCESS_SORT,
  type ProcessInfo,
} from "./display";

describe("formatBytes", () => {
  it("0 → 0 B", () => expect(formatBytes(0)).toBe("0 B"));
  it("1 GiB → 1.0 GB", () => expect(formatBytes(1024 ** 3)).toBe("1.0 GB"));
});

describe("toFsRow", () => {
  it("设备名缺失回退占位符 —", () => {
    const row = toFsRow({
      device: "",
      mount_point: "/",
      filesystem: "ext4",
      total_bytes: 2 * 1024 ** 3,
      used_bytes: 1024 ** 3,
    });
    expect(row.device).toBe("—");
    expect(row.type).toBe("ext4");
  });

  it("可用 = 总量 − 已用", () => {
    const row = toFsRow({
      device: "/dev/sda1",
      mount_point: "/boot",
      filesystem: "ext4",
      total_bytes: 2 * 1024 ** 3,
      used_bytes: 1024 ** 3,
    });
    expect(row.avail).toBe("1.0 GB");
    expect(row.total).toBe("2.0 GB");
  });
});

describe("swapDisplay", () => {
  it("离线 → 全占位符", () => {
    expect(swapDisplay(1024, 2048, true)).toEqual({
      value: "—%",
      widthPercent: 0,
      detail: "— / —",
    });
  });

  it("total=0（旧 Agent 或未配置 swap）→ 占位符，宽度 0", () => {
    const d = swapDisplay(1024, 0, false);
    expect(d.value).toBe("—%");
    expect(d.widthPercent).toBe(0);
    // used 有真实值仍展示（旧 Agent 场景 used 真实、total 缺失）
    expect(d.detail).toBe("1.0 KB / —");
  });

  it("total>0 → 真实百分比与明细", () => {
    const d = swapDisplay(512 * 1024 ** 2, 2 * 1024 ** 3, false);
    expect(d.value).toBe("25.0%");
    expect(d.widthPercent).toBe(25);
    expect(d.detail).toBe("512.0 MB / 2.0 GB");
  });
});

/* ── 进程列表排序（对齐 FileManager 列头排序交互）────────── */

/** 进程测试样本构造器（未给出的字段取安全默认值） */
function proc(partial: Partial<ProcessInfo> & { pid: number }): ProcessInfo {
  return {
    name: `p${partial.pid}`,
    user: "root",
    cpu_percent: 0,
    mem_percent: 0,
    mem_bytes: 0,
    state: "S",
    ...partial,
  };
}

describe("toggleProcessSort", () => {
  it("同列点击 → 翻转方向", () => {
    expect(toggleProcessSort({ key: "cpu", dir: "desc" }, "cpu")).toEqual({ key: "cpu", dir: "asc" });
    expect(toggleProcessSort({ key: "cpu", dir: "asc" }, "cpu")).toEqual({ key: "cpu", dir: "desc" });
  });

  it("异列点击 → 该列默认方向（cpu/mem 降序，其余升序）", () => {
    expect(toggleProcessSort(DEFAULT_PROCESS_SORT, "mem")).toEqual({ key: "mem", dir: "desc" });
    expect(toggleProcessSort(DEFAULT_PROCESS_SORT, "pid")).toEqual({ key: "pid", dir: "asc" });
    expect(toggleProcessSort(DEFAULT_PROCESS_SORT, "name")).toEqual({ key: "name", dir: "asc" });
    expect(toggleProcessSort(DEFAULT_PROCESS_SORT, "state")).toEqual({ key: "state", dir: "asc" });
  });
});

describe("sortProcesses", () => {
  const sample: ProcessInfo[] = [
    proc({ pid: 30, name: "nginx", cpu_percent: 5.5, mem_percent: 10 }),
    proc({ pid: 10, name: "systemd", cpu_percent: 20, mem_percent: 2 }),
    proc({ pid: 20, name: "chrome", cpu_percent: 20, mem_percent: 30 }),
  ];

  it("默认 CPU 降序：最活跃进程置顶", () => {
    expect(sortProcesses(sample, DEFAULT_PROCESS_SORT).map((p) => p.pid)).toEqual([10, 20, 30]);
  });

  it("CPU 升序翻转", () => {
    expect(sortProcesses(sample, { key: "cpu", dir: "asc" }).map((p) => p.pid)).toEqual([30, 10, 20]);
  });

  it("主值相同 → 回退 pid 升序（不随方向翻转）", () => {
    // pid 10 与 20 的 cpu 同为 20：desc 时 [10, 20]（pid 升序聚拢），asc 时同样 [10, 20]
    const desc = sortProcesses(sample, { key: "cpu", dir: "desc" });
    const asc = sortProcesses(sample, { key: "cpu", dir: "asc" });
    expect(desc.slice(0, 2).map((p) => p.pid)).toEqual([10, 20]);
    expect(asc.slice(-2).map((p) => p.pid)).toEqual([10, 20]);
  });

  it("名称列：文本 localeCompare 排序", () => {
    expect(sortProcesses(sample, { key: "name", dir: "asc" }).map((p) => p.name)).toEqual([
      "chrome", "nginx", "systemd",
    ]);
    expect(sortProcesses(sample, { key: "name", dir: "desc" }).map((p) => p.name)).toEqual([
      "systemd", "nginx", "chrome",
    ]);
  });

  it("内存列：按 mem_percent 降序", () => {
    expect(sortProcesses(sample, { key: "mem", dir: "desc" }).map((p) => p.pid)).toEqual([20, 30, 10]);
  });

  it("不修改入参数组", () => {
    const before = [...sample];
    sortProcesses(sample, { key: "pid", dir: "asc" });
    expect(sample).toEqual(before);
  });
});
