import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  HALF_LIFE_MS,
  MAX_TRACKED_PATHS_PER_SERVER,
  MIN_FREQUENT_SCORE,
  SESSION_GAP_MS,
  effectiveScore,
  migratePersistedUsage,
  normalizeUsagePath,
  selectFrequentPaths,
  useFilePathUsageStore,
} from "./filePathUsageStore";

const T0 = 1_800_000_000_000;

/** 取当前服务器下某个路径的原始记录 */
function entryOf(serverId: string, path: string) {
  return useFilePathUsageStore.getState().byServer[serverId]?.[path];
}

describe("filePathUsageStore", () => {
  beforeEach(() => {
    useFilePathUsageStore.setState({ byServer: {} });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("路径归一化：折叠重复斜杠、去尾斜杠，拒绝相对路径", () => {
    expect(normalizeUsagePath("/home//u/a/")).toBe("/home/u/a");
    expect(normalizeUsagePath("/")).toBe("/");
    expect(normalizeUsagePath("home/u")).toBeNull();
    expect(normalizeUsagePath("")).toBeNull();
  });

  it("路过型来源（面包屑/上级/前进后退）与非法路径不计分", () => {
    const { recordVisit } = useFilePathUsageStore.getState();
    recordVisit("s1", "/home/u/a", "none");
    recordVisit(null, "/home/u/a", "sidebar");
    recordVisit("s1", "relative/path", "sidebar");
    expect(useFilePathUsageStore.getState().byServer).toEqual({});
  });

  it("首次访问不会进入常用；跨 30 分钟再次访问后过线", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(T0);
    const { recordVisit } = useFilePathUsageStore.getState();

    recordVisit("s1", "/home/u/a", "sidebar");
    const first = entryOf("s1", "/home/u/a");
    expect(first.score).toBe(5);
    expect(first.count).toBe(1);
    // 单次最高权重 5，低于门槛 5.5
    expect(selectFrequentPaths(useFilePathUsageStore.getState().byServer, "s1", 6, T0)).toEqual([]);

    now.mockReturnValue(T0 + SESSION_GAP_MS + 1_000);
    recordVisit("s1", "/home/u/a", "sidebar");
    const second = entryOf("s1", "/home/u/a");
    expect(second.score).toBeGreaterThanOrEqual(MIN_FREQUENT_SCORE);
    expect(second.count).toBe(2);

    const frequent = selectFrequentPaths(
      useFilePathUsageStore.getState().byServer,
      "s1",
      6,
      T0 + SESSION_GAP_MS + 1_000,
    );
    expect(frequent.map((p) => p.path)).toEqual(["/home/u/a"]);
  });

  it("30 分钟窗口内重复访问只刷新最近时间，不重复加分", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(T0);
    const { recordVisit } = useFilePathUsageStore.getState();
    recordVisit("s1", "/home/u/a", "sidebar");

    now.mockReturnValue(T0 + 60_000);
    recordVisit("s1", "/home/u/a", "sidebar");

    const entry = entryOf("s1", "/home/u/a");
    expect(entry.score).toBe(5);
    expect(entry.count).toBe(1);
    expect(entry.lastUsed).toBe(T0 + 60_000);
    expect(useFilePathUsageStore.getState().byServer.s1["/home/u/a"].scoreAt).toBe(T0);
  });

  it("双击权重更低：两次会话不够，三次会话才过线", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(T0);
    const { recordVisit } = useFilePathUsageStore.getState();

    recordVisit("s1", "/a", "doubleclick");
    now.mockReturnValue(T0 + SESSION_GAP_MS + 1);
    recordVisit("s1", "/a", "doubleclick");
    expect(entryOf("s1", "/a").score).toBeLessThan(MIN_FREQUENT_SCORE);

    now.mockReturnValue(T0 + 2 * (SESSION_GAP_MS + 1));
    recordVisit("s1", "/a", "doubleclick");
    expect(entryOf("s1", "/a").score).toBeGreaterThanOrEqual(MIN_FREQUENT_SCORE);
  });

  it("衰减分数随时间下降；30 天半衰期后低于门槛", () => {
    const entry = { score: 10, scoreAt: T0, lastUsed: T0, count: 2 };
    expect(effectiveScore(entry, T0)).toBe(10);
    expect(effectiveScore(entry, T0 + HALF_LIFE_MS)).toBeCloseTo(5, 6);

    useFilePathUsageStore.setState({ byServer: { s1: { "/a": entry, "/b": { ...entry, score: 20 } } } });
    // 30 天后：/a=5（低于门槛），/b=10（仍保留）
    const frequent = selectFrequentPaths(useFilePathUsageStore.getState().byServer, "s1", 6, T0 + HALF_LIFE_MS);
    expect(frequent.map((p) => p.path)).toEqual(["/b"]);
  });

  it("按衰减分数降序、同分按最近访问降序排序", () => {
    useFilePathUsageStore.setState({
      byServer: {
        s1: {
          "/a": { score: 8, scoreAt: T0, lastUsed: T0, count: 2 },
          "/b": { score: 12, scoreAt: T0, lastUsed: T0, count: 3 },
          "/c": { score: 12, scoreAt: T0, lastUsed: T0 + 1000, count: 3 },
        },
      },
    });
    expect(selectFrequentPaths(useFilePathUsageStore.getState().byServer, "s1", 6, T0).map((p) => p.path)).toEqual([
      "/c",
      "/b",
      "/a",
    ]);
  });

  it("来源权重：地址栏/搜索/侧边栏 5，双击 2", () => {
    vi.spyOn(Date, "now").mockReturnValue(T0);
    const { recordVisit } = useFilePathUsageStore.getState();
    recordVisit("s1", "/a", "address");
    recordVisit("s1", "/b", "search");
    recordVisit("s1", "/c", "sidebar");
    recordVisit("s1", "/d", "doubleclick");
    expect(entryOf("s1", "/a").score).toBe(5);
    expect(entryOf("s1", "/b").score).toBe(5);
    expect(entryOf("s1", "/c").score).toBe(5);
    expect(entryOf("s1", "/d").score).toBe(2);
  });

  it("场景：面包屑逐级浏览不污染；首次访问不显示，跨会话第二次才出现", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(T0);
    const { recordVisit } = useFilePathUsageStore.getState();

    // 面包屑/上级沿链路逐级路过，不产生任何记录
    for (const path of ["/", "/var", "/var/log", "/var/log/nginx", "/var/log/nginx/access"]) {
      recordVisit("s1", path, "none");
    }
    expect(useFilePathUsageStore.getState().byServer).toEqual({});

    // 首次明确访问：分数 5，低于门槛，不显示
    recordVisit("s1", "/var/log/nginx", "sidebar");
    expect(selectFrequentPaths(useFilePathUsageStore.getState().byServer, "s1", 6, T0)).toEqual([]);

    // 31 分钟后再次明确访问：过线并显示
    now.mockReturnValue(T0 + SESSION_GAP_MS + 60_000);
    recordVisit("s1", "/var/log/nginx", "sidebar");
    const frequent = selectFrequentPaths(
      useFilePathUsageStore.getState().byServer,
      "s1",
      6,
      T0 + SESSION_GAP_MS + 60_000,
    );
    expect(frequent.map((p) => p.path)).toEqual(["/var/log/nginx"]);
  });

  it("按服务器隔离，clearServer 只清理指定服务器", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(T0);
    const { recordVisit, clearServer } = useFilePathUsageStore.getState();
    recordVisit("s1", "/a", "sidebar");
    recordVisit("s2", "/b", "sidebar");
    expect(entryOf("s1", "/a").score).toBe(5);
    expect(entryOf("s2", "/b").score).toBe(5);

    clearServer("s1");
    expect(useFilePathUsageStore.getState().byServer.s1).toBeUndefined();
    expect(useFilePathUsageStore.getState().byServer.s2).toBeDefined();
    void now;
  });

  it("超过单服务器上限时淘汰衰减分数最低的路径", () => {
    let now = T0;
    const nowSpy = vi.spyOn(Date, "now").mockImplementation(() => now);
    useFilePathUsageStore.setState({ byServer: {} });
    const { recordVisit } = useFilePathUsageStore.getState();
    for (let i = 0; i <= MAX_TRACKED_PATHS_PER_SERVER; i++) {
      now += SESSION_GAP_MS + 1;
      recordVisit("s1", `/d${i}`, "sidebar");
    }
    const keys = Object.keys(useFilePathUsageStore.getState().byServer.s1);
    expect(keys).toHaveLength(MAX_TRACKED_PATHS_PER_SERVER);
    expect(keys).not.toContain("/d0");
    void nowSpy;
  });

  it("v0 迁移：旧 count 折算为初始分数，scoreAt 取 lastUsed", () => {
    const migrated = migratePersistedUsage(
      { byServer: { s1: { "/a": { count: 10, lastUsed: T0 }, "/b": { count: 1, lastUsed: T0 } } } },
      0,
    );
    expect(migrated.byServer.s1["/a"]).toEqual({ score: 10, scoreAt: T0, lastUsed: T0, count: 10 });
    expect(migrated.byServer.s1["/b"].score).toBe(1);
    expect(selectFrequentPaths(migrated.byServer, "s1", 6, T0).map((p) => p.path)).toEqual(["/a"]);
  });
});
