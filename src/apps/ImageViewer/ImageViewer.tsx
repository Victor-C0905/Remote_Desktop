/**
 * 远程图片查看器
 *
 * 数据流：remote_read_file_binary → base64 → data URI → <img> 浏览器原生解码
 * 功能：滚轮缩放、拖拽平移、适应窗口、1:1 原始尺寸
 * 支持：PNG/JPEG/GIF/WebP/BMP/ICO/SVG（浏览器原生解码格式）
 */

import { useState, useRef, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../utils/logger';
import './ImageViewer.css';

const log = createLogger('ImageViewer');

/** 预加载数据（FileManager 路由时传入） */
interface ImageViewerProps {
  windowId: string;
  preloadData?: {
    path: string;
    serverId: string;
    mimeType?: string;
  };
}

/** 二进制读取结果（与 Rust RemoteBinaryFile 对应） */
interface RemoteBinaryFile {
  path: string;
  base64: string;
  mtime: number;
  size: number;
}

export function ImageViewer({ preloadData }: ImageViewerProps) {
  const [dataUri, setDataUri] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // 视图变换状态
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  // 拖拽状态
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);

  const path = preloadData?.path;
  const serverId = preloadData?.serverId;
  const mimeType = preloadData?.mimeType || 'image/png';

  // 加载图片数据
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
        setDataUri(`data:${mimeType};base64,${result.base64}`);
        // 重置视图（新图片）
        setScale(1);
        setOffset({ x: 0, y: 0 });
      } catch (err) {
        if (cancelled) return;
        log.error('读取图片失败:', err);
        setError(`无法加载图片: ${err}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, [path, serverId, mimeType]);

  // 滚轮缩放（0.05x - 20x）
  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    setScale((s) => {
      const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      const next = s * factor;
      return Math.min(20, Math.max(0.05, next));
    });
  }, []);

  // 拖拽平移（mousedown 记录起点，window 级 mousemove/mouseup 保证拖出画布仍有效）
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    dragRef.current = { startX: e.clientX, startY: e.clientY, baseX: offset.x, baseY: offset.y };
  }, [offset]);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      setOffset({
        x: dragRef.current.baseX + (e.clientX - dragRef.current.startX),
        y: dragRef.current.baseY + (e.clientY - dragRef.current.startY),
      });
    };
    const handleMouseUp = () => { dragRef.current = null; };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  // 重置视图
  const resetView = useCallback(() => { setScale(1); setOffset({ x: 0, y: 0 }); }, []);

  return (
    <div className="iv-root">
      {loading && <div className="iv-status">加载中…</div>}
      {error && (
        <div className="iv-error">
          <p>{error}</p>
          <p className="iv-error-path">{path}</p>
        </div>
      )}
      {!loading && !error && dataUri && (
        <>
          <div className="iv-toolbar">
            <span className="iv-filename" title={path}>{path?.split('/').pop()}</span>
            <span className="iv-zoom-label">{Math.round(scale * 100)}%</span>
            <button className="iv-btn" onClick={resetView} title="重置视图">适应</button>
            <button className="iv-btn" onClick={() => setScale(1)} title="原始尺寸">1:1</button>
          </div>
          <div className="iv-canvas" onWheel={handleWheel} onMouseDown={handleMouseDown}>
            <img
              src={dataUri}
              alt={path ?? ''}
              className="iv-image"
              draggable={false}
              style={{
                transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})`,
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}
