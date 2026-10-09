/**
 * 文本编辑面板组件
 *
 * 简单的文本编辑器面板，提供基础的文本编辑功能
 * - 自动调整高度（根据内容）
 * - 禁用拼写检查
 * - 禁用 resize（用户不能手动调整大小）
 * - 光标位置跟踪（行号、列号）
 * - 无语法高亮、行号等高级功能
 */

import { useRef, useEffect } from 'react';
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

interface TextEditorPaneProps {
  /** 当前文本内容 */
  content: string;

  /** 内容变更回调 */
  onChange: (newContent: string) => void;

  /** 光标位置变更回调（可选） */
  onCursorChange?: (position: CursorPosition) => void;
}

/**
 * 文本编辑面板组件
 *
 * 基础的文本编辑器，使用 textarea 元素实现
 * 自动调整高度以适应内容，并跟踪光标位置
 *
 * @param props - 组件属性
 * @returns 文本编辑面板 React 元素
 *
 * @example
 * ```tsx
 * const Editor = () => {
 *   const [content, setContent] = useState('');
 *   const [cursorPos, setCursorPos] = useState({ line: 1, column: 1 });
 *   return (
 *     <TextEditorPane
 *       content={content}
 *       onChange={setContent}
 *       onCursorChange={setCursorPos}
 *     />
 *   );
 * };
 * ```
 */
export function TextEditorPane({ content, onChange, onCursorChange }: TextEditorPaneProps) {
  // 引用 textarea 元素
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  /**
   * 计算光标位置
   *
   * 根据光标在文本中的位置，计算行号和列号
   *
   * @param textarea - textarea 元素
   * @returns 光标位置信息（行号、列号）
   */
  const calculateCursorPosition = (textarea: HTMLTextAreaElement): CursorPosition => {
    const cursorIndex = textarea.selectionStart;
    const text = textarea.value;

    // 计算行号：光标之前的换行符数量 + 1
    let line = 1;
    let column = 1;
    let lastNewlineIndex = -1;

    for (let i = 0; i < cursorIndex; i++) {
      if (text[i] === '\n') {
        line++;
        lastNewlineIndex = i;
      }
    }

    // 计算列号：光标位置 - 最后一个换行符位置
    column = cursorIndex - lastNewlineIndex;

    return { line, column };
  };

  /**
   * 处理光标位置变更
   *
   * 当光标移动时，计算新的光标位置并触发回调
   */
  const handleCursorChange = () => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    const newPosition = calculateCursorPosition(textarea);

    // 触发回调（如果提供）
    if (onCursorChange) {
      onCursorChange(newPosition);
    }
  };

  /**
   * 自动调整 textarea 高度
   *
   * 根据内容自动调整高度，确保所有内容可见
   * 使用 scrollHeight 计算实际内容高度
   */
  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    // 重置高度以获取正确的 scrollHeight
    textarea.style.height = '0px';

    // 设置新高度（内容高度 + padding）
    const scrollHeight = textarea.scrollHeight;
    textarea.style.height = `${scrollHeight}px`;
  }, [content]); // 依赖 content，内容变更时重新计算高度

  return (
    <div className="te-text-pane">
      <textarea
        ref={textareaRef}
        className="te-textarea"
        value={content}
        onChange={(e) => {
          onChange(e.target.value);
          // 内容变更时也更新光标位置
          handleCursorChange();
        }}
        onKeyUp={handleCursorChange} // 键盘导航（上下左右、Home/End）
        onClick={handleCursorChange} // 鼠标点击
        onSelect={handleCursorChange} // 选择文本时
        spellCheck={false} // 禁用拼写检查
        // 禁用 resize（用户不能手动调整大小）
        // 注意：CSS 中也设置了 resize: none，这里作为补充
      />
    </div>
  );
}