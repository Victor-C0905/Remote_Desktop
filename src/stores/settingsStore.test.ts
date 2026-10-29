import { describe, it, expect, beforeEach } from 'vitest';
import { useSettingsStore } from './settingsStore';
import i18n from '../i18n';

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('settingsStore.locale', () => {
  beforeEach(async () => {
    useSettingsStore.setState({ locale: 'zh-CN' });
    await i18n.changeLanguage('zh-CN');
  });

  it('setLocale 同步切换 i18n 语言与 <html lang>', async () => {
    useSettingsStore.getState().setLocale('en-US');
    await flush();
    expect(useSettingsStore.getState().locale).toBe('en-US');
    expect(i18n.language).toBe('en-US');
    expect(document.documentElement.lang).toBe('en-US');
  });
});

describe('settingsStore.iconTheme DOM 镜像', () => {
  // 图标体系双真相源（SVG 结构跟 store、颜色变量跟根元素 data-icon-theme），
  // 属性失步会导致 classic 结构套 paper 色板的串档渲染——镜像必须对任何
  // 改动路径成立，不限于 setIconTheme
  beforeEach(() => {
    useSettingsStore.setState({ iconTheme: 'classic' });
  });

  it('setIconTheme 后根元素 data-icon-theme 与 store 一致', () => {
    useSettingsStore.getState().setIconTheme('paper');
    expect(document.documentElement.getAttribute('data-icon-theme')).toBe('paper');
    useSettingsStore.getState().setIconTheme('classic');
    expect(document.documentElement.getAttribute('data-icon-theme')).toBe('classic');
  });

  it('绕过 action 直接 setState 也同步镜像（订阅兜底任意 mutation 路径）', () => {
    useSettingsStore.setState({ iconTheme: 'paper' });
    expect(document.documentElement.getAttribute('data-icon-theme')).toBe('paper');
  });
});
