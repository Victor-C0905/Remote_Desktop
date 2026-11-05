import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { ExtractDialog } from '../ArchiveViewer/components/ExtractDialog';

function baseProps() {
  return {
    fileName: 'a.tar.gz',
    path: '/x/a.tar.gz',
    serverId: 's1',
    status: 'ask' as const,
    onExtract: vi.fn(),
    onClose: vi.fn(),
  };
}

describe('ExtractDialog 独立文件夹选项', () => {
  it('包根解压时可用，勾选后目标为父目录/stem', () => {
    const props = baseProps();
    render(<ExtractDialog {...props} />);
    const checkbox = screen.getByRole('checkbox');
    expect(checkbox).toBeEnabled();
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByText('解压'));
    expect(props.onExtract).toHaveBeenCalledWith('/x/a');
  });

  it('子目录解压时禁用，目标保持父目录（不再套一层）', () => {
    const props = baseProps();
    render(<ExtractDialog {...props} currentDir="quirel-web" />);
    expect(screen.getByRole('checkbox')).toBeDisabled();
    fireEvent.click(screen.getByText('解压'));
    expect(props.onExtract).toHaveBeenCalledWith('/x');
  });
});
