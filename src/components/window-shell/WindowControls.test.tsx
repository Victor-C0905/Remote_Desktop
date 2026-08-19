// src/components/window-shell/WindowControls.test.tsx
// 验证窗口控制按钮与标题栏拖拽的事件隔离

import { render, fireEvent } from '@testing-library/react';
import { describe, test, expect, vi } from 'vitest';
import { WindowControls } from './WindowControls';

describe('WindowControls - 事件隔离(避免触发标题栏 tear-off)', () => {
  test('按钮 mousedown 不冒泡到父容器', () => {
    const parentMouseDown = vi.fn();
    const { getByLabelText } = render(
      <div onMouseDown={parentMouseDown}>
        <WindowControls onClose={() => {}} onMinimize={() => {}} onMaximize={() => {}} />
      </div>
    );

    // 修复前:mousedown 冒泡触发 parentMouseDown(tear-off 被错误激活)→ 失败
    // 修复后:stopPropagation 阻断冒泡 → 通过
    fireEvent.mouseDown(getByLabelText('最小化窗口'));
    fireEvent.mouseDown(getByLabelText('最大化窗口'));
    fireEvent.mouseDown(getByLabelText('关闭窗口'));

    expect(parentMouseDown).not.toHaveBeenCalled();
  });

  test('点击最小化按钮仍正常触发 onMinimize 回调', () => {
    const onMinimize = vi.fn();
    const { getByLabelText } = render(
      <WindowControls onClose={() => {}} onMinimize={onMinimize} onMaximize={() => {}} />
    );

    fireEvent.click(getByLabelText('最小化窗口'));
    expect(onMinimize).toHaveBeenCalledTimes(1);
  });

  test('点击最大化按钮仍正常触发 onMaximize 回调', () => {
    const onMaximize = vi.fn();
    const { getByLabelText } = render(
      <WindowControls onClose={() => {}} onMinimize={() => {}} onMaximize={onMaximize} />
    );

    fireEvent.click(getByLabelText('最大化窗口'));
    expect(onMaximize).toHaveBeenCalledTimes(1);
  });

  test('点击关闭按钮仍正常触发 onClose 回调', () => {
    const onClose = vi.fn();
    const { getByLabelText } = render(
      <WindowControls onClose={onClose} onMinimize={() => {}} onMaximize={() => {}} />
    );

    fireEvent.click(getByLabelText('关闭窗口'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
