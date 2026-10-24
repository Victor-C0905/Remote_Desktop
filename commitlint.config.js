// 提交信息校验规则 —— 与 AGENTS.md 第 1 节保持一致
// wip 仅限任务分支上的检查点提交，禁止出现在 dev / main
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'type-enum': [
      2,
      'always',
      ['feat', 'fix', 'docs', 'refactor', 'perf', 'test', 'build', 'chore', 'revert', 'wip'],
    ],
    'type-empty': [2, 'never'],
    'subject-empty': [2, 'never'],
  },
};
