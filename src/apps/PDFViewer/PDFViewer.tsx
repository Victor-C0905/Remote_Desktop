/**
 * 远程 PDF 阅读器
 *
 * 数据流：remote_read_file_binary → base64 → Uint8Array → pdfjs 渲染 canvas
 * 功能：分页导航（上一页/下一页/页码输入）
 * 渲染引擎：pdfjs-dist（浏览器内置 PDF 阅读器同源引擎）
 */

import { useState, useRef, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { PDFDocumentProxy } from 'pdfjs-dist';
import { createLogger } from '../../utils/logger';
import './PDFViewer.css';

const log = createLogger('PDFViewer');

/**
 * pdfjs 懒加载（模块级单例）
 *
 * 动态 import 两个收益：
 * 1. pdfjs（~1MB）按需加载，未打开 PDF 时不占用首屏包体
 * 2. `?url` worker 资源导入仅在运行时解析，避免测试环境（vitest）的静态解析失败
 */
let pdfjsModule: Promise<typeof import('pdfjs-dist')> | null = null;
function loadPdfjs(): Promise<typeof import('pdfjs-dist')> {
  if (!pdfjsModule) {
    pdfjsModule = (async () => {
      const pdfjs = await import('pdfjs-dist');
      // Vite 打包 worker 资源（CSP worker-src 'self' 允许）
      const { default: workerUrl } = await import('pdfjs-dist/build/pdf.worker.min.mjs?url');
      // 配置 pdfjs worker（必须在任何 getDocument 调用前执行）
      pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
      return pdfjs;
    })();
  }
  return pdfjsModule;
}

interface PDFViewerProps {
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

/** base64 → Uint8Array */
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function PDFViewer({ preloadData }: PDFViewerProps) {
  const [numPages, setNumPages] = useState(0);
  const [pageNum, setPageNum] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // pdfjs 文档代理（非 React 状态，避免 Proxy 干扰 pdfjs 内部机制）
  const docRef = useRef<PDFDocumentProxy | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  // 渲染任务引用（防止快速翻页时旧渲染覆盖新页面）
  const renderTaskRef = useRef<{ cancel: () => void; promise: Promise<void> } | null>(null);

  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  // 加载 PDF 文档
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
        const [result, pdfjs] = await Promise.all([
          invoke<RemoteBinaryFile>('remote_read_file_binary', { serverId, path }),
          loadPdfjs(),
        ]);
        if (cancelled) return;

        const data = base64ToBytes(result.base64);
        // getDocument 会 transfer 底层 buffer，必须复制避免再次渲染失败
        const doc = await pdfjs.getDocument({ data: data.slice() }).promise;
        if (cancelled) {
          doc.destroy();
          return;
        }
        docRef.current = doc;
        setNumPages(doc.numPages);
        setPageNum(1);
      } catch (err) {
        if (cancelled) return;
        log.error('加载 PDF 失败:', err);
        setError(`无法加载 PDF: ${err}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
      renderTaskRef.current?.cancel();
      renderTaskRef.current = null;
      docRef.current?.destroy();
      docRef.current = null;
    };
  }, [path, serverId]);

  // 渲染当前页
  useEffect(() => {
    const doc = docRef.current;
    const canvas = canvasRef.current;
    if (!doc || !canvas || pageNum < 1 || pageNum > doc.numPages) return;

    let cancelled = false;
    (async () => {
      try {
        // 取消上一次未完成的渲染任务（快速翻页保护）
        renderTaskRef.current?.cancel();

        const page = await doc.getPage(pageNum);
        if (cancelled) return;

        // 适配容器宽度（2 倍物理像素，HiDPI 清晰渲染）
        const container = canvas.parentElement;
        const containerWidth = container ? container.clientWidth : 800;
        const baseViewport = page.getViewport({ scale: 1 });
        const scale = (containerWidth / baseViewport.width) * 2;
        const viewport = page.getViewport({ scale });

        canvas.width = viewport.width;
        canvas.height = viewport.height;
        canvas.style.width = `${viewport.width / 2}px`;
        canvas.style.height = `${viewport.height / 2}px`;

        const task = page.render({
          canvas,
          canvasContext: canvas.getContext('2d')!,
          viewport,
        });
        renderTaskRef.current = task;
        await task.promise;
      } catch (err) {
        // RenderingCancelledException 是正常流程（翻页取消），不作为错误处理
        if (!cancelled && !(err instanceof Error && err.name === 'RenderingCancelledException')) {
          log.error('渲染 PDF 页面失败:', err);
        }
      }
    })();

    return () => { cancelled = true; };
  }, [pageNum, numPages, loading]);

  // 翻页操作
  const goPrev = useCallback(() => setPageNum((p) => Math.max(1, p - 1)), []);
  const goNext = useCallback(() => setPageNum((p) => Math.min(numPages, p + 1)), [numPages]);
  const handlePageInput = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const n = parseInt(e.target.value, 10);
    if (!Number.isNaN(n) && n >= 1 && n <= numPages) {
      setPageNum(n);
    }
  }, [numPages]);

  return (
    <div className="pv-root">
      {loading && <div className="pv-status">加载中…</div>}
      {error && (
        <div className="pv-error">
          <p>{error}</p>
          <p className="pv-error-path">{path}</p>
        </div>
      )}
      {!loading && !error && numPages > 0 && (
        <>
          <div className="pv-toolbar">
            <span className="pv-filename" title={path}>{path?.split('/').pop()}</span>
            <button className="pv-btn" onClick={goPrev} disabled={pageNum <= 1}>‹</button>
            <input
              className="pv-page-input"
              type="number"
              min={1}
              max={numPages}
              value={pageNum}
              onChange={handlePageInput}
            />
            <span className="pv-page-total">/ {numPages}</span>
            <button className="pv-btn" onClick={goNext} disabled={pageNum >= numPages}>›</button>
          </div>
          <div className="pv-canvas">
            <canvas ref={canvasRef} />
          </div>
        </>
      )}
    </div>
  );
}
