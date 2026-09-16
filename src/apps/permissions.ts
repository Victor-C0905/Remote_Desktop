/**
 * 权限字符串 ↔ 3×3 矩阵 ↔ mode 数值 转换（纯函数，供 FileManager 属性对话框使用）
 *
 * mode 语义约定（与 Linux chmod / Rust PermissionsExt::from_mode 一致）：
 * - 数值 = mode_t 位权值（如 0o755 = 493），r=4 / w=2 / x=1，三组各占 3 个二进制位
 * - 八进制显示字符串用 Number.prototype.toString(8) 生成（如 (493).toString(8) === "755"）
 * - 禁止十进制拼接（如 7*100+5*10+5=755）——数值 755 实为 0o1363，会导致 chmod 结果错乱
 */

/** 权限矩阵：行 = [所有者, 组, 其他]，每行 = [读取, 写入, 执行] */
export type PermMatrix = boolean[][];

/** 全 false 矩阵（权限字符串缺失/非法时的回退，仍可编辑） */
function emptyMatrix(): PermMatrix {
  return [
    [false, false, false],
    [false, false, false],
    [false, false, false],
  ];
}

/**
 * 解析权限字符串为 3×3 矩阵
 * 兼容 9 位 "rwxr-xr-x" 与 10 位带类型前缀 "drwxr-xr-x" 两种长度；
 * 执行位兼容 setuid/setgid/sticky 写法（s/t 视为有执行权限，S/T 视为无）；
 * 权限字符串缺失或长度非法时返回全 false（矩阵全空，仍可编辑）
 */
export function parsePermissions(perm: string | undefined): PermMatrix {
  const s = perm ?? "";
  // 10 位及以上取末 9 位（剥离类型/特殊位前缀）
  const bits = s.length >= 10 ? s.slice(-9) : s;
  if (bits.length !== 9) {
    return emptyMatrix();
  }
  const toTriple = (t: string): boolean[] => [
    t[0] === "r",
    t[1] === "w",
    t[2] === "x" || t[2] === "s" || t[2] === "t",
  ];
  return [toTriple(bits.slice(0, 3)), toTriple(bits.slice(3, 6)), toTriple(bits.slice(6, 9))];
}

/**
 * 矩阵 → mode 数值（位权计算）
 * "rwxr-xr-x" 矩阵 → 0o755 = 493；"r--------" 矩阵 → 0o400 = 256
 */
export function matrixToOctal(m: PermMatrix): number {
  const tri = (t: boolean[]) => (t[0] ? 4 : 0) | (t[1] ? 2 : 0) | (t[2] ? 1 : 0);
  return (tri(m[0]) << 6) | (tri(m[1]) << 3) | tri(m[2]);
}

/**
 * mode 数值 → 9 位权限字符串（位权解析，与 matrixToOctal 互逆）
 * 0o755 = 493 → "rwxr-xr-x"；0o620 = 400 → "rw--w----"
 */
export function octalToPermString(mode: number): string {
  let s = "";
  for (const shift of [6, 3, 0]) {  // 所有者/组/其他 各占 3 个二进制位
    const digit = (mode >> shift) & 7;
    for (let i = 0; i < 3; i++) s += ((digit >> (2 - i)) & 1) ? "rwx"[i] : "-";
  }
  return s;
}
