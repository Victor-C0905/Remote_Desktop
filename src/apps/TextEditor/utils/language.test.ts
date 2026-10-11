/**
 * 语言识别工具单测
 *
 * 覆盖状态栏语言展示名（detectLanguage）
 * 与 CodeMirror 语言描述匹配（findLanguageForPath）
 */

import { describe, it, expect } from 'vitest';
import { detectLanguage, findLanguageForPath } from './language';

describe('detectLanguage（扩展名 → 展示名）', () => {
  it('常见编程语言按扩展名映射', () => {
    expect(detectLanguage('/home/user/main.ts')).toBe('TypeScript');
    expect(detectLanguage('app.py')).toBe('Python');
    expect(detectLanguage('lib.rs')).toBe('Rust');
    expect(detectLanguage('server.go')).toBe('Go');
  });

  it('大小写不敏感', () => {
    expect(detectLanguage('README.MD')).toBe('Markdown');
    expect(detectLanguage('config.JSON')).toBe('JSON');
  });

  it('无扩展名返回纯文本', () => {
    expect(detectLanguage('/etc/hostname')).toBe('纯文本');
    expect(detectLanguage('')).toBe('纯文本');
  });

  it('未知扩展名返回大写扩展名', () => {
    expect(detectLanguage('data.xyz')).toBe('XYZ');
  });

  it('Shell 系列统一为 Shell', () => {
    expect(detectLanguage('run.sh')).toBe('Shell');
    expect(detectLanguage('.zshrc')).toBe('Shell');
  });
});

describe('findLanguageForPath（CodeMirror 语言匹配）', () => {
  it('按扩展名匹配到语言描述', () => {
    expect(findLanguageForPath('/home/user/app.py')?.name).toBe('Python');
    expect(findLanguageForPath('/home/user/main.rs')?.name).toBe('Rust');
  });

  it('TS/TSX/JS 变体均可匹配', () => {
    expect(findLanguageForPath('a.ts')).not.toBeNull();
    expect(findLanguageForPath('a.tsx')).not.toBeNull();
    expect(findLanguageForPath('a.js')).not.toBeNull();
    expect(findLanguageForPath('a.mjs')).not.toBeNull();
  });

  it('无扩展名文件按文件名规则匹配（Dockerfile/CMakeLists.txt）', () => {
    expect(findLanguageForPath('/app/Dockerfile')?.name).toBe('Dockerfile');
    expect(findLanguageForPath('/proj/CMakeLists.txt')?.name).toBe('CMake');
  });

  it('未匹配返回 null', () => {
    expect(findLanguageForPath('blob.unknownext')).toBeNull();
    expect(findLanguageForPath('')).toBeNull();
  });
});
