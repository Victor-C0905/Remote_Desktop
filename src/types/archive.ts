/**
 * 压缩能力探测类型（与 quirel-protocol/src/archive.rs 字段一一对应）
 *
 * 协议字段为 snake_case，经 Tauri 原样返回，前端不做驼峰转换。
 */

export interface ArchiveHostOs {
  id: string;
  version: string;
  package_manager: string;
}

export interface ArchiveTool {
  name: string;
  path: string;
  version: string;
}

export interface ArchiveFormatCapability {
  id: string;
  can_list: boolean;
  can_extract_one: boolean;
  can_extract_all: boolean;
  tool?: string;
  reason_code?: string;
  missing_tool?: string;
  detail?: string;
}

export interface ArchiveCapabilities {
  probed_at: number;
  os: ArchiveHostOs;
  tools: ArchiveTool[];
  formats: ArchiveFormatCapability[];
}
