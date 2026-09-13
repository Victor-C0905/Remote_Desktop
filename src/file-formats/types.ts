/**
 * 文件格式系统类型定义
 *
 * 四层架构：检测层（插件 + 注册表）→ 网关层（FileOpener）→ 渲染层（各应用）→ 数据层（file_info 协议）
 * 本文件是检测层的类型单一真相源。
 */

/** Agent file_info 命令返回的文件探测结果（与 Rust RemoteFileInfo 对应，camelCase） */
export interface RemoteFileInfo {
  path: string;
  /** 文件大小（字节） */
  size: number;
  isDir: boolean;
  /** Agent 端启发式判定是否为文本（BOM/NUL/控制字符比例） */
  isText: boolean;
  /** 小写扩展名，不含点；无扩展名为空串 */
  extension: string;
  /** 文件头部字节（前 512 字节），用于 magic number 检测 */
  magicBytes: number[];
}

/** 格式分类（决定路由到哪个应用/流程） */
export type FormatCategory =
  | 'image'
  | 'pdf'
  | 'text'
  | 'hex'
  | 'archive'          // 压缩包 → 解压流程（FileManager，服务器原生命令）
  | 'browser-local'    // HTML → 下载到本地用系统浏览器打开
  | 'run-script';      // 脚本 → 终端自动执行（类 Windows 双击运行脚本）

/** 单个插件的检测结果 */
export interface FormatMatch {
  /** 命中的插件 ID */
  pluginId: string;
  category: FormatCategory;
  /** 置信度 0-1（magic 命中 > 扩展名命中） */
  confidence: number;
  /** 图片类的 MIME 类型（data URI 用），其他类为空 */
  mimeType?: string;
}

/** 格式检测插件接口（检测层扩展点：新格式 = 新插件文件） */
export interface FileFormatPlugin {
  /** 插件唯一 ID（如 'image'、'pdf'） */
  id: string;
  /**
   * 检测文件格式
   * @param info Agent 返回的探测信息（magicBytes + 扩展名双信号）
   * @returns 命中返回 FormatMatch；不匹配返回 null
   */
  detect(info: RemoteFileInfo): FormatMatch | null;
}
