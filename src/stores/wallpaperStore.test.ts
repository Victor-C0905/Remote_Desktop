// src/stores/wallpaperStore.test.ts
// 旧版 dataURL 壁纸迁移防线（P0：settings.bin 膨胀 5MB）——旧版导入把图片
// 整串 base64 存进 customPath 并持久化，任意设置变更都全量重写 settings.bin。
// 迁移逻辑（migrateLegacyDataUrlWallpaper + onRehydrateStorage 接线）必须：
// 命中即落盘改存 cachedPath、非命中不动作、失败保持原状待重试、rehydrate
// 管线始终接线。防止有人删迁移或改坏命中条件后旧数据永不收敛。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useWallpaperStore, migrateLegacyDataUrlWallpaper } from "./wallpaperStore";

// 全局 setup 已 mock @tauri-apps/api/core，此处取受管 mock 控制返回值
const invokeMock = vi.mocked(invoke);

const DATA_URL = "data:image/jpeg;base64,dGVzdA==";
const CACHED_PATH = "C:\\Users\\t\\AppData\\Roaming\\com.quirel.app\\wallpaper\\wallpaper.cache.jpeg";
const TINY_B64 = "dGVzdA=="; // atob → "test"，createObjectURL 已 mock，不真解码

const flush = () => new Promise((r) => setTimeout(r, 0));
const win = window as unknown as { __TAURI_INTERNALS__?: unknown };

beforeEach(() => {
  invokeMock.mockReset();
  // jsdom 未实现 Blob URL；迁移成功路径 loadCachedWallpaper 会用到
  URL.createObjectURL = vi.fn(() => "blob:mock-wallpaper");
  URL.revokeObjectURL = vi.fn();
  win.__TAURI_INTERNALS__ = {}; // 模拟 Tauri 环境（isTauri 探测点）
  useWallpaperStore.setState({
    wallpaper: { type: "builtin", builtinId: "dusk" },
    objectUrl: null,
  });
});

afterEach(() => {
  delete win.__TAURI_INTERNALS__;
});

describe("旧版 dataURL 壁纸迁移", () => {
  it("custom + dataURL 命中：落盘改存 cachedPath 并加载 blob", async () => {
    useWallpaperStore.setState({
      wallpaper: { type: "custom", customPath: DATA_URL },
      objectUrl: null,
    });
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "wallpaper_cache_write_data" ? CACHED_PATH : TINY_B64
    );

    await migrateLegacyDataUrlWallpaper();

    expect(invokeMock).toHaveBeenCalledWith("wallpaper_cache_write_data", {
      dataUrl: DATA_URL,
    });
    const w = useWallpaperStore.getState().wallpaper;
    expect(w.type).toBe("cached");
    expect(w.cachedPath).toBe(CACHED_PATH);
    // blob 通道已建立（渲染主通道），旧 dataURL 不再被 state 引用
    expect(useWallpaperStore.getState().objectUrl).toBe("blob:mock-wallpaper");
  });

  it("非 custom 或非 dataURL 的 customPath 不触发迁移", async () => {
    useWallpaperStore.setState({
      wallpaper: { type: "cached", cachedPath: CACHED_PATH },
      objectUrl: null,
    });
    await migrateLegacyDataUrlWallpaper();
    expect(invokeMock).not.toHaveBeenCalled();

    useWallpaperStore.setState({
      wallpaper: { type: "custom", customPath: "C:\\pics\\local.png" },
      objectUrl: null,
    });
    await migrateLegacyDataUrlWallpaper();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("迁移失败保持原状（下次启动重试），不向外抛错", async () => {
    useWallpaperStore.setState({
      wallpaper: { type: "custom", customPath: DATA_URL },
      objectUrl: null,
    });
    invokeMock.mockRejectedValueOnce(new Error("落盘失败"));

    await expect(migrateLegacyDataUrlWallpaper()).resolves.toBeUndefined();
    expect(useWallpaperStore.getState().wallpaper).toEqual({
      type: "custom",
      customPath: DATA_URL,
    });
  });

  it("浏览器调试模式（非 Tauri）不迁移——dataURL 是唯一载体", async () => {
    delete win.__TAURI_INTERNALS__;
    useWallpaperStore.setState({
      wallpaper: { type: "custom", customPath: DATA_URL },
      objectUrl: null,
    });
    await migrateLegacyDataUrlWallpaper();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("rehydrate 管线接线迁移（防线：onRehydrateStorage 不得被删）", async () => {
    useWallpaperStore.setState({
      wallpaper: { type: "custom", customPath: DATA_URL },
      objectUrl: null,
    });
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "wallpaper_cache_write_data" ? CACHED_PATH : TINY_B64
    );

    await useWallpaperStore.persist.rehydrate();
    await flush();

    expect(invokeMock).toHaveBeenCalledWith("wallpaper_cache_write_data", {
      dataUrl: DATA_URL,
    });
    expect(useWallpaperStore.getState().wallpaper.type).toBe("cached");
  });
});
