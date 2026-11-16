/**
 * 清理链安全执行器
 *
 * 终端 teardown 等多点清理链（teardownNow / 幽灵会话补清 / 断连回调）要求
 * 「一个失败不断链」：裸 forEach 里某个 unlisten 在依赖 chunk 内部抛同步
 * 异常（双重 unlisten 竞态、dispose 后回调命中已释放对象）时，会中断后续
 * 清理并升级为 uncaught 异常（R8-N1：`Uncaught TypeError: r is not a function`）。
 */
import { createLogger } from './logger';

const log = createLogger('Unlisten');

/** 逐个调用清理函数；单个抛错仅记日志，不中断其余清理、不向外抛 */
export function safeUnlistenAll(fns: ReadonlyArray<() => void>, context?: string): void {
  for (let i = 0; i < fns.length; i++) {
    try {
      fns[i]();
    } catch (e) {
      log.warn(`清理函数 #${i} 抛错${context ? `（${context}）` : ''}:`, e);
    }
  }
}
