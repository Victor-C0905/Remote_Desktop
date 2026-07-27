# 通用SSH密钥处理系统 — 设计文档

> 版本: v1.0 | 日期: 2026-07-27 | 状态: **设计阶段**
>
> 核心定位：参考通用工具（MobaXterm、PuTTY、OpenSSH）的密钥处理方式，实现一个兼容所有主流SSH密钥格式的通用解析系统。

---

## 一、现有工具调研

### 1.1 MobaXterm 密钥处理方式

**支持的格式：**
- ✅ OpenSSH 格式（推荐）
- ✅ PuTTY 格式（.ppk）
- ✅ SSH.com 格式
- ✅ SSH-1 和 SSH-2 密钥

**处理流程：**
```
1. 用户选择密钥文件
2. 自动检测密钥格式
3. 如果是加密密钥，弹出密码输入框
4. 验证密钥有效性
5. 使用密钥进行认证
```

**特点：**
- 自动格式检测（无需用户指定）
- 支持密钥转换功能
- 记住常用密钥
- 支持密钥代理（Pageant集成）

### 1.2 PuTTY 密钥处理方式

**支持的格式：**
- ✅ PuTTY 私钥格式（.ppk）- 原生格式
- ✅ OpenSSH 格式（通过导入功能）
- ✅ SSH-1 和 SSH-2 RSA密钥

**处理流程：**
```
1. 用户手动指定密钥文件
2. 必须使用 .ppk 格式
3. 如果是其他格式，需要使用 PuTTYgen 转换
4. 解密密钥（如果加密）
5. 进行认证
```

**特点：**
- 严格使用 .ppk 格式
- 提供密钥转换工具（PuTTYgen）
- 支持密钥代理（Pageant）
- 支持交互式密码输入

### 1.3 OpenSSH 密钥处理方式

**支持的格式：**
- ✅ OpenSSH 私钥格式（推荐，默认）
- ✅ PEM 格式（兼容旧版本）
  - PKCS#1 RSA
  - PKCS#1 DSA
  - SEC1 ECDSA
  - PKCS#8（RSA/ECDSA/Ed25519）
- ✅ SSH.com 格式（通过转换）

**处理流程：**
```
1. 自动检测密钥格式
2. 尝试解析密钥
   ├─ OpenSSH 格式（优先）
   └─ PEM 格式（回退）
3. 如果是加密密钥：
   ├─ 尝试使用 ssh-agent 解密
   └─ 否则提示输入密码
4. 验证密钥有效性
5. 进行认证
```

**特点：**
- 自动格式检测
- 支持多种加密算法（AES-256-CBC、AES-128-CBC等）
- 支持密钥代理（ssh-agent）
- 支持密钥转换（ssh-keygen -i/-m）

### 1.4 云服务商密钥（阿里云、AWS、Azure）

**阿里云密钥特点：**
- 默认提供 PEM 格式（PKCS#1 RSA）
- 通常未加密（但有加密选项）
- 直接兼容 OpenSSH

**AWS密钥特点：**
- 提供 PEM 格式
- 支持 RSA 和 ED25519
- 未加密

**Azure密钥特点：**
- 提供 PEM 格式
- 支持 RSA 和 ECDSA
- 可能加密

---

## 二、当前问题分析

### 2.1 当前实现的局限

| 问题 | 现状 | 影响 |
|------|------|------|
| PEM 格式支持不完整 | 仅支持未加密的 RSA | 加密私钥无法使用 |
| 缺少格式自动检测 | 需要用户知道格式 | 用户体验差 |
| 错误提示不友好 | 技术性错误信息 | 用户难以理解 |
| 缺少密钥转换指导 | 无 | 用户不知道如何处理不支持的格式 |

### 2.2 用户痛点

1. **阿里云用户**：
   - PEM格式私钥，有时加密
   - 不知道如何转换为OpenSSH格式
   - 错误信息看不懂

2. **Windows用户**：
   - 可能使用PuTTY（.ppk格式）
   - 需要先转换为OpenSSH格式
   - 不知道如何操作

3. **旧版OpenSSH用户**：
   - PEM格式私钥（OpenSSH 6.5之前）
   - 可能有加密
   - 需要转换或解密

---

## 三、设计方案

### 3.1 核心原则

参考 MobaXterm 和 OpenSSH 的设计理念：

1. **自动检测**：无需用户指定格式，自动识别
2. **最大兼容**：支持所有主流格式
3. **友好提示**：提供清晰的错误提示和解决方案
4. **渐进支持**：优先支持常用格式，逐步完善

### 3.2 支持的密钥格式（优先级）

| 优先级 | 格式 | 加密支持 | 使用场景 | 实现复杂度 |
|--------|------|----------|----------|-----------|
| P0 | OpenSSH | ✅ 完整 | 现代Linux默认 | 低 |
| P0 | PEM PKCS#1 RSA | ⚠️ 待实现 | 阿里云、AWS | 中 |
| P1 | PEM PKCS#8 | ⚠️ 待实现 | 通用PEM格式 | 中 |
| P1 | PEM SEC1 ECDSA | ⚠️ 待实现 | ECDSA密钥 | 中 |
| P2 | PuTTY .ppk | ❌ 需转换 | Windows用户 | 高 |
| P2 | SSH.com | ❌ 需转换 | 旧系统 | 高 |

**P0 = 必须支持（核心功能）**
**P1 = 重要但非紧急**
**P2 = 附加功能**

### 3.3 整体架构

```rust
// 统一的密钥解析接口
pub trait KeyParser {
    fn parse(&self, key_data: &str, passphrase: Option<&str>) -> Result<PrivateKey>;
    fn can_parse(&self, key_data: &str) -> bool;
    fn format_name(&self) -> &'static str;
}

// 自动格式检测解析器
pub struct AutoDetectKeyParser {
    parsers: Vec<Box<dyn KeyParser>>,
}

impl AutoDetectKeyParser {
    pub fn parse_auto(&self, key_data: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
        // 1. 尝试自动检测格式
        for parser in &self.parsers {
            if parser.can_parse(key_data) {
                match parser.parse(key_data, passphrase) {
                    Ok(key) => {
                        tracing::info!("[KeyParser] 成功解析 {} 格式密钥", parser.format_name());
                        return Ok(key);
                    }
                    Err(e) => {
                        tracing::debug!("[KeyParser] {} 格式解析失败: {}", parser.format_name(), e);
                        // 继续尝试下一个格式
                    }
                }
            }
        }

        // 2. 所有格式都失败，返回友好错误
        Err(format!(
            "无法识别私钥格式。支持的格式：OpenSSH、PEM (RSA/ECDSA/PKCS#8)\n\
             如果您的密钥是PuTTY格式（.ppk），请使用PuTTYgen转换为OpenSSH格式。\n\
             如果您的密钥有密码保护，请输入密码。"
        ))
    }
}
```

### 3.4 具体实现

#### OpenSSH 格式解析器

```rust
pub struct OpenSshKeyParser;

impl KeyParser for OpenSshKeyParser {
    fn format_name(&self) -> &'static str {
        "OpenSSH"
    }

    fn can_parse(&self, key_data: &str) -> bool {
        key_data.contains("-----BEGIN OPENSSH PRIVATE KEY-----")
    }

    fn parse(&self, key_data: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
        if let Some(pwd) = passphrase {
            PrivateKey::from_openssh(key_data)?
                .decrypt(pwd.as_bytes())
                .map_err(|e| format!("私钥解密失败（密码错误）: {}", e))
        } else {
            PrivateKey::from_openssh(key_data)
                .map_err(|e| format!("私钥解析失败: {}（如果私钥有密码保护，请输入密码）", e))
        }
    }
}
```

#### PEM RSA 格式解析器

```rust
pub struct PemRsaKeyParser;

impl KeyParser for PemRsaKeyParser {
    fn format_name(&self) -> &'static str {
        "PEM RSA"
    }

    fn can_parse(&self, key_data: &str) -> bool {
        key_data.contains("-----BEGIN RSA PRIVATE KEY-----")
            || key_data.contains("-----BEGIN PRIVATE KEY-----")
    }

    fn parse(&self, key_data: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
        // 1. 尝试 PKCS#8
        if key_data.contains("-----BEGIN PRIVATE KEY-----") {
            return self.parse_pkcs8(key_data, passphrase);
        }

        // 2. 尝试 PKCS#1
        self.parse_pkcs1(key_data, passphrase)
    }

    fn parse_pkcs1(&self, key_data: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
        if passphrase.is_some() {
            return Err("加密的PKCS#1私钥暂不支持。请使用以下命令解密：\nssh-keygen -p -f <私钥文件>".to_string());
        }

        let rsa_key = RsaPrivateKey::from_pkcs1_pem(key_data)
            .map_err(|e| format!("PKCS#1 RSA 解析失败: {}", e))?;

        let rsa_keypair = RsaKeypair::try_from(&rsa_key)
            .map_err(|e| format!("转换为SSH密钥失败: {}", e))?;

        PrivateKey::new(KeypairData::Rsa(rsa_keypair), "")
            .map_err(|e| format!("构造SSH私钥失败: {}", e))
    }
}
```

#### 加密PEM密钥处理

```rust
pub struct EncryptedPemKeyParser;

impl KeyParser for EncryptedPemKeyParser {
    fn format_name(&self) -> &'static str {
        "PEM (Encrypted)"
    }

    fn can_parse(&self, key_data: &str) -> bool {
        key_data.contains("ENCRYPTED")
    }

    fn parse(&self, key_data: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
        let pwd = passphrase.ok_or("加密私钥需要密码。请在'私钥密码'字段输入密码。")?;

        // 解密逻辑（需要实现PKCS#8解密）
        // ...
    }
}
```

### 3.5 用户友好的错误提示

```rust
pub fn suggest_solution(error: &str, key_data: &str) -> String {
    if error.contains("PEM") && error.contains("Base64") {
        return "私钥格式错误。可能的原因和解决方案：\n\
                1. 私钥文件损坏 - 请重新下载私钥文件\n\
                2. 使用了不支持的格式（如PuTTY的.ppk）- 请转换为OpenSSH格式\n\
                3. 私钥有密码保护 - 请在'私钥密码'字段输入密码\n\n\
                转换方法：\n\
                - PuTTY (.ppk) 转 OpenSSH: 使用 PuTTYgen 导出为 OpenSSH 格式\n\
                - PEM 转 OpenSSH: ssh-keygen -i -f <PEM私钥> > id_rsa".to_string();
    }

    if error.contains("ENCRYPTED") {
        return "私钥已加密。请在'私钥密码'字段输入密码。\n\
                如果忘记密码，可以尝试重新下载私钥文件（云服务商）。".to_string();
    }

    // 默认提示
    "私钥解析失败。请检查：\n\
     1. 私钥文件是否完整\n\
     2. 私钥格式是否为OpenSSH或PEM（支持RSA/ECDSA）\n\
     3. 如果私钥有密码，请在'私钥密码'字段输入\n\n\
     支持的格式：OpenSSH、PEM (RSA/ECDSA/PKCS#8)\n\
     不支持的格式：PuTTY (.ppk)、SSH.com".to_string()
}
```

---

## 四、实施计划

### 4.1 Phase 1：核心功能（1周）

**目标：支持所有未加密的常见格式**

- ✅ OpenSSH 格式（已有）
- ✅ PEM PKCS#1 RSA（已实现）
- ⚠️ PEM PKCS#8（需要完善）
- ⚠️ PEM SEC1 ECDSA（需要实现）
- ⚠️ 自动格式检测
- ⚠️ 友好的错误提示

**交付物：**
- 支持阿里云、AWS等云服务商的未加密密钥
- 自动识别密钥格式
- 清晰的错误提示和解决方案

### 4.2 Phase 2：加密密钥支持（1周）

**目标：支持加密的PEM密钥**

- ⚠️ PKCS#8 加密密钥解密
- ⚠️ PKCS#1 加密密钥解密（可能需要第三方库）
- ⚠️ 交互式密码输入

**挑战：**
- Rust加密库对PKCS#1加密支持有限
- 可能需要使用OpenSSL或其他C库

### 4.3 Phase 3：高级功能（可选）

**目标：密钥转换和代理支持**

- ⚠️ PuTTY .ppk 格式支持（或提供转换工具）
- ⚠️ 密钥转换功能（PEM ↔ OpenSSH）
- ⚠️ ssh-agent 集成（可选）

---

## 五、风险与缓解

| 风险 | 等级 | 缓解措施 |
|------|------|---------|
| PKCS#1加密支持困难 | 高 | 提供解密指导，建议用户先解密 |
| PEM格式变种多 | 中 | 优先支持常见格式，逐步完善 |
| 用户不理解密钥格式 | 中 | 提供详细的错误提示和教程 |
| 性能问题（多次尝试） | 低 | 按优先级排序，尽早返回 |

---

## 六、总结

### 6.1 设计理念

**向 MobaXterm 和 OpenSSH 学习：**
- 自动检测，无需用户知道格式
- 最大兼容，支持主流格式
- 友好提示，帮助用户解决问题

### 6.2 实施优先级

**立即实现（Phase 1）：**
- ✅ OpenSSH 格式
- ✅ PEM 未加密格式（PKCS#1/PKCS#8）
- ✅ 自动格式检测
- ✅ 友好错误提示

**短期实现（Phase 2）：**
- ⚠️ PEM 加密密钥支持
- ⚠️ 更完善的错误处理

**长期实现（Phase 3）：**
- ⚠️ PuTTY .ppk 支持
- ⚠️ 密钥转换功能

### 6.3 用户价值

- **降低使用门槛**：自动识别格式，无需专业知识
- **兼容主流云服务商**：直接使用阿里云、AWS等提供的密钥
- **清晰的问题解决指导**：遇到问题能快速找到解决方案