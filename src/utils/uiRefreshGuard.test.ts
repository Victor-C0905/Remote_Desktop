import { describe, it, expect } from 'vitest';
import { isUiRefreshKey, installUiRefreshGuard } from './uiRefreshGuard';

const ev = (over: Partial<{ key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }> = {}) => ({
  key: '',
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  ...over,
});

describe('isUiRefreshKey', () => {
  it('F5 属于刷新键', () => {
    expect(isUiRefreshKey(ev({ key: 'F5' }))).toBe(true);
  });

  it('Ctrl+R / Meta+R 属于刷新键', () => {
    expect(isUiRefreshKey(ev({ key: 'r', ctrlKey: true }))).toBe(true);
    expect(isUiRefreshKey(ev({ key: 'R', ctrlKey: true }))).toBe(true);
    expect(isUiRefreshKey(ev({ key: 'r', metaKey: true }))).toBe(true);
  });

  it('Ctrl+Shift+R（强制刷新）属于刷新键', () => {
    expect(isUiRefreshKey(ev({ key: 'R', ctrlKey: true, shiftKey: true }))).toBe(true);
  });

  it('普通按键与无修饰 R 不算刷新键', () => {
    expect(isUiRefreshKey(ev({ key: 'r' }))).toBe(false);
    expect(isUiRefreshKey(ev({ key: 's', ctrlKey: true }))).toBe(false);
    expect(isUiRefreshKey(ev({ key: 'Enter' }))).toBe(false);
    expect(isUiRefreshKey(ev({ key: '' }))).toBe(false);
  });
});

describe('installUiRefreshGuard', () => {
  it('拦截刷新键的默认行为并阻止冒泡', () => {
    let prevented = '';
    const fakeTarget = {
      addEventListener: (_type: string, handler: (e: unknown) => void) => {
        handler({
          key: 'F5',
          ctrlKey: false,
          metaKey: false,
          shiftKey: false,
          preventDefault: () => {
            prevented = 'default';
          },
          stopPropagation: () => {
            prevented += '+propagation';
          },
        });
      },
    } as unknown as Window;

    installUiRefreshGuard(fakeTarget);
    expect(prevented).toBe('default+propagation');
  });

  it('非刷新键不干预', () => {
    let touched = false;
    const fakeTarget = {
      addEventListener: (_type: string, handler: (e: unknown) => void) => {
        handler({
          key: 'a',
          ctrlKey: false,
          metaKey: false,
          shiftKey: false,
          preventDefault: () => {
            touched = true;
          },
          stopPropagation: () => {
            touched = true;
          },
        });
      },
    } as unknown as Window;

    installUiRefreshGuard(fakeTarget);
    expect(touched).toBe(false);
  });
});
