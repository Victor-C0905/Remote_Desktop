/**
 * 文本编辑面板组件
 *
 * 简单的文本编辑器面板，提供基础的文本编辑功能
 * - 自动调整高度（根据内容）
 * - 禁用拼写检查
 * - 禁用 resize（用户不能手动调整大小）
 * - 无语法高亮、行号等高级功能
 */

import { useRef, useEffect } from 'react';
import '../TextEditor.css';

interface TextEditorPaneProps {
  /** 当前文本内容 */
  content: string;

  /** 内容变更回调 */
  onChange: (newContent: string) => void;
}

/**
 * 文本编辑面板组件
 *
 * 基础的文本编辑器，使用 textarea 元素实现
 * 自动调整高度以适应内容
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
 *     />
 *   );
 * };
 * ```
 */
export function TextEditorPane({ content, onChange }: TextEditorPaneProps) {
  // 引用 textarea 元素
  const textareaRef = useRef<HTMLTextAreaElement>(null);

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
        onChange={(e) => onChange(e.target.value)}
        spellCheck={false} // 禁用拼写检查
        // 禁用 resize（用户不能手动调整大小）
        // 注意：CSS 中也设置了 resize: none，这里作为补充
      />
    </div>
  );
}