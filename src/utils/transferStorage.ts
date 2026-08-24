import { TransferTask } from '../hooks/useTransferProgress';
import { createLogger } from './logger';
import { getTransfersStorage } from './storage';

const log = createLogger('TransferStorage');

const STORAGE_KEY = 'quirel-transfers';

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
    }
};
