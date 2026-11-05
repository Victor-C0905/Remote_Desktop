/**
 * 解压资源配额（基于列表元数据的预检）
 *
 * 用途：在真正执行解压前阻止明显异常的压缩包，降低解压炸弹/磁盘耗尽风险。
 * 注意：列表中的大小可被恶意构造，最终仍需 Agent 侧复核（后续 staging 阶段）。
 */
export const MAX_EXTRACT_ENTRIES = 100_000;          // 条目数上限
export const MAX_EXTRACT_BYTES = 100 * 1024 ** 3;    // 声明总解压大小上限：100 GB

/** 单次“筛选提取”最多选中的文件数（避免 argv 超过系统上限） */
export const MAX_SELECTED_ENTRIES = 200;
