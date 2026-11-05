/**
 * 压缩格式与能力展示工具
 *
 * - archiveFormatId：从文件名推断探测协议中的格式 ID
 * - archiveFormatLabel：i18n 展示名
 * - installCommand：按发行版包管理器生成安装建议
 * - describeCapability：把结构化 reason_code 转成用户可读文案
 */

import i18n from "../i18n";
import type { ArchiveHostOs, ArchiveFormatCapability } from "../types/archive";

/** 从文件名推断格式 ID（与 Agent 探测矩阵一一对应） */
export function archiveFormatId(fileName: string): string | null {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".zip")) return "zip";
  if (lower.endsWith(".tar.zst")) return "tar.zst";
  if (lower.endsWith(".tar.bz2")) return "tar.bz2";
  if (lower.endsWith(".tar.xz")) return "tar.xz";
  if (/\.(tar\.gz|tgz)$/.test(lower)) return "tar.gz";
  if (lower.endsWith(".tar")) return "tar";
  if (lower.endsWith(".7z")) return "7z";
  if (lower.endsWith(".rar")) return "rar";
  return null;
}

/** 格式展示名（i18n 键用下划线，避免嵌套 key） */
export function archiveFormatLabel(id: string): string {
  return i18n.t(`archive.formats.${id.replace(/\./g, "_")}`);
}

/** 工具 → 各包管理器包名 */
const PACKAGE_BY_TOOL: Record<string, Record<string, string>> = {
  unzip: { apt: "unzip", dnf: "unzip", apk: "unzip", pacman: "unzip", zypper: "unzip", nix: "unzip" },
  tar: { apt: "tar", dnf: "tar", apk: "tar", pacman: "tar", zypper: "tar", nix: "gnutar" },
  "7z": { apt: "p7zip-full", dnf: "p7zip", apk: "p7zip", pacman: "7zip", zypper: "p7zip", nix: "p7zip" },
  "7zz": { apt: "7zip", dnf: "7zip", apk: "7zip", pacman: "7zip", zypper: "7zip", nix: "p7zip" },
  unrar: { apt: "unrar", dnf: "unrar", apk: "unrar", pacman: "unrar", zypper: "unrar", nix: "unrar" },
  gzip: { apt: "gzip", dnf: "gzip", apk: "gzip", pacman: "gzip", zypper: "gzip", nix: "gzip" },
  bzip2: { apt: "bzip2", dnf: "bzip2", apk: "bzip2", pacman: "bzip2", zypper: "bzip2", nix: "bzip2" },
  xz: { apt: "xz-utils", dnf: "xz", apk: "xz", pacman: "xz", zypper: "xz", nix: "xz" },
  zstd: { apt: "zstd", dnf: "zstd", apk: "zstd", pacman: "zstd", zypper: "zstd", nix: "zstd" },
};

/** 包管理器 → 安装命令模板 */
const INSTALL_COMMAND: Record<string, (pkg: string) => string> = {
  apt: (pkg) => `sudo apt install ${pkg}`,
  dnf: (pkg) => `sudo dnf install ${pkg}`,
  apk: (pkg) => `sudo apk add ${pkg}`,
  pacman: (pkg) => `sudo pacman -S ${pkg}`,
  zypper: (pkg) => `sudo zypper install ${pkg}`,
  nix: (pkg) => `nix profile install nixpkgs#${pkg}`,
};

/** 按发行版生成安装命令；无法识别包管理器时返回 null */
export function installCommand(os: ArchiveHostOs | undefined, tool: string | undefined): string | null {
  if (!os || !tool) return null;
  const pm = os.package_manager;
  const pkg = PACKAGE_BY_TOOL[tool]?.[pm];
  const builder = INSTALL_COMMAND[pm];
  if (!pkg || !builder) return null;
  return builder(pkg);
}

/** 能力不可用原因 → 用户文案（含安装建议） */
export function describeCapability(
  os: ArchiveHostOs | undefined,
  cap: ArchiveFormatCapability,
): string {
  const format = archiveFormatLabel(cap.id);
  if (cap.reason_code === "tool_missing" || cap.reason_code === "decompressor_missing") {
    const tool = cap.missing_tool ?? cap.tool ?? "?";
    const cmd = installCommand(os, tool);
    return cmd
      ? i18n.t("archive.capability.missingTool", { format, tool, command: cmd })
      : i18n.t("archive.capability.missingToolNoCommand", { format, tool });
  }
  if (cap.reason_code === "probe_timeout") {
    return i18n.t("archive.capability.probeTimeout", { format });
  }
  if (cap.reason_code === "platform_unsupported") {
    return i18n.t("archive.capability.platformUnsupported", { format });
  }
  return i18n.t("archive.capability.probeFailed", { format });
}

/** 已可用但只能查看（can_list=true / can_extract=false）的提示 */
export function partialSupportHint(cap: ArchiveFormatCapability): string | null {
  if (!cap.can_list || cap.can_extract_all) return null;
  return i18n.t("archive.capability.partial", {
    tool: cap.tool ?? "?",
  });
}
