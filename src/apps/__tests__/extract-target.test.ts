import { archiveParentPath, archiveStemName } from '../ArchiveViewer/extractTarget';

describe('archiveParentPath', () => {
  it('取压缩包所在目录，根目录回退 /', () => {
    expect(archiveParentPath('/home/user/a.tar.gz')).toBe('/home/user');
    expect(archiveParentPath('/a.tar')).toBe('/');
  });
});

describe('archiveStemName', () => {
  it('剥离复合扩展名；空 stem 回退 archive', () => {
    expect(archiveStemName('a.tar.gz')).toBe('a');
    expect(archiveStemName('.tar.gz')).toBe('archive');
  });
});
