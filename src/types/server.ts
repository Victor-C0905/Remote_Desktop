/**
 * 服务器认证相关类型定义
 *
 * 支持多种SSH认证方式：密码、公钥、键盘交互
 */

/**
 * 认证方式枚举
 *
 * 对应SSH协议的认证方法
 */
export enum AuthMethod {
  /** 密码认证 - 使用用户名和密码 */
  PASSWORD = 'password',
  /** 公钥认证 - 使用SSH私钥 */
  PUBKEY = 'pubkey',
}

/**
 * 认证凭据接口
 *
 * 包含认证所需的所有信息，根据method选择填充相应字段
 */
export interface AuthCredentials {
  /** 认证方式 */
  method: AuthMethod;
  /** 用户名 */
  username: string;
  /** 密码（密码认证时使用，加密存储） */
  password?: string;
  /** SSH私钥内容（公钥认证时使用，加密存储） */
  privateKey?: string;
  /** 私钥密码（可选，用于加密的私钥） */
  passphrase?: string;
}

/**
 * 结构化连接错误（客户端分类，code 为 AuthErrorCode 数值）
 * 旧格式为纯字符串，过渡期两种并存
 */
export interface StructuredError {
  /** 错误码（AuthErrorCode 数值，见 src/types/errors.ts） */
  code: number;
  /** 错误细节（服务器名等上下文） */
  detail?: string;
}

/**
 * 服务器配置接口
 *
 * 包含服务器连接和认证的完整配置
 */
export interface ServerConfig {
  /** 服务器唯一标识 */
  id: string;
  /** 服务器显示名称 */
  name: string;
  /** 服务器主机地址 */
  host: string;
  /** 服务器端口 */
  port: number;
  /** 认证凭据 */
  auth: AuthCredentials;
  /** 连接令牌（可选，用于QUIC连接） */
  token?: string;
  /** 最后连接时间戳 */
  lastConnected?: number;
  /**
   * 连接状态
   * - reconnecting: 网络断开后自动重连中（最多 3 次、5 秒间隔）
   */
  status: 'connected' | 'disconnected' | 'connecting' | 'reconnecting' | 'error';
  /** 错误信息（当status为error时；结构化对象或旧格式字符串） */
  error?: StructuredError | string;
  /** 往返时延（毫秒） */
  rttMs?: number;
  /** 服务器证书指纹（SHA-256，证书钉扎，SSH known_hosts 模式） */
  certFingerprint?: string;
  // ── quireld 自动供应（SSH 自动安装）扩展字段，全部可选不破坏 servers.bin 结构 ──
  /** SSH 端口（自动安装/管理通道使用，默认 22） */
  sshPort?: number;
  /** quireld QUIC 端口（自动安装时探测/写入的值） */
  quireldPort?: number;
  /** 自动安装完成时间戳 */
  provisionedAt?: number;
  /** 安装方式：auto=SSH 自动安装 / manual=手动命令 */
  provisionMethod?: 'auto' | 'manual';
  /** 最近一次确认的 quireld 版本 */
  quireldVersion?: string;
  /** SSH host key 指纹（OpenSSH SHA256:base64，自动安装 TOFU 记录） */
  sshHostKeyFingerprint?: string;
}