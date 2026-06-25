// src/window-system/layout/SnapLayout.ts

import { LayoutEngine } from './LayoutEngine';
import { Window } from '../core/Window';

/**
 * Snap layout - windows snap to screen edges
 * When a window is dragged near an edge, it automatically
 * resizes to fill half the screen
 */
export class SnapLayout extends LayoutEngine {
  private snapThreshold = 20; // pixels from edge to trigger snap

  get type(): 'snap' {
    return 'snap';
  }

  calculateWindows(
    windows: Window[],
    containerSize: { width: number; height: number }
  ): Map<string, { x: number; y: number; width: number; height: number }> {
    const positions = new Map();
    
    windows.forEach(window => {
      // Check if window should be snapped
      const snapState = this.getSnapState(window, containerSize);
      
      if (snapState) {
        positions.set(window.id, this.calculateSnapPosition(snapState, containerSize));
      } else {
        positions.set(window.id, {
          x: window.position.x,
          y: window.position.y,
          width: window.size.width,
          height: window.size.height,
        });
      }
    });
    
    return positions;
  }

  handleResize(
    _window: Window,
    _delta: { x: number; y: number },
    _containerSize: { width: number; height: number }
  ): boolean {
    // Snap layout doesn't affect other windows on resize
    return false;
  }

  handleMove(
    window: Window,
    newPosition: { x: number; y: number },
    containerSize: { width: number; height: number }
  ): boolean {
    // Check if window should snap
    const snapState = this.checkSnapTrigger(newPosition, containerSize);
    
    if (snapState) {
      // Update window position to snapped position
      const snappedPosition = this.calculateSnapPosition(snapState, containerSize);
      window.setPosition({ x: snappedPosition.x, y: snappedPosition.y });
      window.setSize({ width: snappedPosition.width, height: snappedPosition.height });
      return true;
    }
    
    return false;
  }

  /**
   * Get current snap state for a window
   */
  private getSnapState(
    window: Window,
    containerSize: { width: number; height: number }
  ): 'left' | 'right' | 'top' | 'bottom' | null {
    const { x, y } = window.position;
    const { width, height } = window.size;
    
    // Check left edge snap
    if (x <= this.snapThreshold && y > this.snapThreshold && y + height < containerSize.height - this.snapThreshold) {
      return 'left';
    }
    
    // Check right edge snap
    if (x + width >= containerSize.width - this.snapThreshold && y > this.snapThreshold && y + height < containerSize.height - this.snapThreshold) {
      return 'right';
    }
    
    // Check top edge snap
    if (y <= this.snapThreshold && x > this.snapThreshold && x + width < containerSize.width - this.snapThreshold) {
      return 'top';
    }
    
    // Check bottom edge snap
    if (y + height >= containerSize.height - this.snapThreshold && x > this.snapThreshold && x + width < containerSize.width - this.snapThreshold) {
      return 'bottom';
    }
    
    return null;
  }

  /**
   * Check if a position should trigger snap
   */
  private checkSnapTrigger(
    position: { x: number; y: number },
    containerSize: { width: number; height: number }
  ): 'left' | 'right' | 'top' | 'bottom' | null {
    const { x, y } = position;
    
    if (x <= this.snapThreshold) return 'left';
    if (x >= containerSize.width - this.snapThreshold) return 'right';
    if (y <= this.snapThreshold) return 'top';
    if (y >= containerSize.height - this.snapThreshold) return 'bottom';
    
    return null;
  }

  /**
   * Calculate snapped window position
   */
  private calculateSnapPosition(
    snapState: 'left' | 'right' | 'top' | 'bottom',
    containerSize: { width: number; height: number }
  ): { x: number; y: number; width: number; height: number } {
    switch (snapState) {
      case 'left':
        return { x: 0, y: 0, width: containerSize.width / 2, height: containerSize.height };
      case 'right':
        return { x: containerSize.width / 2, y: 0, width: containerSize.width / 2, height: containerSize.height };
      case 'top':
        return { x: 0, y: 0, width: containerSize.width, height: containerSize.height / 2 };
      case 'bottom':
        return { x: 0, y: containerSize.height / 2, width: containerSize.width, height: containerSize.height / 2 };
    }
  }
}