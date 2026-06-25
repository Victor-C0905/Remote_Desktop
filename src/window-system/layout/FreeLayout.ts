// src/window-system/layout/FreeLayout.ts

import { LayoutEngine } from './LayoutEngine';
import { Window } from '../core/Window';

/**
 * Free layout - windows can be positioned anywhere
 * This is the default layout (current behavior)
 */
export class FreeLayout extends LayoutEngine {
  get type(): 'free' {
    return 'free';
  }

  calculateWindows(
    windows: Window[],
    _containerSize: { width: number; height: number }
  ): Map<string, { x: number; y: number; width: number; height: number }> {
    const positions = new Map();

    windows.forEach(window => {
      positions.set(window.id, {
        x: window.position.x,
        y: window.position.y,
        width: window.size.width,
        height: window.size.height,
      });
    });

    return positions;
  }

  handleResize(
    _window: Window,
    _delta: { x: number; y: number },
    _containerSize: { width: number; height: number }
  ): boolean {
    // Free layout doesn't affect other windows on resize
    return false;
  }

  handleMove(
    _window: Window,
    _newPosition: { x: number; y: number },
    _containerSize: { width: number; height: number }
  ): boolean {
    // Free layout doesn't affect other windows on move
    return false;
  }
}