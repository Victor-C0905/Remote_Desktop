/**
 * 服务器凭据的 keyring 读写（Rust credentials.rs commands 的前端封装）。
 *
 * 安全模型：servers.bin（app_data_dir 明文 JSON）只保留非敏感字段；
 * password/privateKey/passphrase 存 OS 凭据管理器（Windows Credential
 * Manager / macOS Keychain / Linux keyutils），account = serverId。
 * 启动时 hydrateCredentials 把凭据合并回内存 state，消费方（连接逻辑）
 * 零改动；persist 时 stripCredentials 剥离，永不落盘。
 */
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "./logger";
import type { ServerConfig } from "../types/server";

const log = createLogger("Credentials");

export interface ServerSecrets {
  password?: string;
  privateKey?: string;
  passphrase?: string;
}

const SECRET_KEYS: (keyof ServerSecrets)[] = ["password", "privateKey", "passphrase"];

/** 是否含任何敏感字段（用于判断是否需要写 keyring） */
export function hasSecrets(auth: Partial<ServerConfig["auth"]> | undefined): boolean {
  if (!auth) return false;
  return SECRET_KEYS.some((k) => typeof auth[k] === "string" && (auth[k] as string).length > 0);
}

/** 剥离 auth 中的敏感字段（persist 落盘前 / 迁移清理用） */
export function stripSecrets<T extends { auth?: Partial<ServerConfig["auth"]> }>(server: T): T {
  if (!server.auth) return server;
  const { password, privateKey, passphrase, ...rest } = server.auth;
  void password; void privateKey; void passphrase;
  return { ...server, auth: rest as T["auth"] };
}

/**
 * keyring 可用性门控：凭据写入至少成功一次前，partialize 不剥离明文——
 * 否则迁移遇到 credential_set 失败（如 dev 进程未重启缺 command）时，
 * 落盘剥离会直接蒸发凭据（写入失败与剥离之间必须有保护）。
 */
let keyringConfirmed = false;

/**
 * keyring 健康状态：boot 时探测一次。
 * - ok：正常，凭据保存可用
 * - unavailable：系统凭据库不可用（含原因）——含凭据的表单提交将被阻止
 */
export type KeyringHealth = { status: "unknown" } | { status: "ok" } | { status: "unavailable"; reason: string };
let keyringHealth: KeyringHealth = { status: "unknown" };

export function getKeyringHealth(): KeyringHealth {
  return keyringHealth;
}

/** 启动探测：写/读/删探针条目验证整条链路；失败推送通知（一次性） */
export async function probeKeyring(): Promise<void> {
  try {
    await invoke("credential_probe");
    keyringHealth = { status: "ok" };
    log.info("keyring 健康探测通过");
  } catch (e) {
    keyringHealth = { status: "unavailable", reason: String(e) };
    log.error("keyring 不可用:", e);
    try {
      const { useNotificationStore } = await import("../stores/notificationStore");
      useNotificationStore.getState().pushNotification({
        title: "系统凭据库不可用",
        body: String(e),
        action: "服务器密码/私钥将无法保存。请重启应用或检查系统凭据服务后重试",
        urgency: "critical",
        source: "凭据",
      });
    } catch { /* 通知不可用时仅日志 */ }
  }
}

/** keyring 已确认可写（凭据已安全落库），此后 servers.bin 才剥离明文 */
export function isKeyringReady(): boolean {
  return keyringConfirmed;
}

/** 写入凭据；返回是否成功。空凭据视为成功（无需存储）。失败弹通知可见。 */
export async function secretsSet(serverId: string, auth: Partial<ServerConfig["auth"]> | undefined): Promise<boolean> {
  try {
    const payload: ServerSecrets = {};
    for (const k of SECRET_KEYS) {
      const v = auth?.[k];
      if (typeof v === "string" && v.length > 0) payload[k] = v;
    }
    if (Object.keys(payload).length === 0) {
      await secretsDelete(serverId);
      return true;
    }
    await invoke("credential_set", { account: serverId, value: JSON.stringify(payload) });
    keyringConfirmed = true;
    return true;
  } catch (e) {
    log.error("凭据写入 keyring 失败:", e);
    // 不再静默：用户必须知道凭据没进安全库（当前会话回退明文落盘模式）
    try {
      const { useNotificationStore } = await import("../stores/notificationStore");
      useNotificationStore.getState().pushNotification({
        title: "凭据安全存储失败",
        body: String(e),
        action: "凭据本次将以明文保存在本地配置中；若刚更新过应用，请完全重启后重新保存服务器",
        urgency: "critical",
        source: "凭据",
      });
    } catch { /* 通知不可用时仅日志 */ }
    return false;
  }
}

export async function secretsGet(serverId: string): Promise<ServerSecrets | null> {
  try {
    const raw = await invoke<string | null>("credential_get", { account: serverId });
    if (!raw) return null;
    return JSON.parse(raw) as ServerSecrets;
  } catch {
    return null;
  }
}

export async function secretsDelete(serverId: string): Promise<void> {
  try {
    await invoke("credential_delete", { account: serverId });
  } catch (e) {
    log.warn("凭据删除失败:", e);
  }
}

/** 完全清除用：按已知 serverId 批量删除系统凭据 */
export async function secretsDeleteAll(serverIds: string[]): Promise<void> {
  try {
    await invoke("credential_delete_all", { accounts: serverIds });
  } catch (e) {
    log.warn("批量删除凭据失败:", e);
  }
}

/**
 * 启动装配：把 keyring 中的凭据合并回服务器列表（内存态）。
 * 同时执行旧数据迁移——若 servers.bin 里还残留明文凭据：
 * 写入 keyring → 返回剥离后的列表（persist 下次落盘即为净数据）。
 */
export async function hydrateCredentials(servers: ServerConfig[]): Promise<ServerConfig[]> {
  const merged = await Promise.all(
    servers.map(async (s) => {
      const legacy = hasSecrets(s.auth) ? extractSecrets(s.auth) : null;
      const stored = await secretsGet(s.id);
      const secrets = { ...(stored ?? {}), ...(legacy ?? {}) };
      let migratedOk = true;
      if (legacy) {
        // 旧明文 → keyring：仅在写入确认成功后才允许落盘剥离；
        // 失败则保留明文（isKeyringReady 门控 partialize），防凭据蒸发
        migratedOk = await secretsSet(s.id, secrets);
        if (migratedOk) {
          log.info("已迁移明文凭据到系统凭据库:", s.id);
        } else {
          log.warn("keyring 写入失败，保留明文存储:", s.id);
        }
      }
      // 内存始终带凭据（消费方零改动）；仅当迁移成功（或无凭据）才生成"净"形态
      const clean = migratedOk ? stripSecrets(s) : s;
      return {
        ...clean,
        auth: { ...clean.auth, ...secrets } as ServerConfig["auth"],
      };
    }),
  );
  return merged;
}

function extractSecrets(auth: Partial<ServerConfig["auth"]> | undefined): ServerSecrets {
  const out: ServerSecrets = {};
  for (const k of SECRET_KEYS) {
    const v = auth?.[k];
    if (typeof v === "string" && v.length > 0) out[k] = v;
  }
  return out;
}
