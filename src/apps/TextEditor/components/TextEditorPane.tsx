/**
 * 文本编辑面板组件（CodeMirror 6 内核）
 *
 * 基于 CodeMirror 6 的编辑面板，提供大型项目级编辑体验：
 * - 行号 gutter、当前行高亮、括号匹配、选区匹配高亮
 * - 按扩展名动态加载语法高亮（@codemirror/language-data 按需分包）
 * - Ctrl+F 查找替换面板、Ctrl+G 跳转行（basicSetup 默认键位）
 * - 自动换行开关（Compartment 动态切换）
 * - 主题全部走 --quirel-* CSS 变量，亮暗主题自动跟随
 *
 * 数据流：
 * - 内部 doc 变更 → updateListener → onChange（同步 useFileManager）
 * - 外部 content 变化（文件加载完成）→ 与内部 doc 不一致时 dispatch 替换
 * - 光标/选区/文档统计 → onCursorChange / onStatsChange（状态栏展示）
 */

import { useEffect, useRef } from 'react';
import { Compartment, EditorState } from '@codemirror/state';import { EditorView, placeholder as cmPlaceholder } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';
import { basicSetup } from 'codemirror';
import { findLanguageForPath } from '../utils/language';
import '../TextEditor.css';

/**
 * 光标位置信息
 */
export interface CursorPosition {
  /** 行号（从 1 开始） */
  line: number;
  /** 列号（从 1 开始） */
  column: number;
}

/**
 * 文档统计信息（状态栏展示用）
 */
export interface DocStats {
  /** 总行数 */
  lines: number;
  /** 字符数 */
  chars: number;
  /** 当前选中的字符数 */
  selected: number;
}

interface TextEditorPaneProps {
  /** 当前文本内容（外部数据源，与内部 doc 双向同步） */
  content: string;

  /** 内容变更回调 */
  onChange: (newContent: string) => void;

  /** 光标位置变更回调（可选） */
  onCursorChange?: (position: CursorPosition) => void;

  /** 文档统计变更回调（行数/字符数/选中数，可选） */
  onStatsChange?: (stats: DocStats) => void;

  /** 文件路径（用于匹配语法高亮语言，可选） */
  filePath?: string;

  /** 是否自动换行（默认关，对齐 VS Code） */
  wrapText?: boolean;

  /** 文档为空时的占位提示（可选） */
  placeholder?: string;
}

/**
 * 查找面板/跳转行等内置 UI 的中文文案
 * （CodeMirror phrases 机制：按英文原文映射，未命中的键回退英文）
 */
const chinesePhrases = EditorState.phrases.of({
  Find: '查找',
  Replace: '替换',
  next: '下一个',
  previous: '上一个',
  all: '全部',
  'match case': '区分大小写',
  'by word': '整词匹配',
  regexp: '正则表达式',
  replace: '替换',
  'replace all': '全部替换',
  close: '关闭',
  'current match': '当前匹配',
  'replaced $ matches': '已替换 $ 个匹配',
  'go to line': '跳转到行',
  'Line:': '行号：',
});

/**
 * 编辑器主题：结构样式通过 EditorView.theme 注入（保证优先级），
 * 颜色全部引用 --quirel-* CSS 变量，随主题切换自动生效。
 */
const editorTheme = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--quirel-window-bg)',
    color: 'var(--quirel-text-primary)',
  },
  '.cm-scroller': {
    fontFamily: 'var(--font-mono)',
    fontSize: '15px',
    lineHeight: '1.6',
    overflow: 'auto',
  },
  '.cm-content': {
    caretColor: 'var(--quirel-accent-bg)',
    padding: '8px 0 40vh 0', /* 底部预留滚动空间（现代编辑器惯例） */
    tabSize: '4',
  },
  '&.cm-focused': { outline: 'none' },
  '.cm-cursor, .cm-dropCursor': {
    borderLeftColor: 'var(--quirel-accent-bg)',
    borderLeftWidth: '2px',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--quirel-window-bg)',
    color: 'var(--quirel-text-secondary)',
    borderRight: '1px solid var(--quirel-border-color)',
  },
  '.cm-lineNumbers .cm-gutterElement': {
    padding: '0 10px 0 14px',
    minWidth: '44px',
  },
  '.cm-activeLine': {
    backgroundColor: 'color-mix(in srgb, var(--quirel-accent-bg) 7%, transparent)',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'transparent',
    color: 'var(--quirel-text-primary)',
    fontWeight: '600',
  },
  '.cm-selectionBackground': {
    backgroundColor: 'color-mix(in srgb, var(--quirel-accent-bg) 20%, transparent) !important',
  },
  '.cm-selectionMatch': {
    backgroundColor: 'color-mix(in srgb, var(--quirel-accent-bg) 12%, transparent)',
  },
  '.cm-searchMatch': {
    backgroundColor: 'color-mix(in srgb, #f5c211 35%, transparent)',
    outline: '1px solid color-mix(in srgb, #f5c211 60%, transparent)',
  },
  '.cm-searchMatch.cm-searchMatch-selected': {
    backgroundColor: 'color-mix(in srgb, #f5c211 60%, transparent)',
  },
  /* 查找替换面板（Ctrl+F）：对齐 HeaderBar 底色与项目控件语言 */
  '.cm-panels': {
    backgroundColor: 'var(--quirel-headerbar-bg)',
    color: 'var(--quirel-text-primary)',
  },
  '.cm-panels-top': { borderBottom: '1px solid var(--quirel-border-color)' },
  '.cm-search': {
    padding: '6px 12px',
    fontFamily: 'var(--font-ui)',
    fontSize: '12px',
  },
  '.cm-textfield': {
    border: '1px solid var(--quirel-border-color)',
    borderRadius: 'var(--radius-xs)',
    background: 'var(--quirel-view-bg)',
    color: 'var(--quirel-text-primary)',
    fontFamily: 'var(--font-ui)',
    fontSize: '12px',
  },
  '.cm-button': {
    border: '1px solid var(--quirel-border-color)',
    borderRadius: 'var(--radius-xs)',
    background: 'transparent',
    color: 'var(--quirel-text-secondary)',
    fontFamily: 'var(--font-ui)',
    fontSize: '12px',
  },
  '.cm-button:hover': {
    background: 'var(--quirel-view-bg)',
    color: 'var(--quirel-text-primary)',
  },
  '.cm-foldPlaceholder': {
    background: 'transparent',
    border: 'none',
    color: 'var(--quirel-text-secondary)',
    padding: '0 6px',
    margin: '0 4px',
  },
  '.cm-tooltip': {
    border: '1px solid var(--quirel-border-color)',
    borderRadius: 'var(--radius-sm)',
    background: 'var(--quirel-view-bg)',
  },
  '.cm-placeholder': { color: 'var(--quirel-text-disabled)' },
});

/**
 * 语法高亮配色（Adwaita 语义化）：
 * - 跟随主题的令牌（accent / text-secondary）保证亮暗一致的可读性
 * - 固定常量色（橙/蓝）为 Adwaita 调色板中亮暗背景均通读的色值
 * - 函数/类型/变量保持默认前景色，保持 GNOME 编辑器的克制风格
 */
const semanticHighlighting = syntaxHighlighting(
  HighlightStyle.define([
    { tag: t.comment, color: 'var(--quirel-text-secondary)', fontStyle: 'italic' },
    {
      tag: [t.keyword, t.modifier, t.operatorKeyword, t.controlKeyword, t.moduleKeyword],
      color: 'var(--quirel-accent-bg)',
    },
    { tag: [t.string, t.special(t.string)], color: '#e66100' },
    { tag: [t.number, t.bool, t.null, t.atom], color: '#3584e4' },
    { tag: [t.heading], color: 'var(--quirel-accent-bg)', fontWeight: '700' },
    { tag: [t.link, t.url], color: '#3584e4', textDecoration: 'underline' },
    { tag: [t.invalid], color: '#c01c28' },
  ]),
  { fallback: true },
);

/**
 * 文本编辑面板组件
 *
 * 挂载时创建一次 EditorView；content/语言/换行等外部输入
 * 通过受控 effect 同步进 view，避免重建实例丢失撤销历史。
 *
 * @param props - 组件属性
 * @returns 文本编辑面板 React 元素
 *
 * @example
 * ```tsx
 * const Editor = () => {
 *   const [content, setContent] = useState('');
 *   return (
 *     <TextEditorPane
 *       content={content}
 *       onChange={setContent}
 *       filePath="/home/user/main.ts"
 *       wrapText={false}
 *     />
 *   );
 * };
 * ```
 */
export function TextEditorPane({
  content,
  onChange,
  onCursorChange,
  onStatsChange,
  filePath,
  wrapText = false,
  placeholder,
}: TextEditorPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  // 语言/换行通过 Compartment 动态切换，无需重建 EditorState
  const langCompartment = useRef(new Compartment());
  const wrapCompartment = useRef(new Compartment());

  // 回调通过 ref 透传，避免 updateListener 闭包过期
  const callbacksRef = useRef({ onChange, onCursorChange, onStatsChange });
  callbacksRef.current = { onChange, onCursorChange, onStatsChange };

  // 创建 EditorView（仅挂载时一次；doc 取挂载时刻的 content）
  useEffect(() => {
    if (!hostRef.current) return;

    const view = new EditorView({
      parent: hostRef.current,
      state: EditorState.create({
        doc: content,
        extensions: [
          basicSetup,
          chinesePhrases,
          editorTheme,
          semanticHighlighting,
          langCompartment.current.of([]),
          wrapCompartment.current.of(wrapText ? EditorView.lineWrapping : []),
          placeholder ? cmPlaceholder(placeholder) : [],
          EditorView.updateListener.of((update) => {
            if (!update.docChanged && !update.selectionSet) return;

            // 光标位置（行/列，均从 1 开始）
            const sel = update.state.selection.main;
            const line = update.state.doc.lineAt(sel.head);
            callbacksRef.current.onCursorChange?.({
              line: line.number,
              column: sel.head - line.from + 1,
            });

            // 文档统计（行数/字符数/选中字符数）
            callbacksRef.current.onStatsChange?.({
              lines: update.state.doc.lines,
              chars: update.state.doc.length,
              selected: sel.to - sel.from,
            });

            // 内容变更同步给外部数据源
            if (update.docChanged) {
              callbacksRef.current.onChange(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 仅挂载时创建一次
  }, []);

  // 外部 content 变化（文件加载完成等）：与内部 doc 不一致时整体替换
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    if (view.state.doc.toString() === content) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: content },
    });
  }, [content]);

  // 文件路径变化 → 匹配并异步加载语法包（按需分包，加载后切换 Compartment）
  useEffect(() => {
    let cancelled = false;
    const view = viewRef.current;
    if (!view) return;

    const desc = findLanguageForPath(filePath ?? '');
    if (!desc) {
      view.dispatch({ effects: langCompartment.current.reconfigure([]) });
      return;
    }

    desc
      .load()
      .then((lang) => {
        if (!cancelled && viewRef.current) {
          viewRef.current.dispatch({ effects: langCompartment.current.reconfigure(lang) });
        }
      })
      .catch((err) => {
        // 加载失败仅失去高亮，不影响编辑
        console.warn('[TextEditorPane] 语法包加载失败:', desc.name, err);
      });

    return () => {
      cancelled = true;
    };
  }, [filePath]);

  // 自动换行开关（Compartment 热切换）
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: wrapCompartment.current.reconfigure(
        wrapText ? EditorView.lineWrapping : [],
      ),
    });
  }, [wrapText]);

  return <div ref={hostRef} className="te-cm-host" />;
}
