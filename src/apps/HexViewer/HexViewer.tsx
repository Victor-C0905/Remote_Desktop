/**
 * 十六进制查看器（未知格式回退）
 *
 * 数据流：remote_read_file_binary → base64 → Uint8Array → hexdump 视图
 * 只加载前 64KB（大文件全量 hexdump 无意义，浏览器渲染不动）
 * 显示格式对齐 hexdump -C：偏移 + 16 字节十六进制 + ASCII 列
 */

import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../utils/logger';
import './HexViewer.css';

const log = createLogger('HexViewer');

/** 最多加载的字节数（64KB） */
const MAX_BYTES = 64 * 1024;
/** 每行字节数 */
const BYTES_PER_LINE = 16;

interface HexViewerProps {
  windowId: string;
  preloadData?: {
    path: string;
    serverId: string;
  };
}

interface RemoteBinaryFile {
  path: string;
  base64: string;
  mtime: number;
  size: number;
}

/** 单行 hexdump 数据 */
interface HexLine {
  offset: number;
  hex: string;
  ascii: string;
}

/** Uint8Array → hexdump 行数组（截断到 MAX_BYTES） */
function buildHexLines(bytes: Uint8Array): HexLine[] {
  const lines: HexLine[] = [];
  const total = Math.min(bytes.length, MAX_BYTES);
  for (let base = 0; base < total; base += BYTES_PER_LINE) {
    const end = Math.min(base + BYTES_PER_LINE, total);
    const lineBytes = bytes.slice(base, end);

    // 十六进制列（不足 16 字节右侧留空对齐）
    const hexParts: string[] = [];
    for (let i = 0; i < BYTES_PER_LINE; i++) {
      hexParts.push(i < lineBytes.length ? lineBytes[i].toString(16).padStart(2, '0') : '  ');
    }

    // ASCII 列（可显示字符 32-126，其余显示 .）
    let ascii = '';
    for (let i = 0; i < lineBytes.length; i++) {
      const b = lineBytes[i];
      ascii += b >= 32 && b <= 126 ? String.fromCharCode(b) : '.';
    }

    lines.push({ offset: base, hex: hexParts.join(' '), ascii });
  }
  return lines;
}

/** base64 → Uint8Array */
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function HexViewer({ preloadData }: HexViewerProps) {
  const [lines, setLines] = useState<HexLine[]>([]);
  const [fileSize, setFileSize] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  useEffect(() => {
    if (!path || !serverId) {
      setError('未指定文件或未连接服务器');
      setLoading(false);
      return;
    }

    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const result = await invoke<RemoteBinaryFile>('remote_read_file_binary', { serverId, path });
        if (cancelled) return;
        const bytes = base64ToBytes(result.base64);
        setLines(buildHexLines(bytes));
        setFileSize(result.size);
      } catch (err) {
        if (cancelled) return;
        log.error('读取文件失败:', err);
        setError(`无法读取文件: ${err}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, [path, serverId]);

  // 文件超出展示范围的提示
  const truncated = useMemo(() => fileSize > MAX_BYTES, [fileSize]);

  return (
    <div className="hv-root">
      {loading && <div className="hv-status">加载中…</div>}
      {error && (
        <div className="hv-error">
          <p>{error}</p>
          <p className="hv-error-path">{path}</p>
        </div>
      )}
      {!loading && !error && (
        <>
          <div className="hv-toolbar">
            <span className="hv-filename" title={path}>{path?.split('/').pop()}</span>
            <span className="hv-size">{fileSize.toLocaleString()} 字节</span>
          </div>
          <div className="hv-content">
            {lines.map((line) => (
              <div key={line.offset} className="hv-line">
                <span className="hv-offset">{line.offset.toString(16).padStart(8, '0')}</span>
                <span className="hv-hex">{line.hex}</span>
                <span className="hv-ascii">|{line.ascii}|</span>
              </div>
            ))}
            {truncated && (
              <div className="hv-truncated">
                … 仅显示前 {MAX_BYTES / 1024} KB（共 {fileSize.toLocaleString()} 字节）
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
