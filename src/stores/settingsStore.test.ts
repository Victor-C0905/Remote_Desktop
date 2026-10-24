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
