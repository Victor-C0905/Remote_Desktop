/**
 * 会话镜像（sessionStorage）读写与纪元闸门
 *
 * 镜像统一形态：`{ epoch, active, connected }`。纪元戳是镜像的组成部分，
 * 消费方按「纪元一致才消费」校验——与 StorageInitializer 的启动闸门
 * （对比 app.epoch 后清理）互为纵深防御：闸门被绕过（读取失败/时序竞态）
 * 时，带旧纪元的镜像仍会在消费端被拒绝，堵死「启动自动重连时有时无」
 * 的泄漏通道（R8-N2/R9-N3）。
 *
 * fail-closed 原则：任何形态不符、纪元缺失或不匹配一律视为上一世代残留，
 * 宁可丢一次恢复，不可把跨世代镜像泄进新进程。
 */
import { createLogger } from './logger';

const log = createLogger('SessionMirror');

/** 镜像业务负载：连接中的服务器集合 + 活动服务器 */
export interface SessionMirrorPayload {
  active: string | null;
  connected: string[];
}

/** 带纪元戳的镜像 */
export interface StampedSessionMirror extends SessionMirrorPayload {
  epoch: string;
}

/** 序列化镜像（自动盖当前纪元戳） */
export function serializeSessionMirror(epoch: string, payload: SessionMirrorPayload): string {
  return JSON.stringify({ epoch, ...payload });
}

/**
 * 解析并校验镜像。
 * @returns 合法且纪元匹配的镜像；否则 null（调用方按「无快照」处理）
 */
export function parseSessionMirror(raw: string | null, currentEpoch: string): StampedSessionMirror | null {
  if (!raw) return null;
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return null; // 镜像损坏视同无快照
  }
  if (typeof data !== 'object' || data === null) return null;
  const { epoch, active, connected } = data as Record<string, unknown>;
  if (typeof epoch !== 'string') return null; // 旧版无纪元戳镜像
  if (epoch !== currentEpoch) return null; // 上一世代镜像（或闸门未运行的降级路径）
  if (active !== null && typeof active !== 'string') return null;
  if (!Array.isArray(connected) || !connected.every((id) => typeof id === 'string')) return null;
  return { epoch, active, connected };
}

/** 清空全部会话镜像（窗口 wm.session / 连接 sm.session / 浏览器挂载代际） */
export function clearSessionMirrors(storage: Storage): void {
  storage.removeItem('wm.session');
  storage.removeItem('sm.session');
  const stale: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (k?.startsWith('browser.mountGen.')) stale.push(k);
  }
  stale.forEach((k) => {
    log.debug('清除挂载代际镜像:', k);
    storage.removeItem(k);
  });
}
