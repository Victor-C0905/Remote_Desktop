/**
 * HTML 实体转义。
 *
 * 项目 i18next 全局配置 `escapeValue: false`（React 渲染路径由 JSX 转义兜底），
 * 因此任何「i18n 插值 → innerHTML」类非 React sink 必须先经此函数转义，
 * 否则远端可控字符串（错误消息、服务器路径等）可注入 HTML。
 */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
