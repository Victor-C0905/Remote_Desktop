import zhCN from './locales/zh-CN';

type Stringify<T> = { [K in keyof T]: T[K] extends string ? string : Stringify<T[K]> };

/** 所有语言包必须满足：与 zh-CN 同形状、叶子均为 string */
export type TranslationSchema = Stringify<typeof zhCN>;
