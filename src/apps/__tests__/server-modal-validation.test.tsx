// 服务器编辑/添加对话框必填校验回归（R11-02）：
// 此前清空名称/主机后点「保存」是静默 no-op——对话框保持打开、无任何错误
// 提示，用户不知道为什么存不上。修复后必须显示内联错误且不提交；
// 提交时名称/主机做 trim。
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AddServerModal, EditServerModal } from "../Settings";

function fill(selector: string, index: number, value: string) {
  const inputs = document.querySelectorAll(selector);
  fireEvent.change(inputs[index], { target: { value } });
}

describe("AddServerModal 必填校验（R11-02 回归）", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("名称主机为空时保存：显示内联错误、不提交、不关闭", () => {
    const onAdd = vi.fn();
    const onClose = vi.fn();
    render(<AddServerModal isOpen onClose={onClose} onAdd={onAdd} />);

    // 添加对话框的提交按钮文案是 settings.addBtn（「添加」），按主按钮类名定位
    const saveBtn = document.querySelector(".st-modal-footer .st-btn-primary") as HTMLElement;
    fireEvent.click(saveBtn);

    expect(onAdd).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText(/不能为空/)).toBeTruthy();
  });

  it("补全后保存：提交 trim 后的名称与主机", () => {
    const onAdd = vi.fn();
    const onClose = vi.fn();
    render(<AddServerModal isOpen onClose={onClose} onAdd={onAdd} />);

    fill(".st-form-input", 0, "  test-srv  ");
    fill(".st-form-input", 1, " example.com ");
    const saveBtn = document.querySelector(".st-modal-footer .st-btn-primary") as HTMLElement;
    fireEvent.click(saveBtn);

    expect(onAdd).toHaveBeenCalledTimes(1);
    expect(onAdd.mock.calls[0][0]).toBe("test-srv");
    expect(onAdd.mock.calls[0][1]).toBe("example.com");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("EditServerModal 必填校验（R11-02 回归）", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("清空主机后保存：显示内联错误、不提交", () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    render(
      <EditServerModal
        isOpen
        onClose={onClose}
        onSave={onSave}
        server={{ id: "srv-1", name: "本地", host: "localhost", port: 8443 }}
      />,
    );

    fill(".st-form-input", 1, "");
    fireEvent.click(screen.getByText("保存"));

    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByText(/不能为空/)).toBeTruthy();
  });
});
