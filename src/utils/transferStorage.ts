import { TransferTask } from '../hooks/useTransferProgress';
import { createLogger } from './logger';

const log = createLogger('TransferStorage');

const STORAGE_KEY = 'quirel-transfers';

/**
 * 传输状态持久化工具
 * 使用 localStorage 存储传输任务状态，在页面刷新后恢复
 */
export const transferStorage = {
    /**
     * 保存传输任务列表到 localStorage
     * @param transfers - 要保存的任务列表
     */
    save(transfers: TransferTask[]): void {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(transfers));
        } catch (error) {
            log.error('保存传输状态失败:', error);
        }
    },

    /**
     * 从 localStorage 加载传输任务列表
     * @returns 加载的任务列表，如果加载失败则返回空数组
     */
    load(): TransferTask[] {
        try {
            const data = localStorage.getItem(STORAGE_KEY);
            return data ? JSON.parse(data) : [];
        } catch (error) {
            log.error('加载传输状态失败:', error);
            return [];
        }
    },

    /**
     * 清除所有存储的传输任务
     */
    clear(): void {
        localStorage.removeItem(STORAGE_KEY);
    }
};