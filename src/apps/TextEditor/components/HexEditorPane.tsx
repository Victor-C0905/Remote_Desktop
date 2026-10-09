/**
 * 十六进制编辑面板组件
 *
 * 用于查看二进制文件的十六进制视图
 * - hexdump 格式显示（地址 + 十六进制 + ASCII）
 * - 分页功能（每页 256 字节）
 * - 只读模式（不支持编辑）
 */

import { useState, useMemo } from 'react';
import '../TextEditor.css';

interface HexEditorPaneProps {
  /** 当前二进制数据 */
  data?: Uint8Array;

  /** 数据变更回调（预留接口，当前版本为只读查看） */
  onChange: (newData: Uint8Array) => void;
}

/** 每页字节数 */
const PAGE_SIZE = 256;

/** 每行字节数 */
const BYTES_PER_LINE = 16;

/**
 * 格式化单行十六进制输出
 *
 * @param offset - 行偏移量（字节数）
 * @param bytes - 当前行的字节数据
 * @returns 格式化的行文本（地址 + 十六进制 + ASCII）
 */
function formatHexLine(offset: number, bytes: Uint8Array): string {
  // 格式化地址（8 位十六进制，左侧补零）
  const address = offset.toString(16).padStart(8, '0');

  // 格式化十六进制部分（每字节 2 位十六进制，空格分隔）
  const hexParts: string[] = [];
  for (let i = 0; i < BYTES_PER_LINE; i++) {
    if (i < bytes.length) {
      hexParts.push(bytes[i].toString(16).padStart(2, '0'));
    } else {
      hexParts.push('  '); // 不足部分填充空格
    }
  }
  const hex = hexParts.join(' ');

  // 格式化 ASCII 部分（可显示字符 32-126，其他显示为 .）
  const asciiParts: string[] = [];
  for (let i = 0; i < bytes.length; i++) {
    const byte = bytes[i];
    if (byte >= 32 && byte <= 126) {
      asciiParts.push(String.fromCharCode(byte));
    } else {
      asciiParts.push('.');
    }
  }
  const ascii = asciiParts.join('');

  return `${address}  ${hex}  |${ascii}|`;
}

/**
 * 十六进制编辑面板组件
 *
 * 以 hexdump 格式显示二进制数据
 *
 * @param props - 组件属性
 * @returns 十六进制编辑面板 React 元素
 *
 * @example
 * ```tsx
 * const HexViewer = () => {
 *   const [data] = useState(new Uint8Array([0x48, 0x65, 0x6c, 0x6c, 0x6f]));
 *   return (
 *     <HexEditorPane
 *       data={data}
 *       onChange={() => {}}
 *     />
 *   );
 * };
 * ```
 */
export function HexEditorPane({ data, onChange: _onChange }: HexEditorPaneProps) {
  // 当前页偏移量（字节）
  const [offset, setOffset] = useState(0);

  // 计算总页数
  const totalPages = data ? Math.ceil(data.length / PAGE_SIZE) : 0;

  // 计算当前页数据
  const currentPageData = useMemo(() => {
    if (!data) return null;

    const start = offset;
    const end = Math.min(start + PAGE_SIZE, data.length);
    return data.slice(start, end);
  }, [data, offset]);

  // 生成分页后的行数据
  const lines = useMemo(() => {
    if (!currentPageData) return [];

    const result: string[] = [];
    for (let i = 0; i < currentPageData.length; i += BYTES_PER_LINE) {
      const lineBytes = currentPageData.slice(i, Math.min(i + BYTES_PER_LINE, currentPageData.length));
      const lineText = formatHexLine(offset + i, lineBytes);
      result.push(lineText);
    }
    return result;
  }, [currentPageData, offset]);

  // 上一页
  const handlePrevPage = () => {
    setOffset(Math.max(0, offset - PAGE_SIZE));
  };

  // 下一页
  const handleNextPage = () => {
    if (!data) return;
    const maxOffset = Math.floor(data.length / PAGE_SIZE) * PAGE_SIZE;
    setOffset(Math.min(maxOffset, offset + PAGE_SIZE));
  };

  // 无数据时显示提示
  if (!data || data.length === 0) {
    return (
      <div className="te-hex-pane">
        <div className="te-hex-empty">
          <span className="te-hex-empty-text">无数据显示</span>
        </div>
      </div>
    );
  }

  // 当前页码（从 1 开始）
  const currentPage = Math.floor(offset / PAGE_SIZE) + 1;

  // 是否可以翻页
  const canGoPrev = offset > 0;
  const canGoNext = offset + PAGE_SIZE < data.length;

  return (
    <div className="te-hex-pane">
      {/* 分页控制栏 */}
      <div className="te-hex-pagination">
        <button
          className="te-hex-pagination-btn"
          onClick={handlePrevPage}
          disabled={!canGoPrev}
          title="上一页"
        >
          ← 上一页
        </button>

        <span className="te-hex-pagination-info">
          第 {currentPage} / {totalPages} 页 ({offset + 1}-{Math.min(offset + PAGE_SIZE, data.length)} / {data.length} 字节)
        </span>

        <button
          className="te-hex-pagination-btn"
          onClick={handleNextPage}
          disabled={!canGoNext}
          title="下一页"
        >
          下一页 →
        </button>
      </div>

      {/* 十六进制内容区 */}
      <div className="te-hex-content">
        {lines.map((line, index) => (
          <div key={index} className="te-hex-line">
            {line}
          </div>
        ))}
      </div>
    </div>
  );
}