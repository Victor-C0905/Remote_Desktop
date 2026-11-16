import { describe, it, expect, beforeEach } from 'vitest';
import {
  serializeSessionMirror,
  parseSessionMirror,
  clearSessionMirrors,
  type StampedSessionMirror,
} from './sessionMirror';

const EPOCH_A = '100-a';
const EPOCH_B = '200-b';

describe('serializeSessionMirror / parseSessionMirror 往返', () => {
  it('写入的镜像可被同纪元读回', () => {
    const raw = serializeSessionMirror(EPOCH_A, { active: 'srv-1', connected: ['srv-1', 'srv-2'] });
    expect(parseSessionMirror(raw, EPOCH_A)).toEqual<StampedSessionMirror>({
      epoch: EPOCH_A,
      active: 'srv-1',
      connected: ['srv-1', 'srv-2'],
    });
  });

  it('active 为 null（无活动服务器）可往返', () => {
    const raw = serializeSessionMirror(EPOCH_A, { active: null, connected: [] });
    expect(parseSessionMirror(raw, EPOCH_A)?.active).toBeNull();
  });
});

describe('parseSessionMirror fail-closed（R8-N2/R9-N3 回归）', () => {
  it('纪元不匹配的镜像（上一世代残留）拒绝消费', () => {
    const raw = serializeSessionMirror(EPOCH_A, { active: null, connected: ['srv-1'] });
    expect(parseSessionMirror(raw, EPOCH_B)).toBeNull();
  });

  it('旧版无纪元戳镜像拒绝消费', () => {
    const legacy = JSON.stringify({ active: 'srv-1', connected: ['srv-1'] });
    expect(parseSessionMirror(legacy, EPOCH_A)).toBeNull();
  });

  it('损坏 JSON 拒绝消费', () => {
    expect(parseSessionMirror('{not-json', EPOCH_A)).toBeNull();
  });

  it('空值拒绝消费', () => {
    expect(parseSessionMirror(null, EPOCH_A)).toBeNull();
    expect(parseSessionMirror('', EPOCH_A)).toBeNull();
  });

  it('结构非法：connected 非字符串数组 / active 类型非法 / 根非对象', () => {
    const mk = (o: unknown) => JSON.stringify({ epoch: EPOCH_A, ...o as object });
    expect(parseSessionMirror(mk({ connected: 'srv-1' }), EPOCH_A)).toBeNull();
    expect(parseSessionMirror(mk({ connected: [1, 2] }), EPOCH_A)).toBeNull();
    expect(parseSessionMirror(mk({ active: 42, connected: [] }), EPOCH_A)).toBeNull();
    expect(parseSessionMirror(JSON.stringify('string-root'), EPOCH_A)).toBeNull();
    expect(parseSessionMirror(JSON.stringify({ epoch: 7, active: null, connected: [] }), EPOCH_A)).toBeNull();
  });
});

describe('clearSessionMirrors', () => {
  beforeEach(() => sessionStorage.clear());

  it('清除 wm/sm 镜像与 browser.mountGen.*，不碰无关键', () => {
    sessionStorage.setItem('wm.session', 'x');
    sessionStorage.setItem('sm.session', 'x');
    sessionStorage.setItem('browser.mountGen.1', 'x');
    sessionStorage.setItem('app.epoch', EPOCH_A);
    sessionStorage.setItem('unrelated', 'x');

    clearSessionMirrors(sessionStorage);

    expect(sessionStorage.getItem('wm.session')).toBeNull();
    expect(sessionStorage.getItem('sm.session')).toBeNull();
    expect(sessionStorage.getItem('browser.mountGen.1')).toBeNull();
    expect(sessionStorage.getItem('app.epoch')).toBe(EPOCH_A);
    expect(sessionStorage.getItem('unrelated')).toBe('x');
  });
});
