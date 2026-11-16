import { TransferTask } from '../hooks/useTransferProgress';
import { createLogger } from './logger';
import { getTransfersStorage } from './storage';

const log = createLogger('TransferStorage');

const STORAGE_KEY = 'quirel-transfers';

/** 服务器删除后通知已挂载的传输面板同步移除内存态（detail = serverId） */
export const TRANSFER_PURGE_SERVER_EVENT = 'quirel:transfers:purge-server';

/**
 * 传输状态持久化工具
 * 使用 Tauri Store（Rust 侧管理）存储传输任务状态，在页面刷新后恢复
 */
export const transferStorage = {
    /**
     * 保存传输任务列表到 Tauri Store
     * @param transfers - 要保存的任务列表
     */
    async save(transfers: TransferTask[]): Promise<void> {
        try {
            const storage = await getTransfersStorage();
            await storage.setItem(STORAGE_KEY, JSON.stringify(transfers));
        } catch (error) {
            log.error('保存传输状态失败:', error);
        }
    },

    /**
     * 从 Tauri Store 加载传输任务列表
     * @returns 加载的任务列表，如果加载失败则返回空数组
     */
    async load(): Promise<TransferTask[]> {
        try {
            const storage = await getTransfersStorage();
            const data = await storage.getItem(STORAGE_KEY);
            return data ? JSON.parse(data) : [];
        } catch (error) {
            log.error('加载传输状态失败:', error);
            return [];
        }
    },

    /**
     * 清除所有存储的传输任务
     */
    async clear(): Promise<void> {
        try {
            const storage = await getTransfersStorage();
            await storage.removeItem(STORAGE_KEY);
        } catch (error) {
            log.error('清除传输状态失败:', error);
        }
    },

    /**
     * 删除服务器时清除其传输任务（服务器生命周期统一清理入口调用）：
     * 先过滤落盘数据，再派发事件让已挂载的传输面板同步移除内存态。
     * 无论落盘是否有变化都派发事件——节流窗口内的新任务可能尚未落盘。
     */
    async removeServer(serverId: string): Promise<void> {
        try {
            const tasks = await this.load();
            const remaining = tasks.filter((t) => t.session_id !== serverId);
            if (remaining.length !== tasks.length) {
                const storage = await getTransfersStorage();
                await storage.setItem(STORAGE_KEY, JSON.stringify(remaining));
            }
        } catch (error) {
            log.error('清除服务器传输任务失败:', error);
        }
        window.dispatchEvent(new CustomEvent(TRANSFER_PURGE_SERVER_EVENT, { detail: serverId }));
    },
};
