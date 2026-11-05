import { joinRemotePath, normalizeRemotePath, parentRemotePath } from './path';

describe('normalizeRemotePath', () => {
  it('补前导斜杠并去掉多余分隔符', () => {
    expect(normalizeRemotePath('home/user')).toBe('/home/user');
    expect(normalizeRemotePath('//home//user//')).toBe('/home/user');
    expect(normalizeRemotePath('')).toBe('/');
    expect(normalizeRemotePath('/')).toBe('/');
  });
});

describe('joinRemotePath', () => {
  it('根目录与普通目录都能正确拼接', () => {
    expect(joinRemotePath('/', 'a.txt')).toBe('/a.txt');
    expect(joinRemotePath('/home/user', 'a.txt')).toBe('/home/user/a.txt');
    expect(joinRemotePath('home/user/', 'a.txt')).toBe('/home/user/a.txt');
  });
});

describe('parentRemotePath', () => {
  it('取父目录，根目录与一级目录回退到 /', () => {
    expect(parentRemotePath('/home/user/file.txt')).toBe('/home/user');
    expect(parentRemotePath('/home/user')).toBe('/home');
    expect(parentRemotePath('/home')).toBe('/');
    expect(parentRemotePath('/')).toBe('/');
  });
});
