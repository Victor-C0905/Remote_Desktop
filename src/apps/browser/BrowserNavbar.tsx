// src/apps/browser/BrowserNavbar.tsx
// 浏览器导航行：后退/前进/刷新 + 地址栏 + 设置下拉（Edge/Chrome 布局的第二行；
// 标签条是窗口标题栏本体，见 BrowserApp 的 titlebarSlot portal）
//
// 设置下拉：无痕开关 + 默认搜索引擎 + 在系统浏览器中打开。设置本体由
// Rust 按服务器持久化（browser_settings，settings.json），此处仅承载展示
// 与操作回调。
// "在系统浏览器中打开"：Rust browser_open_in_system 以 --proxy-server 挂
// 本会话 SOCKS5 隧道 + 独立 user-data-dir spawn 系统浏览器（Edge/Chrome），
// 流量与内置浏览器同路（服务器视角）——被墙站点在系统浏览器里同样可达。
// 不用 opener 插件：那走本地网络出口，语义不同。

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "../../stores/settingsStore";
import { SymbolicIcon } from "../../components/symbolic";
import { createLogger } from "../../utils/logger";
import type { TabInfo } from "./useBrowserTabs";
import { useBrowserSuggest } from "./useBrowserSuggest";

const log = createLogger("BrowserNavbar");

/** 浏览器设置（与 Rust BrowserSettings serde camelCase 对齐） */
export interface BrowserSettings {
  searchEngine: "google" | "bing";
  incognito: boolean;
}

/** 可选搜索引擎（品牌名不翻译，直接展示） */
const SEARCH_ENGINES: { id: BrowserSettings["searchEngine"]; label: string }[] = [
  { id: "google", label: "Google" },
  { id: "bing", label: "Bing" },
];

/**
 * 搜索结果页 URL → 搜索词的"美化显示"（Chrome 同款）：地址栏回显搜索词
 * 本身而非 https://www.bing.com/search?q=… 长串；再次回车等价于重搜该词。
 * 仅识别两大引擎的 /search?q= 形态，其余 URL 原样展示。
 */
function displayUrl(url: string): string {
  try {
    const u = new URL(url);
    if (/^(www\.)?(bing|google)\.com$/i.test(u.hostname) && u.pathname === "/search") {
      const q = u.searchParams.get("q");
      if (q) return q;
    }
  } catch {
    // 非法 URL 原样展示
  }
  return url;
}

interface BrowserNavbarProps {
  activeTab: TabInfo | null;
  /** 服务器 id：联想词经 Rust browser_suggest 取（隧道内请求），未连接时为 null */
  serverId: string | null;
  incognito: boolean;
  searchEngine: BrowserSettings["searchEngine"];
  onNavigate: (url: string) => void;
  onBack: () => void;
  onForward: () => void;
  onReload: () => void;
  onToggleIncognito: () => void;
  onSearchEngineChange: (engine: BrowserSettings["searchEngine"]) => void;
  /** 下拉开合上报（设置菜单或联想词任一展开即 true）：父层据此在展开期间隐藏 webview（child HWND 盖住 React 层） */
  onMenuOpenChange: (open: boolean) => void;
}

export function BrowserNavbar({
  activeTab,
  serverId,
  incognito,
  searchEngine,
  onNavigate,
  onBack,
  onForward,
  onReload,
  onToggleIncognito,
  onSearchEngineChange,
  onMenuOpenChange,
}: BrowserNavbarProps) {
  const { t } = useTranslation();

  // 地址栏本地草稿：失焦保留用户输入（不被页面事件覆盖），真实 URL 经
  // EvSource 回流后由下方 effect 自然接替显示（搜索结果页美化显示为搜索词，
  // 见 displayUrl）。focusedRef 供 effect 读取最新焦点态而无需入依赖——
  // 失焦瞬间 effect 不跑（依赖未变），草稿才得以保留
  const shownUrl = activeTab ? displayUrl(activeTab.url) : "";
  const [draft, setDraft] = useState(shownUrl);
  const [focused, setFocused] = useState(false);
  // 本聚焦周期内用户是否打过字：联想只对真实输入触发——失焦保留的草稿
  // 重新聚焦时原样显示、绝不据此自动弹下拉（否则下拉无输入翻转，连带
  // webview 隐藏/显示抖动，browser_set_visible 风暴，实测）
  const [typed, setTyped] = useState(false);
  const focusedRef = useRef(false);
  const addressRef = useRef<HTMLInputElement>(null);
  // 聚焦点击的全选与浏览器 mouseup caret 定位竞态：focus 里的 select() 会被
  // mouseup 默认行为（单击=caret 定位到点击处，点在末尾/空白即"全选瞬间还原
  // 成光标在最后"）覆盖，时序差异导致全选时有时无。wasFocusedRef 记录
  // mousedown 前的焦点态（mousedown handler 先于默认的 focus 转移执行），
  // 用于区分"本次点击引发聚焦"（补全选）与"焦点内再点"（放行原生 caret）
  const wasFocusedRef = useRef(false);
  const mouseDownPosRef = useRef<{ x: number; y: number } | null>(null);
  useEffect(() => {
    // 仅同 tab 内的页面事件（URL 回流）或切 tab 时跟随展示值；
    // 失焦本身不触发本 effect（focusedRef 不入依赖）
    if (!focusedRef.current) setDraft(shownUrl);
  }, [activeTab?.id, shownUrl]);

  const submit = (override?: string) => {
    // override：选中的联想词（纯文本，经 Rust normalize_url 落到搜索结果页）
    const value = (override ?? draft).trim();
    if (value) {
      onNavigate(value);
      // 回车导航后主动失焦（Chrome/Edge 同款：导航即离开地址栏聚焦）。
      // 失焦不重置草稿（见上方 effect 与 focusedRef），输入内容保留显示，
      // 真实 URL 回流后自然接替——旧实现失焦即用（此刻仍过期的）
      // activeTab.url 覆盖，表现为"回车后地址栏网址消失"
      addressRef.current?.blur();
    }
  };

  // 搜索联想：防抖拉取 + 竞态序号 + 键盘选中态（数据经 Rust 走 SOCKS5→QUIC 隧道）。
  // 联想仅对「本聚焦周期内的用户输入」触发（typed），不问 draft 是否等于
  // 展示值——草稿保留语义下二者不再等价（残留输入 ≠ 用户正在输入），
  // Chrome 同款：聚焦回显/保留内容本身不弹联想，打字才弹
  const suggestDraft = typed ? draft : "";
  const suggest = useBrowserSuggest(serverId, searchEngine, suggestDraft, focused);

  // 设置下拉/联想词下拉开合；任一展开期间点外部/Esc 关闭（联想词由 hook 管理）
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  // 开合上报（含挂载首帧 false：父层 set 相同值会 bail out，无多余渲染）。
  // 经由 effect 而非散落在各关闭路径，保证外部点击/Esc/切换标签等
  // 任何收起方式都会恢复 webview 可见性。任一展开即隐藏 webview
  useEffect(() => {
    onMenuOpenChange(menuOpen || suggest.open);
  }, [menuOpen, suggest.open, onMenuOpenChange]);
  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);

  return (
    <div className={`br-navbar${incognito ? " br-incognito" : ""}`}>
      <button type="button" className="br-nav-btn" title={t("browser.back")} onClick={onBack}>
        <SymbolicIcon name="go-previous" size={14} />
      </button>
      <button type="button" className="br-nav-btn" title={t("browser.forward")} onClick={onForward}>
        <SymbolicIcon name="go-next" size={14} />
      </button>
      <button type="button" className="br-nav-btn" title={t("browser.reload")} onClick={onReload}>
        <SymbolicIcon name="view-refresh" size={14} />
      </button>
      {/* 首页按钮 = 当前设置搜索引擎的官网（与默认首页/新标签页同源：
          Rust 侧 SearchEngine::home_url） */}
      <button
        type="button"
        className="br-nav-btn"
        title={t("browser.home")}
        onClick={() => onNavigate(searchEngine === "bing" ? "https://www.bing.com/" : "https://www.google.com/")}
      >
        <SymbolicIcon name="go-home" size={14} />
      </button>
      <div className="br-addressbar">
        {incognito && <SymbolicIcon name="view-private" size={13} className="br-incognito-badge" />}
        {activeTab?.loading && <span className="br-loading-dot" aria-hidden />}
        <input
          className="br-address"
          ref={addressRef}
          value={draft}
          // 关拼写检查：Chromium 默认对非词典词（URL/搜索词）画红色波浪
          // 标记线，失焦高亮消失后显露为"文字底部阴影"；地址栏内容本就
          // 不是自然语言，拼写检查无意义
          spellCheck={false}
          placeholder={t("browser.addressPlaceholder")}
          onChange={(e) => {
            setDraft(e.target.value);
            setTyped(true);
          }}
          onMouseDown={(e) => {
            // 此刻默认焦点转移尚未发生，focusedRef 即"聚焦前焦点态"
            wasFocusedRef.current = focusedRef.current;
            mouseDownPosRef.current = { x: e.clientX, y: e.clientY };
          }}
          onMouseUp={(e) => {
            const p = mouseDownPosRef.current;
            mouseDownPosRef.current = null;
            // 焦点内再点：Chrome 同款，caret 定位到点击处，不重选
            if (wasFocusedRef.current || !p) return;
            // 拖选（位移超阈值）：交给浏览器原生选择行为，不拦
            if (Math.abs(e.clientX - p.x) >= 3 || Math.abs(e.clientY - p.y) >= 3) return;
            // 聚焦单击：拦掉 mouseup 默认 caret 定位（它会覆盖 focus 里的
            // select()，见上方 wasFocusedRef 注释），重新全选保证一致
            e.preventDefault();
            e.currentTarget.select();
          }}
          onFocus={() => {
            focusedRef.current = true;
            // 新聚焦周期：仅清输入标记，草稿原样保留——不回显展示值，
            // 否则失焦保留的输入会在重新聚焦时被清掉（用户实测驳回）
            setTyped(false);
            setFocused(true);
            // 聚焦即全选（Chrome/Edge 行为），但延后一帧：聚焦瞬间恰有
            // pending 的草稿接替渲染（失焦期间 URL 回流的 setDraft）时，
            // 同步 select 的选区会被该渲染 commit 的 value 写入清掉
            // （caret 落末尾，表现为"全选瞬间还原"）——rAF 在所有
            // pending commit 之后执行，全选的是最终值。target 不能跨帧
            // 引用（合成事件 currentTarget 异步后为 null），用 ref
            requestAnimationFrame(() => addressRef.current?.select());
          }}
          onBlur={(e) => {
            focusedRef.current = false;
            setFocused(false);
            // 塌缩选区：全选后失焦，选区范围若保留，Chromium 会以灰色
            // inactive selection 背景衬出失焦选中文字（表现为"文字底部
            // 阴影"）。Chrome 地址栏失焦即塌缩同款；草稿显示不受影响，
            // 重新聚焦由 onFocus 的 rAF select 重新全选
            const el = e.currentTarget;
            const end = el.value.length;
            el.setSelectionRange(end, end);
          }}
          onKeyDown={(e) => {
            // 联想词键盘导航：↑↓ 环形选中、Esc 收起（同输入不再弹回）
            if (suggest.open) {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                suggest.moveActive(1);
                return;
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                suggest.moveActive(-1);
                return;
              }
              if (e.key === "Escape") {
                suggest.close();
                return;
              }
            }
            // Enter：导航后必须收起联想下拉——下拉展开期间 webview 被隐藏
            // （child HWND airspace），不收起则页面加载了也看不见，表现为
            // "回车不跳转，还停在联想词上"（实测）
            if (e.key === "Enter") {
              const picked =
                suggest.open && suggest.activeIndex >= 0
                  ? suggest.suggestions[suggest.activeIndex]
                  : undefined;
              if (picked !== undefined) {
                // 选中项 inline 回显（Chrome 同款）：地址栏立即替换为选中词
                suggest.closeWith(picked);
                setDraft(picked);
              } else {
                suggest.close();
              }
              submit(picked);
            }
          }}
        />
        {/* 搜索联想下拉（锚定地址栏；展开期间父层隐藏 webview 露出本面板） */}
        {suggest.open && (
          <div className="br-suggest" role="listbox">
            {suggest.suggestions.map((s, i) => (
              <div
                key={`${i}-${s}`}
                role="option"
                aria-selected={i === suggest.activeIndex}
                className={`br-suggest-item${i === suggest.activeIndex ? " is-active" : ""}`}
                // mousedown preventDefault：避免输入框先失焦（失焦即收起下拉，click 落空）。
                // 不挂 onMouseEnter 写选中态：悬停视觉由 CSS :hover 承担，
                // activeIndex 仅键盘 ↑↓ 驱动——否则鼠标恰好停在词表上时回车
                // 会误提交悬停项而非输入内容（实测）
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  // 选中项 inline 回显：地址栏立即替换为该词，再导航
                  suggest.closeWith(s);
                  setDraft(s);
                  submit(s);
                }}
              >
                {/* 第 0 项 = 本地模板项（Chrome Omnibox 同款）：显示"搜索「词」"
                    + 放大镜，选中即搜索该词（s 为纯词，文案仅展示层） */}
                {i === 0 ? (
                  <>
                    <SymbolicIcon name="system-search" size={12} />{" "}
                    {t("browser.searchFor", { q: s })}
                  </>
                ) : (
                  s
                )}
              </div>
            ))}
          </div>
        )}
      </div>
      {/* 设置下拉（导航行右端；锚定按钮容器定位） */}
      <div className="br-settings-wrap" ref={menuRef}>
        <button
          type="button"
          className={`br-nav-btn br-settings-btn${menuOpen ? " is-open" : ""}`}
          title={t("browser.settings")}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((o) => !o)}
        >
          <SymbolicIcon name="emblem-system" size={14} />
        </button>
        {menuOpen && (
          <div className="br-settings-menu" role="menu">
            <button
              type="button"
              role="menuitemcheckbox"
              aria-checked={incognito}
              className="br-settings-item"
              onClick={() => {
                // 无痕切换会关窗重开会话，菜单随会话重启收起
                setMenuOpen(false);
                onToggleIncognito();
              }}
            >
              <SymbolicIcon name="view-private" size={14} />
              <span className="br-settings-label">{t("browser.incognito")}</span>
              <span className={`br-switch${incognito ? " on" : ""}`} aria-hidden />
            </button>
            <div className="br-settings-divider" />
            {/* 在系统浏览器中打开：空白页/内部地址也允许——仅启动浏览器开新
                标签页（挂隧道），URL 仅在是网页地址时携带 */}
            <button
              type="button"
              role="menuitem"
              className="br-settings-item"
              disabled={!activeTab || !serverId}
              title={t("browser.openInSystemBrowser")}
              onClick={() => {
                const url = activeTab?.url;
                const target =
                  url && (url.startsWith("http://") || url.startsWith("https://")) ? url : undefined;
                setMenuOpen(false);
                // 后端按项目设置解析浏览器，以 --proxy-server 挂本会话隧道 spawn
                //（见文件头注释）；browserId 持久化于 settingsStore（旧版同款）
                invoke("browser_open_in_system", {
                  serverId,
                  browserId: useSettingsStore.getState().browserId,
                  url: target,
                }).catch((err) => log.warn("系统浏览器打开失败", target ?? "(新标签页)", err));
              }}
            >
              <SymbolicIcon name="external-link" size={14} />
              <span className="br-settings-label">{t("browser.openInSystemBrowser")}</span>
            </button>
            <div className="br-settings-divider" />
            <div className="br-settings-heading">{t("browser.searchEngine")}</div>
            {SEARCH_ENGINES.map((engine) => (
              <button
                key={engine.id}
                type="button"
                role="menuitemradio"
                aria-checked={searchEngine === engine.id}
                className="br-settings-item"
                onClick={() => onSearchEngineChange(engine.id)}
              >
                <span className="br-settings-label">{engine.label}</span>
                {searchEngine === engine.id && (
                  <SymbolicIcon name="emblem-ok" size={12} className="br-settings-check" />
                )}
              </button>
            ))}
            <div className="br-settings-hint">{t("browser.incognitoHint")}</div>
          </div>
        )}
      </div>
    </div>
  );
}
