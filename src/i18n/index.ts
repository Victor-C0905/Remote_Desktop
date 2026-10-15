import i18next from 'i18next';
import { initReactI18next } from 'react-i18next';
import zhCN from './locales/zh-CN';
import enUS from './locales/en-US';

/**
 * 语言注册表。
 * 新增语言包协议（全项目唯一改动点）：
 *   1. src/i18n/locales/ 下新建 <locale>.ts，导出标注 TranslationSchema 的完整词典；
 *   2. 本注册表加一行 { label: '语言名', resource: 词典 }。
 * Settings 语言下拉框从 RESOURCES 渲染，新语言自动出现。
 */
export const RESOURCES = {
  'zh-CN': { label: '简体中文', resource: zhCN },
  'en-US': { label: 'English', resource: enUS },
} as const;

export type Locale = keyof typeof RESOURCES;
export const FALLBACK_LOCALE: Locale = 'zh-CN';

// 模块导入即初始化（默认 zh-CN），持久化的 locale 由 settingsStore rehydrate 后应用
void i18next.use(initReactI18next).init({
  resources: Object.fromEntries(
    Object.entries(RESOURCES).map(([code, { resource }]) => [code, { translation: resource }])
  ),
  lng: FALLBACK_LOCALE,
  fallbackLng: FALLBACK_LOCALE,
  interpolation: { escapeValue: false }, // React 已转义
  returnEmptyString: false,
});

/** 应用语言到 DOM 与 i18next（settingsStore 的 setLocale / onRehydrateStorage 调用） */
export function applyLocale(locale: Locale): void {
  document.documentElement.lang = locale;
  if (i18next.language !== locale) void i18next.changeLanguage(locale);
}

export default i18next;
