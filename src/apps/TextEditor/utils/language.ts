/**
 * 语言识别工具
 *
 * 状态栏的语言展示与 CodeMirror 语法高亮共用的扩展名映射。
 * 从 StatusBar 提取为独立模块，供两处复用。
 */

import { LanguageDescription } from '@codemirror/language';
import { languages } from '@codemirror/language-data';

/** 扩展名 → 语言展示名（状态栏用） */
const LANG_MAP: Record<string, string> = {
  ts: 'TypeScript', tsx: 'TypeScript React',
  js: 'JavaScript', jsx: 'JavaScript React', mjs: 'JavaScript',
  py: 'Python', rs: 'Rust', go: 'Go', java: 'Java', kt: 'Kotlin',
  c: 'C', h: 'C Header', cpp: 'C++', cc: 'C++', hpp: 'C++',
  cs: 'C#', rb: 'Ruby', php: 'PHP', swift: 'Swift',
  css: 'CSS', scss: 'SCSS', less: 'Less',
  html: 'HTML', htm: 'HTML', xml: 'XML', json: 'JSON', jsonc: 'JSON',
  md: 'Markdown', markdown: 'Markdown', rst: 'reStructuredText',
  sh: 'Shell', bash: 'Shell', zsh: 'Shell', fish: 'Shell',
  zshrc: 'Shell', bashrc: 'Shell', bash_profile: 'Shell', profile: 'Shell',
  yml: 'YAML', yaml: 'YAML', toml: 'TOML', ini: 'INI', conf: 'Config',
  sql: 'SQL', lua: 'Lua', vim: 'Vim Script',
  dockerfile: 'Dockerfile', makefile: 'Makefile',
  txt: '纯文本', log: '日志',
};

/**
 * 根据扩展名推断编辑语言（状态栏显示用）
 *
 * @param path - 文件路径
 * @returns 语言显示名（例如：TypeScript、Markdown）
 */
export function detectLanguage(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? '';
  const dot = base.lastIndexOf('.');
  const ext = dot >= 0 ? base.slice(dot + 1).toLowerCase() : '';
  return LANG_MAP[ext] || (ext ? ext.toUpperCase() : '纯文本');
}

/**
 * 按文件路径匹配 CodeMirror 语言描述（语法高亮用）
 *
 * 使用 @codemirror/language-data 的注册表，按需动态加载语言包
 * （打开对应扩展名的文件时才会加载该语言的分包）。
 * 覆盖扩展名规则与 Makefile/Dockerfile 等无扩展名文件名规则；
 * filename 规则只对文件名生效，因此这里传入 basename。
 *
 * @param path - 文件路径
 * @returns 匹配到的语言描述；无匹配返回 null
 */
export function findLanguageForPath(path: string): LanguageDescription | null {
  if (!path) return null;
  const base = path.split(/[\\/]/).pop() ?? '';
  return LanguageDescription.matchFilename(languages, base);
}
