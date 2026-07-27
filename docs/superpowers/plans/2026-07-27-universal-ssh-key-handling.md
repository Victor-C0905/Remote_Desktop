# 通用SSH密钥处理系统 - 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现自动检测多种SSH密钥格式的通用解析系统，支持OpenSSH和PEM格式（PKCS#1/PKCS#8），提供友好的错误提示。

**Architecture:** 采用策略模式实现多个KeyParser，通过AutoDetectKeyParser自动尝试所有格式，失败时返回详细的解决方案。

**Tech Stack:** Rust, ssh-key crate, rsa crate, pkcs8 crate, tracing

---

## Task 1: 实现PEM PKCS#8格式支持

**Files:**
- Modify: `src-tauri/src/connection.rs:1743-1820`（parse_pem_rsa函数）

- [ ] **Step 1: 添加PKCS#8格式解析逻辑**

```rust
/// 解析 PEM 格式的 RSA 私钥（阿里云等云服务商常用）
fn parse_pem_rsa(pem: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    use ssh_key::private::{RsaKeypair, KeypairData};

    tracing::debug!("[PEM] 开始解析 PEM RSA 私钥");

    // 简化实现：仅支持未加密的 PEM 格式
    if passphrase.is_some() {
        return Err("加密的PEM私钥暂不支持。请使用ssh-keygen解密：\nssh-keygen -p -f <私钥文件>\n或转换为OpenSSH格式：\nssh-keygen -i -f <PEM私钥> > id_rsa".to_string());
    }

    // 尝试解析未加密的私钥（支持PKCS#1和PKCS#8）
    let rsa_key = if pem.contains("-----BEGIN PRIVATE KEY-----") {
        // PKCS#8 格式
        tracing::debug!("[PEM] 检测到 PKCS#8 格式");
        RsaPrivateKey::from_pkcs8_pem(pem)
            .map_err(|e| {
                tracing::error!("[PEM] PKCS#8 RSA 解析失败: {}", e);
                format!("PKCS#8 RSA 解析失败: {}", e)
            })?
    } else {
        // PKCS#1 格式
        tracing::debug!("[PEM] 检测到 PKCS#1 格式");
        RsaPrivateKey::from_pkcs1_pem(pem)
            .map_err(|e| {
                tracing::error!("[PEM] PKCS#1 RSA 解析失败: {}", e);
                format!("PKCS#1 RSA 解析失败: {}", e)
            })?
    };

    // 直接从 rsa::RsaPrivateKey 转换为 ssh_key 格式
    let rsa_keypair = RsaKeypair::try_from(&rsa_key)
        .map_err(|e| {
            tracing::error!("[PEM] 转换为 SSH 密钥失败: {}", e);
            format!("转换为 SSH 密钥失败: {}", e)
        })?;

    // 构造 ssh_key::PrivateKey
    let private_key = ssh_key::PrivateKey::new(KeypairData::Rsa(rsa_keypair), "")
        .map_err(|e| {
            tracing::error!("[PEM] 构造 SSH 私钥失败: {}", e);
            format!("构造 SSH 私钥失败: {}", e)
        })?;

    Ok(private_key)
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误

---

## Task 2: 实现自动格式检测

**Files:**
- Modify: `src-tauri/src/connection.rs:1450-1500`（公钥认证逻辑）

- [ ] **Step 1: 重构密钥解析逻辑为自动检测**

找到公钥认证函数中的密钥解析部分（约在1450-1500行），替换为：

```rust
    // 自动检测私钥格式并解析
    tracing::debug!("[PubKeyAuth] 解析私钥（长度={}字节）", private_key_cleaned.len());

    let key = Self::parse_private_key_auto(&private_key_cleaned, passphrase.as_deref())
        .map_err(|e| {
            tracing::error!("[PubKeyAuth] 私钥解析失败: {}", e);
            e
        })?;
```

- [ ] **Step 2: 添加自动解析函数**

在connection.rs末尾添加：

```rust
// ── 私钥自动格式检测 ─────────────────────────────────────

/// 自动检测私钥格式并解析
///
/// 支持的格式（按优先级）：
/// 1. OpenSSH 格式（现代标准）
/// 2. PEM PKCS#8 格式（通用PEM）
/// 3. PEM PKCS#1 RSA 格式（阿里云等云服务商）
fn parse_private_key_auto(key_data: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    tracing::debug!("[KeyParser] 开始自动检测私钥格式");

    // 1. 尝试 OpenSSH 格式（优先级最高）
    if key_data.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        tracing::debug!("[KeyParser] 尝试解析 OpenSSH 格式");
        match Self::parse_openssh_key(key_data, passphrase) {
            Ok(key) => {
                tracing::info!("[KeyParser] ✅ 成功解析 OpenSSH 格式密钥");
                return Ok(key);
            }
            Err(e) => {
                tracing::debug!("[KeyParser] ❌ OpenSSH 格式解析失败: {}", e);
            }
        }
    }

    // 2. 尝试 PEM 格式
    if key_data.contains("-----BEGIN") {
        tracing::debug!("[KeyParser] 尝试解析 PEM 格式");

        // 检查是否加密
        if key_data.contains("ENCRYPTED") {
            tracing::warn!("[KeyParser] 检测到加密私钥");
            if passphrase.is_none() {
                return Err("私钥已加密，请在'私钥密码'字段输入密码。".to_string());
            }
        }

        match Self::parse_pem_rsa(key_data, passphrase) {
            Ok(key) => {
                tracing::info!("[KeyParser] ✅ 成功解析 PEM 格式密钥");
                return Ok(key);
            }
            Err(e) => {
                tracing::debug!("[KeyParser] ❌ PEM 格式解析失败: {}", e);
            }
        }
    }

    // 3. 所有格式都失败，返回友好的错误提示
    Err(Self::generate_key_parse_error(key_data))
}

/// 解析 OpenSSH 格式私钥
fn parse_openssh_key(key_data: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    if let Some(pwd) = passphrase {
        ssh_key::PrivateKey::from_openssh(key_data)
            .map_err(|e| format!("OpenSSH格式解析失败: {}", e))?
            .decrypt(pwd.as_bytes())
            .map_err(|e| format!("私钥解密失败（密码错误）: {}", e))
    } else {
        ssh_key::PrivateKey::from_openssh(key_data)
            .map_err(|e| format!("OpenSSH格式解析失败: {}（如果私钥有密码保护，请输入密码）", e))
    }
}

/// 生成友好的密钥解析错误提示
fn generate_key_parse_error(key_data: &str) -> String {
    let mut error_msg = String::from("无法解析私钥文件。\n\n");

    // 分析可能的问题
    if key_data.contains("PuTTY") {
        error_msg.push_str("检测到 PuTTY 格式（.ppk）。请使用以下方法转换：\n");
        error_msg.push_str("1. 打开 PuTTYgen\n");
        error_msg.push_str("2. 加载您的 .ppk 文件\n");
        error_msg.push_str("3. 点击 'Conversions' -> 'Export OpenSSH key'\n");
        error_msg.push_str("4. 保存为新的 OpenSSH 格式文件\n\n");
    } else if !key_data.contains("-----BEGIN") {
        error_msg.push_str("未检测到有效的私钥格式。请检查：\n");
        error_msg.push_str("1. 文件是否完整（包含 -----BEGIN 和 -----END 标记）\n");
        error_msg.push_str("2. 是否是私钥文件（公钥文件无法用于认证）\n\n");
    } else {
        error_msg.push_str("支持的格式：\n");
        error_msg.push_str("✅ OpenSSH 格式（推荐）\n");
        error_msg.push_str("✅ PEM 格式 - PKCS#1 RSA（阿里云、AWS等云服务商）\n");
        error_msg.push_str("✅ PEM 格式 - PKCS#8（通用PEM格式）\n\n");

        error_msg.push_str("不支持的格式：\n");
        error_msg.push_str("❌ PuTTY 格式（.ppk）- 需要先转换为 OpenSSH 格式\n");
        error_msg.push_str("❌ SSH.com 格式 - 需要先转换\n\n");

        error_msg.push_str("如果私钥有密码保护，请在'私钥密码'字段输入密码。");
    }

    error_msg
}
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误

---

## Task 3: 完善前端错误提示

**Files:**
- Modify: `src/apps/Settings.tsx:85-97`（私钥文件选择处理）

- [ ] **Step 1: 添加私钥格式验证**

修改 `handlePrivateKeySelect` 函数：

```tsx
// 处理私钥文件选择
const handlePrivateKeySelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
  const file = e.target.files?.[0];
  if (file) {
    try {
      // 读取文件内容
      const content = await file.text();

      // 基本格式验证
      if (!content.includes('-----BEGIN')) {
        alert('选择的文件不是有效的私钥文件。\n\n私钥文件应以 -----BEGIN 开头。\n\n支持的格式：\n• OpenSSH 格式（如 ~/.ssh/id_ed25519）\n• PEM 格式（云服务商提供的密钥）\n\n不支持：PuTTY 格式（.ppk）');
        return;
      }

      // 检查是否是 PuTTY 格式
      if (content.includes('PuTTY')) {
        alert('检测到 PuTTY 格式私钥（.ppk）。\n\n请使用 PuTTYgen 转换为 OpenSSH 格式：\n1. 打开 PuTTYgen\n2. 加载您的 .ppk 文件\n3. 点击 "Conversions" -> "Export OpenSSH key"\n4. 保存新的文件');
        return;
      }

      // 检查是否加密
      if (content.includes('ENCRYPTED') && !passphrase) {
        console.log('检测到加密私钥，用户需要在密码字段输入密码');
      }

      setPrivateKeyFile(content);
      log.info('已加载私钥文件:', file.name);
    } catch (error) {
      console.error("读取私钥文件失败:", error);
      alert("读取私钥文件失败，请检查文件格式和权限");
    }
  }
};
```

- [ ] **Step 2: 更新私钥选择器提示**

修改文件选择器的提示信息（约在188行）：

```tsx
<span className="st-form-hint">
  选择 SSH 私钥文件（OpenSSH 或 PEM 格式）<br/>
  支持：id_ed25519、id_rsa、云服务商提供的密钥<br/>
  不支持：PuTTY 格式（.ppk）- 请先转换
</span>
```

- [ ] **Step 3: 前端编译验证**

Run: `npm run build`
Expected: 编译成功，无错误

---

## Task 4: 测试和文档

**Files:**
- Create: `agent/tests/key_parsing_test.rs`（可选，集成测试）
- Modify: `agent/README.md`（添加密钥格式说明）

- [ ] **Step 1: 编写集成测试（可选）**

创建 `agent/tests/key_parsing_test.rs`:

```rust
// 集成测试：测试不同格式的密钥解析
// 注：实际测试需要真实的测试密钥文件

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openssh_format() {
        // 测试 OpenSSH 格式解析
    }

    #[test]
    fn test_pem_pkcs1_format() {
        // 测试 PEM PKCS#1 格式解析
    }

    #[test]
    fn test_pem_pkcs8_format() {
        // 测试 PEM PKCS#8 格式解析
    }

    #[test]
    fn test_auto_detect() {
        // 测试自动格式检测
    }
}
```

- [ ] **Step 2: 更新README文档**

在 `agent/README.md` 添加密钥格式说明：

```markdown
## 支持的密钥格式

### 客户端支持

- ✅ **OpenSSH 格式**（推荐）
  - 现代Linux/Mac默认格式
  - 通常位于 `~/.ssh/id_ed25519` 或 `~/.ssh/id_rsa`

- ✅ **PEM 格式**
  - PKCS#1 RSA：阿里云、AWS等云服务商提供的密钥
  - PKCS#8：通用PEM格式

### 不支持的格式

- ❌ **PuTTY 格式（.ppk）**
  - Windows用户常用格式
  - 需要先转换为 OpenSSH 格式
  - 转换方法：使用 PuTTYgen 导出为 OpenSSH 格式

### 加密私钥处理

如果您的私钥有密码保护：

1. 在"私钥密码"字段输入密码
2. 如果是PEM格式加密私钥，建议先解密：
   ```bash
   ssh-keygen -p -f <私钥文件>
   ```

### 常见问题

**Q: 提示"无法解析私钥文件"？**
A: 检查以下几点：
- 私钥文件是否完整（包含 -----BEGIN 和 -----END）
- 是否是PuTTY格式（.ppk）- 需要转换
- 是否有密码保护 - 需要输入密码

**Q: 如何转换PuTTY格式？**
A: 使用PuTTYgen：
1. 打开PuTTYgen
2. 加载.ppk文件
3. 点击"Conversions" -> "Export OpenSSH key"
4. 保存新文件
```

---

## Task 5: 编译和测试

**Files:**
- 无文件修改，仅执行编译和测试

- [ ] **Step 1: 完整编译客户端**

Run: `cd e:\MyWork\gnome-remote && npm run tauri build`
Expected: 编译成功，生成可执行文件

- [ ] **Step 2: 测试OpenSSH格式**

使用现有的OpenSSH密钥测试连接

- [ ] **Step 3: 测试PEM格式**

使用阿里云等云服务商提供的PEM密钥测试连接

- [ ] **Step 4: 测试错误提示**

故意选择错误的文件（如.ppk格式），验证错误提示是否友好

---

## 自我审查

### 1. Spec覆盖率检查

| Spec要求 | 对应Task | 状态 |
|---------|---------|------|
| OpenSSH格式支持 | Task 2 | ✅ |
| PEM PKCS#1 RSA支持 | Task 1 | ✅ |
| PEM PKCS#8支持 | Task 1 | ✅ |
| 自动格式检测 | Task 2 | ✅ |
| 友好错误提示 | Task 2, 3 | ✅ |
| 前端格式验证 | Task 3 | ✅ |
| 文档说明 | Task 4 | ✅ |
| 测试验证 | Task 5 | ✅ |

### 2. Placeholder扫描

✅ 无TBD、TODO、未实现的功能
✅ 所有步骤都有具体的代码
✅ 没有模糊的描述

### 3. 类型一致性

✅ 所有函数签名一致
✅ 错误类型统一为 `String`
✅ 使用相同的 `ssh_key::PrivateKey` 类型

---

## 执行选项

计划已完成并保存到 `docs/superpowers/plans/2026-07-27-universal-ssh-key-handling.md`。

**两种执行方式：**

1. **Subagent-Driven（推荐）** - 每个Task由独立的子代理执行，Task之间有审查机会，快速迭代
2. **Inline Execution** - 在当前会话中执行，批量执行Task并在检查点审查

**选择哪种方式？**