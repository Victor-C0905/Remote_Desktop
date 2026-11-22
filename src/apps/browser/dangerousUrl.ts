/**
 * 危险地址 scheme 的前端拦截判据（审计 C-01）。
 *
 * - 朴素正则可被内嵌控制字符绕过（`java\tscript:`），先剥离
 *   [\x00-\x1f\x7f] 再匹配
 * - Rust normalize_url 是最终强制层（browser_window.rs），此处为纵深
 *   防御：脚本类 scheme 在产品语义上永远不应执行
 */
const DANGEROUS_SCHEME = /^(javascript|data|vbscript):/i;

export function isDangerousUrl(url: string): boolean {
  const stripped = url.replace(/[\x00-\x1f\x7f]/g, "").trim();
  return DANGEROUS_SCHEME.test(stripped);
}
