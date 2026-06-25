// src/window-system/events/WindowEvents.ts

import { WindowEventType } from '../types';

// Event type constants for easier usage
export const WINDOW_EVENTS: Record<WindowEventType, WindowEventType> = {
  'window:created': 'window:created',
  'window:closed': 'window:closed',
  'window:focused': 'window:focused',
  'window:minimized': 'window:minimized',
  'window:restored': 'window:restored',
  'window:resized': 'window:resized',
  'window:moved': 'window:moved',
  'window:layout-changed': 'window:layout-changed',
};