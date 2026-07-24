# Phase 4: 客户端集成实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** 完成客户端侧的认证集成，支持SSH公钥和密码认证，实现完整的端到端功能。

**Architecture:** 客户端配置认证信息 → QUIC连接 → 认证请求 → 会话建立 → 业务操作。

**Tech Stack:** TypeScript (React), Tauri, Rust

---

## File Structure

### 客户端侧新增文件

无新增文件，仅修改现有文件。

### 客户端侧修改文件

| 文件路径 | 改动内容 |
|---------|---------|
| `src/types/server.ts` | 添加认证相关类型定义 |
| `src/stores/serversStore.ts` | 添加认证字段到ServerConfig |
| `src/apps/Settings.tsx` | 添加认证配置UI |
| `src/context/ServerManager.tsx` | 修改连接逻辑传递凭据 |
| `src-tauri/src/connection.rs` | 修改connect命令接受凭据 |
| `src-tauri/Cargo.toml` | 添加russh依赖 |

---

## Phase 4任务

### Task 14: 添加认证类型定义

**Files:**
- Modify: `src/types/server.ts`

- [ ] **Step 1: 定义认证相关类型**

修改文件 `src/types/server.ts`：

```typescript
// 认证方式枚举
export enum AuthMethod {
  PASSWORD = 'password',
  PUBKEY = 'pubkey',
  KEYBOARD_INTERACTIVE = 'keyboard-interactive',
}

// 认证凭据
export interface AuthCredentials {
  method: AuthMethod;
  username: string;
  password?: string; // 加密存储
  privateKey?: string; // 加密存储
  passphrase?: string; // 私钥密码（可选）
}

// 扩展ServerConfig
export interface ServerConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  
  // 新增：认证配置
  auth: AuthCredentials;
  
  // 其他字段...
  useSSL: boolean;
  autoConnect: boolean;
}
```

- [ ] **Step 2: 提交类型定义**

```bash
git add src/types/server.ts
git commit -m "feat(client): 添加认证类型定义"
```

---

### Task 15: 更新服务器Store

**Files:**
- Modify: `src/stores/serversStore.ts`

- [ ] **Step 1: 添加认证字段**

修改文件 `src/stores/serversStore.ts`：

```typescript
import { ServerConfig, AuthMethod } from '../types/server';

interface ServersStore {
  servers: ServerConfig[];
  activeServer: string | null;
  
  // 添加方法
  addServer: (server: ServerConfig) => void;
  updateServer: (id: string, updates: Partial<ServerConfig>) => void;
  deleteServer: (id: string) => void;
}

// 示例服务器配置（更新）
const defaultServer: ServerConfig = {
  id: 'default',
  name: 'Default Server',
  host: 'localhost',
  port: 8443,
  
  // 新增：默认认证配置
  auth: {
    method: AuthMethod.PASSWORD,
    username: 'user',
    password: undefined,
    privateKey: undefined,
    passphrase: undefined,
  },
  
  useSSL: true,
  autoConnect: false,
};
```

- [ ] **Step 2: 提交Store更新**

```bash
git add src/stores/serversStore.ts
git commit -m "feat(client): 添加认证字段到Store"
```

---

### Task 16: 实现认证配置UI

**Files:**
- Modify: `src/apps/Settings.tsx`

- [ ] **Step 1: 添加认证配置表单**

修改文件 `src/apps/Settings.tsx`：

```typescript
import { AuthMethod } from '../types/server';

function Settings() {
  const [authMethod, setAuthMethod] = useState<AuthMethod>(AuthMethod.PASSWORD);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [passphrase, setPassphrase] = useState('');

  const renderAuthFields = () => {
    switch (authMethod) {
      case AuthMethod.PASSWORD:
        return (
          <div>
            <div className="form-field">
              <label>用户名</label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="输入用户名"
              />
            </div>
            <div className="form-field">
              <label>密码</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="输入密码"
              />
            </div>
          </div>
        );
      
      case AuthMethod.PUBKEY:
        return (
          <div>
            <div className="form-field">
              <label>用户名</label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="输入用户名"
              />
            </div>
            <div className="form-field">
              <label>私钥文件</label>
              <input
                type="file"
                onChange={(e) => {
                  // 读取私钥文件
                  const file = e.target.files?.[0];
                  if (file) {
                    const reader = new FileReader();
                    reader.onload = (e) => {
                      setPrivateKey(e.target?.result as string);
                    };
                    reader.readAsText(file);
                  }
                }}
              />
            </div>
            <div className="form-field">
              <label>私钥密码（可选）</label>
              <input
                type="password"
                value={passphrase}
                onChange={(e) => setPassphrase(e.target.value)}
                placeholder="输入私钥密码"
              />
            </div>
          </div>
        );
      
      default:
        return null;
    }
  };

  return (
    <div className="settings">
      <h2>服务器配置</h2>
      
      {/* 认证方式选择 */}
      <div className="form-field">
        <label>认证方式</label>
        <select
          value={authMethod}
          onChange={(e) => setAuthMethod(e.target.value as AuthMethod)}
        >
          <option value={AuthMethod.PASSWORD}>密码认证</option>
          <option value={AuthMethod.PUBKEY}>公钥认证</option>
        </select>
      </div>
      
      {/* 认证字段 */}
      {renderAuthFields()}
    </div>
  );
}
```

- [ ] **Step 2: 添加样式**

确保认证表单样式正确。

- [ ] **Step 3: 提交UI实现**

```bash
git add src/apps/Settings.tsx
git commit -m "feat(client): 实现认证配置UI"
```

---

### Task 17: 修改连接逻辑

**Files:**
- Modify: `src/context/ServerManager.tsx`

- [ ] **Step 1: 更新连接函数**

修改文件 `src/context/ServerManager.tsx`：

```typescript
import { invoke } from '@tauri-apps/api/tauri';
import { ServerConfig } from '../types/server';

async function connectServer(config: ServerConfig) {
  try {
    // 调用Tauri后端连接
    const sessionId = await invoke<string>('connect_server', {
      host: config.host,
      port: config.port,
      credentials: {
        method: config.auth.method,
        username: config.auth.username,
        password: config.auth.password,
        private_key: config.auth.privateKey,
        passphrase: config.auth.passphrase,
      },
    });
    
    console.log('连接成功，session_id:', sessionId);
    return sessionId;
  } catch (error) {
    console.error('连接失败:', error);
    throw error;
  }
}
```

- [ ] **Step 2: 提交连接逻辑修改**

```bash
git add src/context/ServerManager.tsx
git commit -m "feat(client): 修改连接逻辑传递凭据"
```

---

### Task 18: 实现Tauri后端连接命令

**Files:**
- Modify: `src-tauri/src/connection.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 添加russh依赖**

修改文件 `src-tauri/Cargo.toml`：

```toml
[dependencies]
russh = "0.42"
russh-keys = "0.42"
```

- [ ] **Step 2: 定义认证结构**

修改文件 `src-tauri/src/connection.rs`：

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    Pubkey,
    KeyboardInteractive,
}

#[derive(Debug, Deserialize)]
pub struct Credentials {
    method: AuthMethod,
    username: String,
    password: Option<String>,
    private_key: Option<String>,
    passphrase: Option<String>,
}
```

- [ ] **Step 3: 实现连接命令**

```rust
use tauri::command;

#[command]
pub async fn connect_server(
    host: String,
    port: u16,
    credentials: Credentials,
) -> Result<String, String> {
    // 1. 建立QUIC连接
    let conn = quic_connect(&host, port)
        .await
        .map_err(|e| e.to_string())?;
    
    // 2. 根据认证方式执行认证
    let session_id = match credentials.method {
        AuthMethod::Password => {
            authenticate_with_password(&conn, &credentials)
                .await
                .map_err(|e| e.to_string())?
        }
        AuthMethod::Pubkey => {
            authenticate_with_pubkey(&conn, &credentials)
                .await
                .map_err(|e| e.to_string())?
        }
        AuthMethod::KeyboardInteractive => {
            return Err("暂不支持键盘交互认证".to_string());
        }
    };
    
    // 3. 返回session_id
    Ok(session_id)
}

async fn authenticate_with_password(
    conn: &quinn::Connection,
    credentials: &Credentials,
) -> Result<String, anyhow::Error> {
    // 实现密码认证逻辑
    // ...
}

async fn authenticate_with_pubkey(
    conn: &quinn::Connection,
    credentials: &Credentials,
) -> Result<String, anyhow::Error> {
    // 实现公钥认证逻辑
    // ...
}
```

- [ ] **Step 4: 注册命令**

在 `src-tauri/src/main.rs` 中：

```rust
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ... 其他命令
            connection::connect_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: 提交Tauri后端实现**

```bash
git add src-tauri/src/connection.rs
git add src-tauri/src/main.rs
git add src-tauri/Cargo.toml
git commit -m "feat(client): 实现Tauri连接命令和认证逻辑"
```

---

## 执行建议

Phase 4是客户端集成，建议：
1. 分步骤实施，每步验证功能
2. 注意敏感信息安全存储（使用Tauri安全存储API）
3. 测试不同认证方式
4. 确保UI用户体验良好

**下一步**: 开始Task 14（添加认证类型定义）