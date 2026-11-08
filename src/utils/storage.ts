import { load } from "@tauri-apps/plugin-store";
import { createLogger } from "./logger";

const log = createLogger('Storage');

/* ── 磁盘契约与写保护 ───────────────────────────────────── */

export type StoreBlockReason = "newer-version" | "unreadable";

/** 阻断状态：本地文件版本过高或结构不可读时，禁止任何写回，等用户显式处理 */
export interface StoreBlock {
  path: string;
  reason: StoreBlockReason;
  detail: string;
  version?: number;
  expected?: number;
}

/**
 * zustand persist store 名称 → 期望的磁盘结构版本。
 * 只有破坏性结构变更才递增；结构不变时禁止改动（见 serversStore 注释）。
 */
const EXPECTED_STORE_VERSIONS: Record<string, number> = {
  "quirel-servers": 0,
  // v1：browserProxyPort 语义变更（旧默认 1080 → 0 随机端口，M12），
  // 旧 v0 文件由 persist migrate 升级
  "quirel-settings": 1,
  "quirel-wallpaper": 0,
  "notifications": 0,
  "bookmarks": 0,
  "history": 0,
};

/** 直接存放业务 JSON 的文件（非 zustand persist 的 {state, version} 包装） */
const RAW_STORE_PATHS = new Set(["transfers.bin", "secure.bin"]);

const blockedStores = new Map<string, StoreBlock>();

/** 当前被阻断的 store 文件（启动预检/恢复对话框使用） */
export function getStoreBlocks(): StoreBlock[] {
  return Array.from(blockedStores.values());
}

/** 指定 store 文件是否处于禁止写入状态 */
export function isStoreBlocked(storePath: string): boolean {
  return blockedStores.has(storePath);
}

/**
 * 校验 zustand persist 的磁盘对象（纯函数，便于单测）。
 * 只拒绝“更高版本”与结构缺失；更低版本交给 persist migrate 处理。
 */
export function evaluatePersistedValue(
  value: unknown,
  expectedVersion: number,
): { ok: true } | { ok: false; reason: StoreBlockReason; detail: string; version?: number } {
  if (value === null || value === undefined) return { ok: true };
  if (typeof value !== "object" || Array.isArray(value)) {
    return { ok: false, reason: "unreadable", detail: "持久化数据不是对象" };
  }
  const record = value as Record<string, unknown>;
  if (!("state" in record)) {
    return { ok: false, reason: "unreadable", detail: "持久化数据缺少 state 字段" };
  }
  const version = record.version;
  if (typeof version === "number" && version > expectedVersion) {
    return {
      ok: false,
      reason: "newer-version",
      version,
      detail: `数据版本 ${version} 高于当前支持的 ${expectedVersion}`,
    };
  }
  return { ok: true };
}

/**
 * 记录阻断：原文件保留，等待用户在恢复对话框处理。
 * 备份由 Rust 侧在 webview 加载前统一完成（见 store_maintenance::backup_all_on_startup）。
 */
function blockStore(storePath: string, block: Omit<StoreBlock, "path">): void {
  if (blockedStores.has(storePath)) return;
  blockedStores.set(storePath, { path: storePath, ...block });
  log.error(`已停止写入 ${storePath}: ${block.detail}（原文件保留，等待用户处理）`);
}

/* ── Types ─────────────────────────────────────────────── */

interface TauriStorage {
  getItem: (name: string) => Promise<string | null>;
  setItem: (name: string, value: string) => Promise<void>;
  removeItem: (name: string) => Promise<void>;
}

/* ── Tauri Storage Adapter for Zustand ───────────────────── */

/**
 * 创建 Tauri Store 作为 Zustand persist 中间件的存储后端
 * 
 * 注意：使用 load() 预加载存储，而不是 LazyStore
 * 因为 Zustand persist 中间件在初始化时就需要读取数据
 * 
 * @param storePath - Store 文件路径（相对于 app_data_dir）
 * @returns Zustand 兼容的 Storage 对象
 */
export async function createTauriStorage(storePath: string): Promise<TauriStorage> {
  // 使用 load() 预加载存储，确保数据立即可用
  const store = await load(storePath);

  return {
    getItem: async (name: string): Promise<string | null> => {
      try {
        const value = await store.get(name);
        log.debug(`GET "${name}" from ${storePath}:`, value);
        if (value === null || value === undefined) return null;

        // 磁盘契约校验：更高版本/结构损坏 → 阻断该文件全部写入，保留原文件
        if (!RAW_STORE_PATHS.has(storePath)) {
          const expected = EXPECTED_STORE_VERSIONS[name];
          if (expected !== undefined) {
            const verdict = evaluatePersistedValue(value, expected);
            if (!verdict.ok) {
              blockStore(storePath, {
                reason: verdict.reason,
                detail: verdict.detail,
                version: verdict.version,
                expected,
              });
              return null;
            }
          }
        }

        return JSON.stringify(value);
      } catch (e) {
        // 读失败意味着设置/数据将静默回退默认值，必须 error 级才不会淹没在 debug 噪音里
        log.error(`Failed to get "${name}" from ${storePath}:`, e);
        blockStore(storePath, { reason: "unreadable", detail: String(e) });
        return null;
      }
    },

    setItem: async (name: string, value: string): Promise<void> => {
      // 阻断态的 store 一律拒绝写回：宁可本次修改不生效，也不覆盖原文件
      if (blockedStores.has(storePath)) {
        log.error(`已拒绝写入 ${storePath}（本地数据处于保护状态）`);
        return;
      }
      try {
        const parsedValue = JSON.parse(value);
        log.debug(`SET "${name}" in ${storePath}:`, parsedValue);
        await store.set(name, parsedValue);
        await store.save();
      } catch (e) {
        log.warn(`Failed to set "${name}" in ${storePath}:`, e);
      }
    },

    removeItem: async (name: string): Promise<void> => {
      // 删除同属写操作：保护态下不允许改写原文件
      if (blockedStores.has(storePath)) {
        log.error(`已拒绝删除 ${storePath} 中的 "${name}"（本地数据处于保护状态）`);
        return;
      }
      try {
        await store.delete(name);
        await store.save();
      } catch (e) {
        log.warn(`Failed to delete "${name}" from ${storePath}:`, e);
      }
    },
  };
}

/* ── 预定义的 Store 实例（分层管理）─────────────────────── */

// 存储实例缓存
let _settingsStorage: TauriStorage | null = null;
let _serversStorage: TauriStorage | null = null;
let _secureStorage: TauriStorage | null = null;
let _transfersStorage: TauriStorage | null = null;

/**
 * 获取普通设置存储（主题、字体、壁纸等）
 */
export async function getSettingsStorage(): Promise<TauriStorage> {
  if (!_settingsStorage) {
    _settingsStorage = await createTauriStorage("settings.bin");
  }
  return _settingsStorage;
}

/**
 * 获取服务器配置存储
 */
export async function getServersStorage(): Promise<TauriStorage> {
  if (!_serversStorage) {
    _serversStorage = await createTauriStorage("servers.bin");
  }
  return _serversStorage;
}

/**
 * 获取安全设置存储（敏感数据）
 */
export async function getSecureStorage(): Promise<TauriStorage> {
  if (!_secureStorage) {
    _secureStorage = await createTauriStorage("secure.bin");
  }
  return _secureStorage;
}

/**
 * 获取传输任务存储（传输进度、历史）
 */
export async function getTransfersStorage(): Promise<TauriStorage> {
  if (!_transfersStorage) {
    _transfersStorage = await createTauriStorage("transfers.bin");
  }
  return _transfersStorage;
}

// 异步存储对象（用于 Zustand persist）
export const settingsStorage: TauriStorage = {
  getItem: async (name: string) => {
    const storage = await getSettingsStorage();
    return storage.getItem(name);
  },
  setItem: async (name: string, value: string) => {
    const storage = await getSettingsStorage();
    return storage.setItem(name, value);
  },
  removeItem: async (name: string) => {
    const storage = await getSettingsStorage();
    return storage.removeItem(name);
  },
};

export const serversStorage: TauriStorage = {
  getItem: async (name: string) => {
    const storage = await getServersStorage();
    return storage.getItem(name);
  },
  setItem: async (name: string, value: string) => {
    const storage = await getServersStorage();
    return storage.setItem(name, value);
  },
  removeItem: async (name: string) => {
    const storage = await getServersStorage();
    return storage.removeItem(name);
  },
};

export const secureStorage: TauriStorage = {
  getItem: async (name: string) => {
    const storage = await getSecureStorage();
    return storage.getItem(name);
  },
  setItem: async (name: string, value: string) => {
    const storage = await getSecureStorage();
    return storage.setItem(name, value);
  },
  removeItem: async (name: string) => {
    const storage = await getSecureStorage();
    return storage.removeItem(name);
  },
};

export const transfersStorage: TauriStorage = {
  getItem: async (name: string) => {
    const storage = await getTransfersStorage();
    return storage.getItem(name);
  },
  setItem: async (name: string, value: string) => {
    const storage = await getTransfersStorage();
    return storage.setItem(name, value);
  },
  removeItem: async (name: string) => {
    const storage = await getTransfersStorage();
    return storage.removeItem(name);
  },
};
let _browserDataStorage: TauriStorage | null = null;

/**
 * 获取浏览器数据存储（书签/历史等浏览产物）
 */
export async function getBrowserDataStorage(): Promise<TauriStorage> {
  if (!_browserDataStorage) {
    _browserDataStorage = await createTauriStorage("browser-data.bin");
  }
  return _browserDataStorage;
}

// 异步存储对象（用于 Zustand persist）：书签与历史两个 store 共用一个文件
export const browserDataStorage: TauriStorage = {
  getItem: async (name) => (await getBrowserDataStorage()).getItem(name),
  setItem: async (name, value) => (await getBrowserDataStorage()).setItem(name, value),
  removeItem: async (name) => (await getBrowserDataStorage()).removeItem(name),
};

let _notificationsStorage: TauriStorage | null = null;

/**
 * 获取通知存储（按服务器持久化的通知中心条目）
 */
export async function getNotificationsStorage(): Promise<TauriStorage> {
  if (!_notificationsStorage) {
    _notificationsStorage = await createTauriStorage("notifications.bin");
  }
  return _notificationsStorage;
}

// 异步存储对象（用于 Zustand persist）
export const notificationsStorage: TauriStorage = {
  getItem: async (name) => (await getNotificationsStorage()).getItem(name),
  setItem: async (name, value) => (await getNotificationsStorage()).setItem(name, value),
  removeItem: async (name) => (await getNotificationsStorage()).removeItem(name),
};

/* ── 启动预检 ───────────────────────────────────────────── */

/** 关键 store 文件与其中的数据键：预检必须在渲染子组件前完成 */
const PREFLIGHT_GROUPS: Array<{
  path: string;
  getStorage: () => Promise<TauriStorage>;
  keys: string[];
}> = [
  { path: "servers.bin", getStorage: getServersStorage, keys: ["quirel-servers"] },
  { path: "settings.bin", getStorage: getSettingsStorage, keys: ["quirel-settings", "quirel-wallpaper"] },
  { path: "notifications.bin", getStorage: getNotificationsStorage, keys: ["notifications"] },
  { path: "browser-data.bin", getStorage: getBrowserDataStorage, keys: ["bookmarks", "history"] },
  { path: "transfers.bin", getStorage: getTransfersStorage, keys: ["quirel-transfers"] },
];

/**
 * 读取所有关键 store 的磁盘数据并完成契约校验。
 * 返回被阻断的文件列表；非空时调用方必须阻止进入主界面，等待用户备份/重启。
 */
export async function preflightStores(): Promise<StoreBlock[]> {
  for (const group of PREFLIGHT_GROUPS) {
    try {
      const storage = await group.getStorage();
      for (const key of group.keys) {
        await storage.getItem(key);
      }
    } catch (e) {
      blockStore(group.path, { reason: "unreadable", detail: String(e) });
    }
  }
  return getStoreBlocks();
}
