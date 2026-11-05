import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { ArchiveList } from '../ArchiveViewer/components/ArchiveList';
import type { ArchiveTreeNode } from '../ArchiveViewer/tree';

function node(name: string, isDir: boolean, path = name, children: ArchiveTreeNode[] = []): ArchiveTreeNode {
  return { name, path, isDir, kind: isDir ? 'dir' : 'file', size: isDir ? 0 : 1, children };
}

function renderList(overrides: Partial<Parameters<typeof ArchiveList>[0]> = {}) {
  const onNavigate = vi.fn();
  const onOpenFile = vi.fn();
  const onSelectionChange = vi.fn();
  render(
    <ArchiveList
      nodes={[]}
      currentPath=""
      selected={new Set()}
      extractingPath={null}
      onNavigate={onNavigate}
      onOpenFile={onOpenFile}
      onSelectionChange={onSelectionChange}
      {...overrides}
    />,
  );
  return { onNavigate, onOpenFile, onSelectionChange };
}

describe('ArchiveList 双击导航', () => {
  it('双击文件夹回调目录路径（指针捕获不得吞掉 dblclick）', () => {
    const { onNavigate } = renderList({ nodes: [node('dir', true), node('a.txt', false)] });
    fireEvent.doubleClick(screen.getByText('dir'));
    expect(onNavigate).toHaveBeenCalledWith('dir');
  });

  it('pointerdown 不立即捕获指针（否则 dblclick 会被重定向到容器）', () => {
    renderList({ nodes: [node('dir', true)] });
    const row = screen.getByText('dir').closest<HTMLElement>('.av-row');
    expect(row).not.toBeNull();
    expect(() => fireEvent.pointerDown(row!, { pointerId: 1, button: 0 })).not.toThrow();
  });

  it('双击文件回调打开', () => {
    const { onNavigate, onOpenFile } = renderList({ nodes: [node('a.txt', false)] });
    fireEvent.doubleClick(screen.getByText('a.txt'));
    expect(onOpenFile).toHaveBeenCalledTimes(1);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it('双击 .. 返回父目录', () => {
    const { onNavigate } = renderList({ nodes: [], currentPath: 'dir/sub' });
    fireEvent.doubleClick(screen.getByText('..'));
    expect(onNavigate).toHaveBeenCalledWith('dir');
  });

  it('顶层目录的上一级是包根（空字符串，不是 /）', () => {
    const { onNavigate } = renderList({ nodes: [], currentPath: 'dir' });
    fireEvent.doubleClick(screen.getByText('..'));
    expect(onNavigate).toHaveBeenCalledWith('');
  });
});
