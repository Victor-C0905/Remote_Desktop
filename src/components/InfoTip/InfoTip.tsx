import { SymbolicIcon } from "../symbolic";
import "./InfoTip.css";

/**
 * InfoTip — 行内信息提示（ⓘ 图标，悬停/聚焦显示原生 tooltip）
 *
 * 信息架构规范（R11 体验整改）：界面不放多行说明文字墙——一行能说清的
 * 短提示内联；背景知识、安全注意事项、多行解释收进 InfoTip，由用户主动
 * 悬停查看。原生 title 支持多行文本，零依赖，跟随系统延迟与主题样式。
 *
 * - 放在标签/标题文本右侧，勿单独成段
 * - tip 文案按「结论先行、逐行一个事实」组织（原生 tooltip 不做富排版）
 */
export function InfoTip({ tip }: { tip: string }) {
  return (
    <span
      className="info-tip"
      tabIndex={0}
      role="note"
      aria-label={tip}
      title={tip}
    >
      <SymbolicIcon name="dialog-information" size={14} />
    </span>
  );
}
