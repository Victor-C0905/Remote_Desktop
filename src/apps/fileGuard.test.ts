import { describe, it, expect } from 'vitest';
import { assessTransferRisk } from './fileGuard';

describe('assessTransferRisk', () => {
  it('普通业务目录移动：无发现', () => {
    expect(assessTransferRisk(['/home/u/a.txt'], '/home/u/b')).toEqual([]);
  });

  it('源在系统目录下：systemPath 发现', () => {
    const r = assessTransferRisk(['/etc/nginx/nginx.conf'], '/home/u');
    expect(r).toEqual([{ reason: 'systemPath', sample: '/etc/nginx/nginx.conf' }]);
  });

  it('目标目录是系统目录本体：systemPath 发现', () => {
    const r = assessTransferRisk(['/home/u/x'], '/var');
    expect(r).toEqual([{ reason: 'systemPath', sample: '/var' }]);
  });

  it('同前缀但非子路径不误报（/varwww ≠ /var）', () => {
    expect(assessTransferRisk(['/varwww/x'], '/home')).toEqual([]);
  });

  it('路径段含 .ssh：sensitiveDir 发现（.sshx 不误报）', () => {
    expect(assessTransferRisk(['/root/.ssh/authorized_keys'], '/home/u')).toEqual([
      { reason: 'sensitiveDir', sample: '/root/.ssh/authorized_keys' },
    ]);
    expect(assessTransferRisk(['/home/u/.sshx/f'], '/home/u')).toEqual([]);
  });

  it('src 与目标双命中：按原因去重各取一条，系统目录优先', () => {
    const r = assessTransferRisk(['/usr/local/x', '/root/.ssh/k'], '/etc/nginx');
    expect(r.map((f) => f.reason)).toEqual(['systemPath', 'sensitiveDir']);
    expect(r[0].sample).toBe('/usr/local/x');
    expect(r[1].sample).toBe('/root/.ssh/k');
  });
});
