// src/apps/browser/useBrowserSuggest.ts
// 地址栏搜索联想的前端侧：防抖拉取 + 竞态序号 + 键盘选中态。
// 数据由 Rust 经 SOCKS5→QUIC 隧道取自搜索引擎 suggest 端点
// （proxy/suggest.rs，与页面浏览同出口 IP），本 hook 只承载 UI 时序。

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** 搜索引擎 id（与 BrowserSettings.searchEngine 结构对齐，独立声明避免循环依赖） */
export type SuggestEngine = "google" | "bing";

/** 防抖窗口：后端连接池消化打字并发（每主机多条热连接 + 池空短等归还，
 * 各查询均为 1×隧道 RTT）+ 连接预热（会话启动即建池），窗口可压到很低；
 * 40ms 仅合并快速连击，过期请求的结果由本 hook 的 seq 丢弃（后端不做
 * 请求级取消——abort 会连带杀掉池连接，实测为预热失效根因）。
 * 弹出感对齐 Chrome Omnibox（其防抖 ~50ms 且首字符即发） */
const SUGGEST_DEBOUNCE_MS = 40;

/**
 * URL 形输入粗判（仅用于决定是否拉联想词，导航判定仍在 Rust normalize_url）：
 * - 含空白 → 搜索词（如 "rust async book"）
 * - 内部 scheme（about:/javascript:/data: 等浏览器内部地址）→ URL，
 *   永不作为搜索词联想（实测教训：about:blank 无 // 无点无端口，
 *   三条 URL 规则全不命中，被当成搜索词发给了搜索引擎）
 * - scheme:// → URL（如 https://…）
 * - host.tld[:port][/path] → URL（如 docs.rs、localhost:3000）
 * 误判无害：把搜索词当 URL 只是少拉一次联想；把 URL 当搜索词只是多拉一次。
 */
export function looksLikeUrl(s: string): boolean {
  if (/\s/.test(s)) return false;
  if (/^(about|javascript|data|blob|file|view-source|chrome|edge|mocha):/i.test(s)) return true;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) return true;
  if (/^[^/\s]+\.[^/\s:]{2,}(:\d+)?(\/|$)/i.test(s)) return true;
  return /^[\w.-]+:\d+(\/.*)?$/i.test(s);
}

export interface BrowserSuggest {
  /** 当前联想词列表（仅最新请求的结果会落入） */
  suggestions: string[];
  /** 下拉是否展开（焦点内 + 未被 Esc 收起 + 有词） */
  open: boolean;
  /** 键盘选中下标（仅 ↑↓ 驱动；鼠标悬停不落此处——回车只提交键盘选中项，
   *  悬停视觉反馈由 CSS :hover 承担），-1 = 未选中 */
  activeIndex: number;
  /** 环形移动选中项（↑ / ↓） */
  moveActive: (delta: number) => void;
  /** Esc 收起：同输入不再弹回，输入变化后恢复 */
  close: () => void;
  /** 选中收起：与 close 同为收起，但以 text 为"收起基准"——调用方紧随其后
   * 把地址栏草稿替换为 text（选中项 inline 回显）时不会被视为新输入弹回，
   * 用户继续输入偏离 text 后恢复弹出 */
  closeWith: (text: string) => void;
}

export function useBrowserSuggest(
  serverId: string | null,
  engine: SuggestEngine,
  draft: string,
  focused: boolean,
): BrowserSuggest {
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [activeIndex, setActiveIndex] = useState(-1);
  // Esc 收起后保持关闭，直到输入变化（Chrome 行为：同输入不弹回）；
  // closeWith 以选中项为基准——回显替换不算新输入
  const [dismissed, setDismissed] = useState(false);
  // 收起时的草稿基准：null = Esc 收起（任何输入变化即弹回）；
  // 非 null = 选中收起（仅偏离该值才弹回）
  const closeBaselineRef = useRef<string | null>(null);
  // 竞态序号：快速输入时只有最新一次请求的结果允许落 state
  const seqRef = useRef(0);

  // 输入变化决定是否解除收起（Esc → 继续输入 → 重新弹出；选中回显不弹回）
  useEffect(() => {
    if (closeBaselineRef.current === null || draft !== closeBaselineRef.current) {
      closeBaselineRef.current = null;
      setDismissed(false);
    }
  }, [draft]);

  // 防抖拉取：焦点内 + 非 dismissed + 非 URL 形输入才请求。
  // 建议数组第 0 项恒为本地模板项（当前输入词本身，Chrome Omnibox 同款：
  // 输入瞬间即显示"搜索「query」"，网络结果随后追加——感知零延迟，独立
  // profile 的 Chrome 经同一隧道"秒出联想"的机制即此，其网络请求同样要
  // 付完整握手开销）
  useEffect(() => {
    if (!focused || dismissed || !serverId) return;
    const q = draft.trim();
    if (!q || looksLikeUrl(q)) {
      setSuggestions([]);
      return;
    }
    // 模板项立即落地（防抖前）：输入瞬间下拉即有"搜索「query」"可选，
    // 网络结果 40ms 后追加。open 依赖 suggestions.length，此项即弹出门票
    setSuggestions([q]);
    const seq = ++seqRef.current;
    const timer = setTimeout(() => {
      invoke<string[]>("browser_suggest", { serverId, engine, query: q })
        .then((list) => {
          if (seq === seqRef.current) setSuggestions([q, ...list]);
        })
        .catch(() => {
          if (seq === seqRef.current) setSuggestions([q]);
        });
    }, SUGGEST_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [draft, focused, dismissed, engine, serverId]);

  // 失焦清空：焦点恢复后由上方 effect 重新拉取
  useEffect(() => {
    if (!focused) {
      setSuggestions([]);
      setActiveIndex(-1);
    }
  }, [focused]);

  // 列表变化重置选中：新列表沿用旧下标会指向无关词条
  useEffect(() => {
    setActiveIndex(-1);
  }, [suggestions]);

  const moveActive = useCallback(
    (delta: number) => {
      const n = suggestions.length;
      if (!n) return;
      // 取模环形：首项↑到末项、末项↓回首项（Chrome 同款）
      setActiveIndex((i) => (((i + delta) % n) + n) % n);
    },
    [suggestions.length],
  );
  const close = useCallback(() => {
    closeBaselineRef.current = null;
    setDismissed(true);
  }, []);
  const closeWith = useCallback((text: string) => {
    closeBaselineRef.current = text;
    setDismissed(true);
  }, []);

  return {
    suggestions,
    open: focused && !dismissed && suggestions.length > 0,
    activeIndex,
    moveActive,
    close,
    closeWith,
  };
}
