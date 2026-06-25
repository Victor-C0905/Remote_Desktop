// src/window-system/layout/LayoutEngine.ts

import { Window } from '../core/Window';

/**
 * Layout engine base class
 * Defines interface for different window layout strategies
 */
export abstract class LayoutEngine {
  /**
   * Calculate window positions based on layout strategy
   * Returns a map of windowId -> { x, y, width, height }
   */
  abstract calculateWindows(
    windows: Window[],
    containerSize: { width: number; height: number }
  ): Map<string, { x: number; y: number; width: number; height: number }>;

  /**
   * Handle window resize event
   * Returns true if layout was affected
   */
  abstract handleResize(
    window: Window,
    delta: { x: number; y: number },
    containerSize: { width: number; height: number }
  ): boolean;

  /**
   * Handle window move event
   * Returns true if layout was affected (e.g., snap triggered)
   */
  abstract handleMove(
    window: Window,
    newPosition: { x: number; y: number },
    containerSize: { width: number; height: number }
  ): boolean;

  /**
   * Get layout type name
   */
  abstract get type(): 'free' | 'snap' | 'split';
}