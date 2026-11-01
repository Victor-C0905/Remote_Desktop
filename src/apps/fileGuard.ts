// src/apps/fileGuard.ts
// 移动/复制防呆评估（纯函数 + 单测）：远程服务器上误动系统目录/敏感目录
// 可能导致服务或登录不可用，操作前给出分级确认。只拦"位置风险"，
// 普通业务文件的移动保持零打扰（移动本身可逆，不做全面确认）。

/** 系统关键目录前缀：进出这些位置（含其子路径）都值得确认 */
export const SYSTEM_PATHS = [
  '/etc', '/usr', '/bin', '/sbin', '/lib', '/lib32', '/lib64', '/libx32',
  '/boot', '/proc', '/sys', '/dev', '/run', '/var',
] as const;

/** 敏感目录段：路径任一段命中即提示（误移可能影响登录/凭据） */
export const SENSITIVE_SEGMENTS = ['.ssh', '.gnupg', '.kube', '.aws'] as const;

export type GuardReason = 'systemPath' | 'sensitiveDir';

export interface GuardFinding {
  reason: GuardReason;
  /** 首个命中路径（展示样例） */
  sample: string;
}

const isUnderOrEqual = (path: string, base: string): boolean =>
  path === base || path.startsWith(`${base}/`);

const containsSegment = (path: string, seg: string): boolean =>
  path.split('/').includes(seg);

/**
 * 评估一批移动/复制：src 全量 + 目标目录触及系统/敏感位置即返回发现。
 * 按原因去重（各取首个命中作为样例），顺序稳定（系统目录优先）。
 */
export function assessTransferRisk(srcs: string[], dstDir: string): GuardFinding[] {
  const findings: GuardFinding[] = [];
  const paths = [...srcs, dstDir];
  const systemHit = paths.find((p) => SYSTEM_PATHS.some((base) => isUnderOrEqual(p, base)));
  if (systemHit) findings.push({ reason: 'systemPath', sample: systemHit });
  const sensitiveHit = paths.find((p) => SENSITIVE_SEGMENTS.some((seg) => containsSegment(p, seg)));
  if (sensitiveHit) findings.push({ reason: 'sensitiveDir', sample: sensitiveHit });
  return findings;
}
