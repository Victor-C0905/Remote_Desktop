// src/components/TransferStatusBar/transferGrouping.test.ts
import { describe, it, expect } from "vitest";
import { groupTransfersByServer, buildGroupSummary } from "./transferGrouping";
import type { TransferTask } from "../../hooks/useTransferProgress";

const task = (overrides: Partial<TransferTask>): TransferTask => ({
  id: "t1",
  session_id: "s1",
  direction: "download",
  file_name: "a.bin",
  remote_path: "/tmp/a.bin",
  file_size: 100,
  transferred: 0,
  speed: 0,
  eta: 0,
  status: "active",
  progress: 0,
  start_time: 1000,
  ...overrides,
});

const servers = [
  { id: "s1", name: "web-01", host: "1.1.1.1" },
  { id: "s2", name: "db-01", host: "2.2.2.2" },
];

describe("groupTransfersByServer", () => {
  it("按 session_id 分组并解析服务器名", () => {
    const groups = groupTransfersByServer(
      [
        task({ id: "t1", session_id: "s1" }),
        task({ id: "t2", session_id: "s2" }),
        task({ id: "t3", session_id: "s1" }),
      ],
      servers,
      "s1"
    );
    expect(groups).toHaveLength(2);
    const g1 = groups.find((g) => g.serverId === "s1")!;
    expect(g1.serverName).toBe("web-01");
    expect(g1.tasks).toHaveLength(2);
    expect(g1.isCurrent).toBe(true);
  });

  it("当前服务器组排在最前", () => {
    const groups = groupTransfersByServer(
      [
        task({ id: "t1", session_id: "s2", start_time: 2000 }),
        task({ id: "t2", session_id: "s1", start_time: 1000 }),
      ],
      servers,
      "s1"
    );
    expect(groups[0].serverId).toBe("s1");
  });

  it("服务器已删除 → 兜底「未知服务器」", () => {
    const groups = groupTransfersByServer(
      [task({ session_id: "gone" })],
      servers,
      null
    );
    expect(groups[0].serverName).toBe("未知服务器");
  });

  it("服务器无 name 时回退 host", () => {
    const groups = groupTransfersByServer(
      [task({ session_id: "s3" })],
      [...servers, { id: "s3", name: "", host: "3.3.3.3" }],
      null
    );
    expect(groups[0].serverName).toBe("3.3.3.3");
  });

  it("非当前组按最近任务时间倒序", () => {
    const groups = groupTransfersByServer(
      [
        task({ id: "old", session_id: "s2", start_time: 1000 }),
        task({ id: "new", session_id: "s3", start_time: 9000 }),
      ],
      servers,
      null
    );
    // 无当前服务器时，全部按最近任务排序
    expect(groups[0].serverId).toBe("s3");
    expect(groups[1].serverId).toBe("s2");
  });
});

describe("buildGroupSummary", () => {
  it("聚合各状态计数", () => {
    const summary = buildGroupSummary([
      task({ status: "interrupted" }),
      task({ status: "interrupted" }),
      task({ status: "completed" }),
    ]);
    expect(summary).toBe("2 个已中断 · 1 个已完成");
  });
});
