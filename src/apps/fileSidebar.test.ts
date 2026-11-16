import { describe, it, expect } from 'vitest';
import { buildSidebarSections } from './fileSidebar';
import type { CommonLocation } from './fileSidebar';

const location = (overrides: Partial<CommonLocation>): CommonLocation => ({
  group: 'home',
  key: 'documents',
  label: '文档',
  path: '/home/vic/文档',
  ...overrides,
});

describe('buildSidebarSections', () => {
  it('新 Agent：仅渲染服务器真实存在的家目录标准目录', () => {
    const sections = buildSidebarSections({
      username: 'vic',
      homeDir: '/home/vic',
      activeServerId: 's1',
      commonLocations: [
        location({ key: 'downloads', label: '下载', path: '/home/vic/下载' }),
        location({ group: 'system', key: 'etc', label: '/etc', path: '/etc' }),
      ],
    });

    const locationSection = sections.find((s) => s.titleKey === 'fileManager.location');
    expect(locationSection?.items).toEqual([
      { icon: 'home', labelKey: 'fileManager.home', path: '/home/vic' },
      { icon: 'download', label: '下载', path: '/home/vic/下载' },
    ]);
    // 不再渲染自造的「系统位置」分组
    expect(sections.some((s) => s.titleKey === 'fileManager.systemLocations')).toBe(false);
  });

  it('服务器无标准位置（空数组）时仅展示主目录，不臆造不存在的路径', () => {
    // R6-P2-02：/root 等无 XDG 目录的家目录，硬编码回退会指向不存在的目录
    const sections = buildSidebarSections({
      username: 'root',
      homeDir: '/root',
      activeServerId: 's1',
      commonLocations: [location({ group: 'system', key: 'etc', label: '/etc', path: '/etc' })],
    });

    const locationSection = sections.find((s) => s.titleKey === 'fileManager.location');
    expect(locationSection?.items).toEqual([
      { icon: 'home', labelKey: 'fileManager.home', path: '/root' },
    ]);
  });

  it('常用分组置于首位，按给定统计顺序渲染目录名与完整路径 tooltip', () => {
    const sections = buildSidebarSections({
      username: 'vic',
      homeDir: '/home/vic',
      activeServerId: 's1',
      commonLocations: [location({})],
      frequentPaths: [
        { path: '/home/vic/projects/quirel', score: 12, count: 2, lastUsed: 2 },
        { path: '/srv/data', score: 7, count: 1, lastUsed: 1 },
      ],
    });

    expect(sections[0].titleKey).toBe('fileManager.frequent');
    expect(sections[0].items).toEqual([
      { icon: 'folder', label: 'quirel', title: '/home/vic/projects/quirel', path: '/home/vic/projects/quirel' },
      { icon: 'folder', label: 'data', title: '/srv/data', path: '/srv/data' },
    ]);
  });

  it('常用排除家目录与标准位置，避免与「位置」分组功能重复', () => {
    const sections = buildSidebarSections({
      username: 'vic',
      homeDir: '/home/vic',
      activeServerId: 's1',
      commonLocations: [
        location({ key: 'downloads', label: '下载', path: '/home/vic/下载' }),
      ],
      frequentPaths: [
        { path: '/home/vic', score: 20, count: 9, lastUsed: 1 },                  // 家目录 → 排除
        { path: '/home/vic/下载', score: 15, count: 8, lastUsed: 2 },             // 标准位置 → 排除
        { path: '/home/vic/projects/quirel', score: 12, count: 2, lastUsed: 3 },  // 保留
      ],
    });

    const frequent = sections.find((s) => s.titleKey === 'fileManager.frequent');
    expect(frequent?.items.map((i) => i.path)).toEqual(['/home/vic/projects/quirel']);
  });

  it('无访问统计时不渲染常用分组', () => {
    const sections = buildSidebarSections({
      username: 'vic',
      homeDir: '/home/vic',
      activeServerId: 's1',
      commonLocations: [location({})],
      frequentPaths: [],
    });
    expect(sections.some((s) => s.titleKey === 'fileManager.frequent')).toBe(false);
  });

  it('不再渲染设备分组', () => {
    const sections = buildSidebarSections({
      username: 'vic',
      homeDir: '/home/vic',
      activeServerId: 's1',
      commonLocations: [location({})],
    });
    expect(sections.some((s) => s.titleKey === 'fileManager.devices')).toBe(false);
  });

  it('离线模式仅保留默认 Linux 占位结构', () => {
    const sections = buildSidebarSections({
      username: null,
      homeDir: null,
      activeServerId: null,
    });

    expect(sections).toHaveLength(1);
    expect(sections[0].titleKey).toBe('fileManager.location');
    expect(sections[0].items[0]).toEqual({
      icon: 'home',
      labelKey: 'fileManager.home',
      path: '/home',
    });
  });
});
