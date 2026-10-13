import type { TranslationSchema } from '../types';

/** English 词典。类型约束：缺 key / 多 key / 结构不一致 → 编译报错 */
const enUS: TranslationSchema = {
  common: {
    confirm: 'OK',
    cancel: 'Cancel',
  },
};

export default enUS;
