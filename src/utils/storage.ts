import { load } from "@tauri-apps/plugin-store";

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
        console.log(`[TauriStorage] GET "${name}" from ${storePath}:`, value);
        return value !== null && value !== undefined 
          ? JSON.stringify(value) 
          : null;
      } catch (e) {
        console.warn(`[TauriStorage] Failed to get "${name}" from ${storePath}:`, e);
        return null;
      }
    },

    setItem: async (name: string, value: string): Promise<void> => {
      try {
        const parsedValue = JSON.parse(value);
        console.log(`[TauriStorage] SET "${name}" in ${storePath}:`, parsedValue);
        await store.set(name, parsedValue);
        await store.save();
      } catch (e) {
        console.warn(`[TauriStorage] Failed to set "${name}" in ${storePath}:`, e);
      }
    },

    removeItem: async (name: string): Promise<void> => {
      try {
        await store.delete(name);
        await store.save();
      } catch (e) {
        console.warn(`[TauriStorage] Failed to delete "${name}" from ${storePath}:`, e);
      }
    },
  };
}

/* ── 预定义的 Store 实例（分层管理）─────────────────────── */

// 存储实例缓存
let _settingsStorage: TauriStorage | null = null;
let _serversStorage: TauriStorage | null = null;
let _secureStorage: TauriStorage | null = null;

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