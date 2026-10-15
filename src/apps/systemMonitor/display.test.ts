import { describe, expect, it } from "vitest";
import { formatBytes, swapDisplay, toFsRow } from "./display";

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
