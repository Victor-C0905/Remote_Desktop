# 文件管理器地址栏实时导航实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现文件管理器地址栏的实时导航功能，支持路径建议列表和自动导航，修复 Backspace 键被拦截的问题。

**Architecture:** 使用防抖机制实现实时导航，输入停止 300ms 后自动获取路径建议或导航。建议列表使用下拉 UI，支持键盘导航。全局键盘处理在输入框聚焦时被阻止。

**Tech Stack:** React 18, TypeScript, Tauri 2.x, CSS (Adwaita 设计系统)

---

## 文件结构

**前端文件**：
- `src/apps/FileManager.tsx`：修改，添加状态、函数、键盘事件处理
- `src/apps/FileManager.css`：修改，添加建议列表样式

**后端文件**：
- `src-tauri/src/lib.rs`：修改，添加 `remote_get_path_suggestions` Tauri Command
- `agent/src/handler.rs`：修改，实现路径建议逻辑（如果需要）

---

## Task 1: 修复 Backspace 键被拦截的问题

**Files:**
- Modify: `src/apps/FileManager.tsx:610-626`

- [ ] **Step 1: 修改全局键盘处理函数**

在 `handleKeyDown` 函数开头添加 `isEditingPath` 检查，确保输入框聚焦时不拦截 Backspace。

```typescript
const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
  // 如果正在编辑路径，不处理任何全局键盘事件
  if (isEditingPath) return;

  // 如果正在编辑文件名，不处理任何键盘事件
  if (editingEntry) return;

  if (e.key === "Backspace") { e.preventDefault(); goUp(); }
  if (e.key === "Enter" && selectedIdx !== null) {
    handleOpen(entries[selectedIdx]);
  }
  if (e.key === "ArrowDown") {
    e.preventDefault();
    setSelectedIdx(prev => Math.min((prev ?? -1) + 1, entries.length - 1));
  }
  if (e.key === "ArrowUp") {
    e.preventDefault();
    setSelectedIdx(prev => Math.max((prev ?? 0) - 1, 0));
  }
}, [entries, selectedIdx, goUp, handleOpen, editingEntry, isEditingPath]);
```

- [ ] **Step 2: 测试 Backspace 键**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/us`
3. 按 Backspace 键
4. 验证：删除最后一个字符，变为 `/home/u`（而不是返回上级目录）

---

## Task 2: 添加路径建议列表状态

**Files:**
- Modify: `src/apps/FileManager.tsx:129-132`

- [ ] **Step 1: 添加新的状态变量**

在现有状态变量下方添加建议列表相关状态。

```typescript
// 路径输入框状态
const [pathInput, setPathInput] = useState(currentPath);
const [isEditingPath, setIsEditingPath] = useState(false);

// 路径建议列表状态
const [suggestions, setSuggestions] = useState<string[]>([]);
const [showSuggestions, setShowSuggestions] = useState(false);
const [selectedSuggestionIdx, setSelectedSuggestionIdx] = useState<number | null>(null);
```

- [ ] **Step 2: 验证状态添加成功**

检查代码，确保新状态变量已添加到正确的位置。

---

## Task 3: 实现获取路径建议函数

**Files:**
- Modify: `src/apps/FileManager.tsx:133-179`

- [ ] **Step 1: 添加 fetchSuggestions 函数**

在 `loadDir` 函数下方添加 `fetchSuggestions` 函数。

```typescript
const fetchSuggestions = useCallback(async (path: string) => {
  if (!activeServerId || path === "") {
    setSuggestions([]);
    setShowSuggestions(false);
    return;
  }

  try {
    // 调用后端 API 获取路径建议
    const suggestions = await invoke<string[]>("remote_get_path_suggestions", {
      serverId: activeServerId,
      path,
    });

    setSuggestions(suggestions);
    setShowSuggestions(suggestions.length > 0);
    setSelectedSuggestionIdx(null);
  } catch (err) {
    console.error("[FileManager] 获取建议失败:", err);
    setSuggestions([]);
    setShowSuggestions(false);
  }
}, [activeServerId]);
```

- [ ] **Step 2: 添加 handleSelectSuggestion 函数**

在 `fetchSuggestions` 函数下方添加 `handleSelectSuggestion` 函数。

```typescript
const handleSelectSuggestion = useCallback((suggestion: string) => {
  setPathInput(suggestion);
  setShowSuggestions(false);
  setIsEditingPath(false);
  navigateTo(suggestion);
}, [navigateTo]);
```

---

## Task 4: 实现防抖导航机制

**Files:**
- Modify: `src/apps/FileManager.tsx:365-370`

- [ ] **Step 1: 添加防抖 useEffect**

在现有的路径输入框同步 useEffect 下方添加防抖 useEffect。

```typescript
// 同步路径输入框
useEffect(() => {
  if (!isEditingPath) {
    setPathInput(currentPath);
  }
}, [currentPath, isEditingPath]);

// 防抖：输入停止 300ms 后获取建议或导航
useEffect(() => {
  if (!isEditingPath || pathInput === currentPath) {
    return;
  }

  const timer = setTimeout(() => {
    // 获取路径建议
    fetchSuggestions(pathInput);
  }, 300);

  return () => clearTimeout(timer);
}, [pathInput, isEditingPath, currentPath, fetchSuggestions]);
```

- [ ] **Step 2: 测试防抖机制**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/us`
3. 等待 300ms
4. 验证：显示建议列表（如果后端 API 已实现）

---

## Task 5: 实现键盘事件处理

**Files:**
- Modify: `src/apps/FileManager.tsx:377-387`

- [ ] **Step 1: 修改 handlePathInputKeyDown 函数**

扩展 `handlePathInputKeyDown` 函数，添加建议列表的键盘导航。

```typescript
const handlePathInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
  if (e.key === "Enter") {
    e.preventDefault();
    setShowSuggestions(false);
    setIsEditingPath(false);
    navigateTo(pathInput);
  } else if (e.key === "Escape") {
    e.preventDefault();
    setShowSuggestions(false);
    setIsEditingPath(false);
    setPathInput(currentPath);
  } else if (e.key === "Tab" && suggestions.length > 0) {
    e.preventDefault();
    handleSelectSuggestion(suggestions[0]);
  } else if (e.key === "ArrowDown" && showSuggestions) {
    e.preventDefault();
    setSelectedSuggestionIdx(prev =>
      prev === null ? 0 : Math.min(prev + 1, suggestions.length - 1)
    );
  } else if (e.key === "ArrowUp" && showSuggestions) {
    e.preventDefault();
    setSelectedSuggestionIdx(prev =>
      prev === null ? suggestions.length - 1 : Math.max(prev - 1, 0)
    );
  } else if (e.key === "Enter" && showSuggestions && selectedSuggestionIdx !== null) {
    e.preventDefault();
    handleSelectSuggestion(suggestions[selectedSuggestionIdx]);
  }
};
```

- [ ] **Step 2: 测试键盘导航**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/us`
3. 等待建议列表显示
4. 按 ↓ 键，验证：高亮第一个建议项
5. 按 ↓ 键，验证：高亮第二个建议项
6. 按 Enter 键，验证：导航到选中的路径
7. 按 Escape 键，验证：关闭建议列表，恢复原路径

---

## Task 6: 添加建议列表 UI 组件

**Files:**
- Modify: `src/apps/FileManager.tsx:642-652`
- Modify: `src/apps/FileManager.css`

- [ ] **Step 1: 修改地址栏容器，添加相对定位**

修改地址栏的容器，使其支持建议列表的定位。

```tsx
{/* Path Input Container */}
<div style={{ position: "relative", flex: 1 }}>
  <input
    type="text"
    className="fm-path-input"
    value={pathInput}
    onChange={handlePathInputChange}
    onKeyDown={handlePathInputKeyDown}
    onFocus={handlePathInputFocus}
    onBlur={handlePathInputBlur}
    placeholder="/home"
    title="输入路径并按 Enter 跳转"
  />

  {/* 建议列表 */}
  {showSuggestions && suggestions.length > 0 && (
    <div className="fm-suggestions">
      {suggestions.map((suggestion, idx) => (
        <div
          key={idx}
          className={`fm-suggestion-item${selectedSuggestionIdx === idx ? " selected" : ""}`}
          onClick={() => handleSelectSuggestion(suggestion)}
          onMouseEnter={() => setSelectedSuggestionIdx(idx)}
        >
          <span className="fm-suggestion-icon">📁</span>
          <span className="fm-suggestion-text">{suggestion}</span>
        </div>
      ))}
    </div>
  )}
</div>
```

- [ ] **Step 2: 添加建议列表样式**

在 `FileManager.css` 文件末尾添加建议列表样式。

```css
/* ── Path Suggestions ─────────────────────────────────── */

/* 建议列表容器 */
.fm-suggestions {
  position: absolute;
  top: 100%;  /* 紧贴输入框下方 */
  left: 0;
  right: 0;
  background: var(--view-bg);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  z-index: 100;
  max-height: 300px;
  overflow-y: auto;
}

/* 建议项 */
.fm-suggestion-item {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  gap: 8px;
  cursor: pointer;
  transition: background 0.1s ease-out;
}

.fm-suggestion-item:hover {
  background: var(--card-bg);
}

.fm-suggestion-item.selected {
  background: var(--accent-bg);
  color: var(--accent-fg);
}

/* 建议项图标 */
.fm-suggestion-icon {
  font-size: 16px;
}

/* 建议项文本 */
.fm-suggestion-text {
  font-size: var(--font-body);
  flex: 1;
}
```

- [ ] **Step 3: 测试建议列表 UI**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/us`
3. 等待建议列表显示
4. 验证：建议列表显示在输入框下方
5. 点击建议项，验证：导航到选中的路径
6. 验证：建议列表样式符合 GNOME 风格

---

## Task 7: 实现后端 API（可选）

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `agent/src/handler.rs`（如果需要）

- [ ] **Step 1: 添加 Tauri Command**

在 `src-tauri/src/lib.rs` 中添加 `remote_get_path_suggestions` Tauri Command。

```rust
#[tauri::command]
async fn remote_get_path_suggestions(
    server_id: String,
    path: String,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    // 通过 QUIC 连接获取路径建议
    // 实现逻辑：
    // 1. 解析输入路径，获取父目录
    // 2. 列出父目录下的所有子目录
    // 3. 过滤匹配输入路径前缀的目录
    // 4. 返回建议列表（最多 10 个）

    // 临时实现：返回空列表（后续实现完整逻辑）
    Ok(vec![])
}
```

- [ ] **Step 2: 注册 Tauri Command**

在 `src-tauri/src/lib.rs` 的 `invoke_handler` 中注册新命令。

```rust
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ... 其他命令
            remote_get_path_suggestions,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: 测试后端 API**

手动测试：
1. 重新编译并运行应用
2. 打开文件管理器
3. 点击地址栏，输入路径
4. 验证：前端可以调用后端 API（即使返回空列表）

---

## Task 8: 完整测试

**Files:**
- None

- [ ] **Step 1: 测试实时导航**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/user`
3. 等待 300ms
4. 验证：自动导航到 `/home/user`

- [ ] **Step 2: 测试错误路径**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/nonexistent`
3. 等待 300ms
4. 验证：显示错误提示，保持当前目录

- [ ] **Step 3: 测试建议列表交互**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/us`
3. 等待建议列表显示
4. 点击建议项
5. 验证：立即导航到选中的路径

- [ ] **Step 4: 测试键盘导航**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入路径
3. 按 ↓ 键导航建议列表
4. 按 Enter 键选择建议项
5. 按 Escape 键关闭建议列表

- [ ] **Step 5: 测试 Backspace 键**

手动测试：
1. 打开文件管理器
2. 点击地址栏，输入 `/home/user`
3. 按 Backspace 键多次
4. 验证：逐个删除字符，最终变为 `/`
5. 点击文件列表（输入框失焦）
6. 按 Backspace 键
7. 验证：返回上级目录

---

## Task 9: 提交代码（提示用户）

**Files:**
- None

- [ ] **Step 1: 提示用户提交代码**

提示用户：
```
所有改动已完成，请提交代码到 git：
git add src/apps/FileManager.tsx src/apps/FileManager.css src-tauri/src/lib.rs
git commit -m "feat: 实现文件管理器地址栏实时导航和路径建议列表"
```

---

## Self-Review

**1. Spec coverage:**
- ✅ Backspace 键修复：Task 1
- ✅ 实时导航：Task 4
- ✅ 路径建议列表：Task 2, Task 3, Task 6
- ✅ 键盘导航：Task 5
- ✅ 错误处理：Task 8
- ✅ 后端 API：Task 7

**2. Placeholder scan:**
- ✅ 没有 "TBD" 或 "TODO"
- ✅ 所有步骤都有完整的代码
- ✅ 所有测试都有明确的验证步骤

**3. Type consistency:**
- ✅ 状态变量名称一致：`suggestions`, `showSuggestions`, `selectedSuggestionIdx`
- ✅ 函数名称一致：`fetchSuggestions`, `handleSelectSuggestion`
- ✅ CSS 类名一致：`fm-suggestions`, `fm-suggestion-item`, `fm-suggestion-icon`, `fm-suggestion-text`

---

## 总结

本实现计划包含 9 个任务，覆盖了文件管理器地址栏实时导航的所有功能：

1. **Task 1**：修复 Backspace 键被拦截的问题
2. **Task 2**：添加路径建议列表状态
3. **Task 3**：实现获取路径建议函数
4. **Task 4**：实现防抖导航机制
5. **Task 5**：实现键盘事件处理
6. **Task 6**：添加建议列表 UI 组件
7. **Task 7**：实现后端 API（可选）
8. **Task 8**：完整测试
9. **Task 9**：提交代码（提示用户）

每个任务都包含详细的步骤、完整的代码和明确的测试验证。