# 文件管理器地址栏实时导航设计

> 版本: v1.0 | 日期: 2026-06-22
>
> 设计目标：实现文件管理器地址栏的实时导航功能，支持路径建议列表和自动导航。

---

## 一、问题分析

### 当前问题

**问题 1：Backspace 键被全局键盘处理拦截**
- 当前代码在全局键盘处理中拦截 Backspace，导致输入框聚焦时无法删除文本
- 用户期望：输入框聚焦时，Backspace 应删除文本，而非返回上级目录

**问题 2：没有实时响应**
- 当前实现需要按 Enter 才会导航，没有实时响应
- 用户期望：输入时实时尝试导航到输入的路径

---

## 二、设计方案

### 方案选择

**方案 3：混合模式（自动补全 + 实时导航）**

```
用户输入 → 防抖延迟(300ms) → 获取路径建议 → 显示建议列表
         ↓
         用户选择建议项 → 立即导航
         或
         继续输入 → 防抖后自动导航（如果路径存在）
```

**核心特点**：
- 输入时显示路径建议列表（自动补全）
- 用户可以选择建议项立即导航
- 或者继续输入，防抖后自动导航
- 结合了 GNOME Nautilus 的自动补全和用户的实时导航需求

---

## 三、架构设计

### 数据流

```
┌─ 输入框状态 ─────────────────────────────────────┐
│  pathInput: 当前输入的路径文本                     │
│  isEditingPath: 是否正在编辑                       │
│  suggestions: 路径建议列表                         │
│  showSuggestions: 是否显示建议列表                 │
└───────────────────────────────────────────────────┘
          │
          │ onChange → 更新 pathInput
          │
          ├─ 防抖 useEffect (300ms) ────────────────┐
          │                                         │
          │   等待输入停止                           │
          │                                         │
          │   ↓                                     │
          │                                         │
          │   获取路径建议 (后端 API)                │
          │                                         │
          │   ├─ 有建议 → 显示建议列表               │
          │   │   → 用户可以点击选择                 │
          │   │                                     │
          │   └─ 无建议 → 尝试导航                   │
          │       ├─ 路径存在 → 导航成功             │
          │       └─ 路径不存在 → 显示错误           │
          │                                         │
          └─────────────────────────────────────────┘
```

---

## 四、组件设计

### 新增状态

```typescript
// 路径建议列表状态
const [suggestions, setSuggestions] = useState<string[]>([]);
const [showSuggestions, setShowSuggestions] = useState(false);
const [selectedSuggestionIdx, setSelectedSuggestionIdx] = useState<number | null>(null);
```

---

### 新增函数

**1. 获取路径建议**

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
  } catch (err) {
    console.error("[FileManager] 获取建议失败:", err);
    setSuggestions([]);
    setShowSuggestions(false);
  }
}, [activeServerId]);
```

**2. 选择建议项**

```typescript
const handleSelectSuggestion = useCallback((suggestion: string) => {
  setPathInput(suggestion);
  setShowSuggestions(false);
  setIsEditingPath(false);
  navigateTo(suggestion);
}, [navigateTo]);
```

---

### 防抖实现

```typescript
// 防抖：输入停止 300ms 后获取建议或导航
useEffect(() => {
  if (!isEditingPath || pathInput === currentPath) {
    return;
  }

  const timer = setTimeout(() => {
    // 获取路径建议
    fetchSuggestions(pathInput);

    // 如果没有建议，尝试导航
    if (suggestions.length === 0) {
      navigateTo(pathInput);
    }
  }, 300);

  return () => clearTimeout(timer);
}, [pathInput, isEditingPath, currentPath, fetchSuggestions, suggestions.length, navigateTo]);
```

---

### 键盘事件处理

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

---

### 全局键盘处理修复

```typescript
const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
  // 如果正在编辑路径，不处理任何全局键盘事件
  if (isEditingPath) return;

  // 如果正在编辑文件名，不处理任何键盘事件
  if (editingEntry) return;

  if (e.key === "Backspace") { e.preventDefault(); goUp(); }
  // ... 其他键盘处理
}, [isEditingPath, editingEntry, goUp, ...]);
```

---

## 五、错误处理

### 路径不存在时的处理

**策略**：
- 不改变 `currentPath`（保持当前目录）
- 输入框保持用户输入的错误路径
- 显示错误提示（Toast 或输入框下方）
- 用户可以继续修改或按 Escape 恢复

**实现**：

```typescript
// navigateTo 函数中的错误处理
const navigateTo = useCallback(async (path: string) => {
  setLoading(true);
  setError(null);

  try {
    // 尝试加载目录
    const resp = await invoke<ReadDirResponse>("remote_read_dir", {
      serverId: activeServerId,
      path,
    });

    // 成功：更新路径和文件列表
    setCurrentPath(path);
    setEntries(resp.entries);
    setHistory([...history.slice(0, historyIdx + 1), path]);
    setHistoryIdx(history.length);
  } catch (err) {
    // 失败：显示错误但不改变 currentPath
    setError(`路径不存在: ${path}`);
    // 不更新 currentPath，保持当前目录
  } finally {
    setLoading(false);
  }
}, [activeServerId, history, historyIdx]);
```

---

### 建议列表为空时的处理

**策略**：
- 如果没有建议，尝试直接导航到输入的路径
- 如果路径不存在，显示错误提示

**实现**：

```typescript
// 防抖 useEffect 中
useEffect(() => {
  if (!isEditingPath || pathInput === currentPath) return;

  const timer = setTimeout(async () => {
    const suggestions = await fetchSuggestions(pathInput);

    if (suggestions.length === 0) {
      // 没有建议，尝试导航
      try {
        await navigateTo(pathInput);
      } catch (err) {
        // 路径不存在，显示错误
        setError(`路径不存在: ${pathInput}`);
      }
    }
  }, 300);

  return () => clearTimeout(timer);
}, [pathInput, isEditingPath, currentPath]);
```

---

## 六、UI 设计

### 建议列表样式（GNOME 风格）

```css
/* FileManager.css */

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

---

### 渲染代码

```tsx
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
```

---

## 七、测试

### 测试场景

**1. 实时导航测试**

| 测试项 | 输入 | 预期结果 |
|--------|------|----------|
| 输入存在的路径 | `/home/user` | 300ms 后自动导航到 `/home/user` |
| 输入不存在的路径 | `/home/nonexistent` | 显示错误提示，保持当前目录 |
| 输入部分路径 | `/home/us` | 显示建议列表 `/home/user` 等 |
| 快速输入 | 快速输入 `/home/user/Documents` | 只在停止 300ms 后触发一次导航 |

---

**2. 建议列表交互测试**

| 测试项 | 操作 | 预期结果 |
|--------|------|----------|
| 显示建议列表 | 输入 `/home/us` | 显示包含 `/home/user` 的建议列表 |
| 点击建议项 | 点击 `/home/user` | 立即导航到 `/home/user`，关闭建议列表 |
| 按 Tab | 输入后按 Tab | 选择第一个建议项并导航 |
| 按 ↑↓ | 输入后按 ↑↓ | 在建议列表中导航，高亮选中项 |
| 按 Enter（有建议） | 输入后按 Enter | 导航到当前输入的路径（忽略建议） |
| 按 Escape | 输入后按 Escape | 关闭建议列表，恢复原路径 |

---

**3. Backspace 键测试**

| 测试项 | 操作 | 预期结果 |
|--------|------|----------|
| 输入框聚焦时按 Backspace | 输入 `/home/us` 后按 Backspace | 删除最后一个字符，变为 `/home/u` |
| 输入框失焦时按 Backspace | 文件列表聚焦时按 Backspace | 返回上级目录 |
| 输入框聚焦时按多次 Backspace | 输入 `/home/user` 后按多次 Backspace | 逐个删除字符，最终变为 `/` |

---

**4. 错误处理测试**

| 测试项 | 输入 | 预期结果 |
|--------|------|----------|
| 无效路径 | `/invalid/path` | 显示错误提示，不改变当前目录 |
| 网络错误 | 输入路径时网络断开 | 显示网络错误提示 |
| 权限不足 | 输入 `/root`（无权限） | 显示权限错误提示 |

---

### 测试方法

**手动测试**：
- 在开发环境中手动测试所有场景
- 使用不同的路径输入测试实时导航
- 测试建议列表的交互（点击、键盘导航）

---

## 八、后端 API

### 新增 Tauri Command

**remote_get_path_suggestions**

```rust
#[tauri::command]
async fn remote_get_path_suggestions(
    server_id: String,
    path: String,
) -> Result<Vec<String>, String> {
    // 通过 QUIC 连接获取路径建议
    // 返回匹配的路径列表
}
```

**实现逻辑**：
- 解析输入路径，获取父目录
- 列出父目录下的所有子目录
- 过滤匹配输入路径前缀的目录
- 返回建议列表（最多 10 个）

---

## 九、实施步骤

### 步骤 1：修复 Backspace 问题
- 在全局键盘处理中添加 `isEditingPath` 检查
- 确保输入框聚焦时不拦截 Backspace

### 步骤 2：实现路径建议列表
- 新增状态：`suggestions`, `showSuggestions`, `selectedSuggestionIdx`
- 实现 `fetchSuggestions` 函数
- 添加建议列表 UI 组件

### 步骤 3：实现防抖导航
- 添加防抖 useEffect
- 输入停止 300ms 后获取建议或导航

### 步骤 4：实现键盘交互
- 处理 Tab、↑↓、Enter、Escape 键
- 支持键盘导航建议列表

### 步骤 5：实现后端 API
- 新增 `remote_get_path_suggestions` Tauri Command
- 在 Agent 中实现路径建议逻辑

### 步骤 6：测试
- 手动测试所有场景
- 修复发现的问题

---

## 十、注意事项

### 性能优化
- 使用防抖避免频繁的后端请求
- 建议列表最多显示 10 个，避免 UI 过长

### 用户体验
- 错误提示清晰，不改变当前目录
- 建议列表支持键盘导航，符合 GNOME HIG
- 输入框聚焦时阻止全局键盘处理

### 兼容性
- 保持现有的导航按钮功能（← → ↑）
- 保持历史记录功能
- 保持侧边栏导航功能

---

## 十一、总结

本设计实现了文件管理器地址栏的实时导航功能，结合了路径建议列表和自动导航，符合 GNOME HIG 的交互规范，并解决了 Backspace 键被拦截的问题。

**核心改进**：
1. 实时导航：输入停止 300ms 后自动导航
2. 路径建议：显示匹配的路径建议列表
3. 键盘交互：支持 Tab、↑↓、Enter、Escape 键
4. Backspace 修复：输入框聚焦时可以删除文本

**预期效果**：
- 用户输入路径时，可以实时看到建议列表
- 用户可以选择建议项立即导航
- 输入框聚焦时，Backspace 可以删除文本
- 错误路径不会改变当前目录，用户体验友好