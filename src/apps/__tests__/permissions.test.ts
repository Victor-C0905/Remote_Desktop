/**
 * 权限转换工具单元测试
 *
 * 背景 bug（已修复）：matrixToOctal 曾用十进制拼接（7*100+5*10+5=755），
 * 数值 755 实为 0o1363，导致 chmod 结果错乱、界面八进制与符号串显示互相矛盾。
 * 本测试锁定位权语义：mode 数值 = mode_t（0o755=493），显示用 toString(8)。
 */
import { describe, it, expect } from 'vitest';
import { parsePermissions, matrixToOctal, octalToPermString } from '../permissions';

describe('parsePermissions', () => {
  it('9 位标准权限串', () => {
    expect(parsePermissions('rwxr-xr-x')).toEqual([
      [true, true, true],
      [true, false, true],
      [true, false, true],
    ]);
  });

  it('10 位带类型前缀取末 9 位', () => {
    expect(parsePermissions('drw--w----')).toEqual(parsePermissions('rw--w----'));
  });

  it('执行位兼容 s/t（视为有执行），S/T 视为无执行', () => {
    expect(parsePermissions('rwsr-xr-t')[0]).toEqual([true, true, true]);
    expect(parsePermissions('rwsr-xr-t')[2]).toEqual([true, false, true]);
    expect(parsePermissions('rwSr-xr-T')[0]).toEqual([true, true, false]);
    expect(parsePermissions('rwSr-xr-T')[2]).toEqual([true, false, false]);
  });

  it('缺失/非法长度返回全 false 矩阵', () => {
    const empty = [
      [false, false, false],
      [false, false, false],
      [false, false, false],
    ];
    expect(parsePermissions(undefined)).toEqual(empty);
    expect(parsePermissions('')).toEqual(empty);
    expect(parsePermissions('rwx')).toEqual(empty);
  });
});

describe('matrixToOctal（位权计算，回归 bug 场景）', () => {
  it('只勾选所有者读取 → 0o400 = 256（曾是 bug：十进制拼接得 400 = 0o620）', () => {
    const m = parsePermissions('r--------');
    expect(matrixToOctal(m)).toBe(0o400);
    expect(matrixToOctal(m)).toBe(256);
    // 显示必须用 toString(8)：bug 场景下界面显示 rw--w---- 的根源
    expect(matrixToOctal(m).toString(8)).toBe('400');
  });

  it('rw--w---- → 0o620 = 400（曾是 bug：得 620 = 0o1154 含 sticky）', () => {
    expect(matrixToOctal(parsePermissions('rw--w----'))).toBe(0o620);
    expect(matrixToOctal(parsePermissions('rw--w----'))).toBe(400);
  });

  it('rwxr-xr-x → 0o755 = 493', () => {
    expect(matrixToOctal(parsePermissions('rwxr-xr-x'))).toBe(0o755);
    expect(matrixToOctal(parsePermissions('rwxr-xr-x'))).toBe(493);
  });

  it('全空矩阵 → 0', () => {
    expect(matrixToOctal(parsePermissions(undefined))).toBe(0);
  });
});

describe('octalToPermString（位权解析，与 matrixToOctal 互逆）', () => {
  it('0o755 = 493 → "rwxr-xr-x"', () => {
    expect(octalToPermString(0o755)).toBe('rwxr-xr-x');
    expect(octalToPermString(493)).toBe('rwxr-xr-x');
  });

  it('0o620 = 400 → "rw--w----"', () => {
    expect(octalToPermString(0o620)).toBe('rw--w----');
    expect(octalToPermString(400)).toBe('rw--w----');
  });

  it('0o400 = 256 → "r--------"', () => {
    expect(octalToPermString(0o400)).toBe('r--------');
    expect(octalToPermString(256)).toBe('r--------');
  });
});

describe('往返一致性（parse ↔ matrix ↔ mode ↔ string）', () => {
  const cases = ['rwxr-xr-x', 'rw--w----', 'r--------', '---------', 'rwxrwxrwx', 'r--r-----'];

  it.each(cases)('%s 全链路往返不变', (perm) => {
    // 字符串 → 矩阵 → mode → 字符串
    const m = parsePermissions(perm);
    const mode = matrixToOctal(m);
    expect(octalToPermString(mode)).toBe(perm);
    // 字符串 → 矩阵 → mode → 矩阵（跳过字符串，直接矩阵往返）
    expect(parsePermissions(octalToPermString(mode))).toEqual(m);
  });
});
