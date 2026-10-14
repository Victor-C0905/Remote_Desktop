# 新增语言包指南

新增一种界面语言只需两步，无任何其他改动：

## 步骤

1. **创建词典文件** `src/i18n/locales/<locale>.ts`（如 `ja-JP.ts`）：

```ts
import type { TranslationSchema } from '../types';

const jaJP: TranslationSchema = {
  // 结构必须与 zh-CN.ts 完全一致（类型系统强制：缺 key / 多 key / 结构不符均编译报错）
  common: { confirm: 'OK', cancel: 'キャンセル' /* ... */ },
  // ...其余域
};

export default jaJP;
```

建议流程：复制 `en-US.ts` 全文 → 逐条翻译。跑 `npm run test:run -- src/i18n` 会运行时校验结构一致性。

2. **注册语言** `src/i18n/index.ts` 的 `RESOURCES` 加一行：

```ts
export const RESOURCES = {
  'zh-CN': { label: '简体中文', resource: zhCN },
  'en-US': { label: 'English', resource: enUS },
  'ja-JP': { label: '日本語', resource: jaJP },   // ← 新增
} as const;
```

设置页语言下拉框自动出现新选项。`label` 一律用该语言的**自称**（如 日本語 / Deutsch），不用英文。

## 约定

- 术语表见 `docs/superpowers/plans/2026-10-13-i18n.md`（冲突以术语表为准）
- 词典不使用数组；插值 `{{var}}`；宁可两个 key 不做语法拼接
- zh-CN 是 source of truth：新增功能先加中文 key，其他语言同步补
- 缺 key 时显示中文（`fallbackLng: 'zh-CN'`），不会出现裸 key

## 错误文案与 Rust 错误码的对应关系（重要）

Rust 端所有用户可见错误是结构化 `AppError { code, params?, detail? }`（见 `src-tauri/src/errors.rs` 的 `codes` 模块）。前端词典 `errors.op.*` 的键与 codes 模块**一一对应**：

- 新增 Rust 错误码时，**同一 commit** 内必须补齐：
  1. `src/i18n/locales/zh-CN.ts` 与 `en-US.ts` 的 `errors.op.<camelCaseKey>` 文案（params 参与插值，如 `{{message}}`）；
  2. `src/types/errors.ts` 的 `OP_ERROR_KEY_MAP`（snake_case code → 词典键）；
  3. `src/types/errors.test.ts` 的 snake_case 全覆盖数组。
- `detail` 是英文技术串，仅供日志排查，**不得**进入用户正文；「三不暴露」：不暴露协议名词、库错误原文、内部机制。
- code 语义一经发布不可变更（词典按 code 查表）。
