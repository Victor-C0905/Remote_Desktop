import { useMemo, type ReactNode } from "react";
import { TerminalSkeleton } from "../components/skeleton/TerminalSkeleton";
import { MonitorSkeleton } from "../components/skeleton/MonitorSkeleton";

/** 支持的骨架屏变体类型 */
export type SkeletonVariant = 'terminal' | 'monitor';

interface UseDataGateOptions {
  /** 数据是否已就绪，由调用方根据自身状态判断传入 */
  ready: boolean;
  /** 骨架屏类型，决定展示哪种布局的骨架屏 */
  variant?: SkeletonVariant;
}

interface UseDataGateReturn {
  /** 是否应展示真实内容（true）或隐藏内容等待加载（false） */
  showContent: boolean;
  /** 对应类型的骨架屏 JSX 元素 */
  skeleton: ReactNode;
}

/**
 * 通用数据门控 Hook
 *
 * 用于 Terminal / SystemMonitor 等组件内部的数据加载门控。
 * 当 ready=false 时返回对应类型的骨架屏 JSX，
 * ready=true 时 showContent=true，调用方据此切换真实内容的渲染。
 *
 * HeaderBar / StatusBar 等纯静态部分不受此门控影响，始终渲染。
 *
 * @example
 * ```tsx
 * const { showContent, skeleton } = useDataGate({ ready: xtermAvailable, variant: 'terminal' });
 *
 * return (
 *   <div>
 *     <HeaderBar />
 *     {showContent ? <TerminalContent /> : skeleton}
 *     <StatusBar />
 *   </div>
 * );
 * ```
 */
export function useDataGate(options: UseDataGateOptions): UseDataGateReturn {
  const { ready, variant = 'terminal' } = options;

  const skeleton = useMemo<ReactNode>(() => {
    switch (variant) {
      case 'monitor':
        return <MonitorSkeleton />;
      case 'terminal':
      default:
        return <TerminalSkeleton />;
    }
  }, [variant]);

  return {
    showContent: ready,
    skeleton,
  };
}
