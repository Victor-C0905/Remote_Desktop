use std::fs;

fn main() {
    // 从命令行参数获取私钥文件路径
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: {} <私钥文件路径>", args[0]);
        std::process::exit(1);
    }

    let key_path = &args[1];

    // 读取私钥文件
    let content = match fs::read_to_string(key_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取文件失败: {}", e);
            std::process::exit(1);
        }
    };

    println!("=== 私钥文件诊断 ===");
    println!("文件路径: {}", key_path);
    println!("文件大小: {} 字节", content.len());

    // 检查BOM
    let has_bom = content.starts_with('\u{FEFF}');
    println!("UTF-8 BOM: {}", if has_bom { "有（需要清理）" } else { "无" });

    // 检查换行符
    let has_crlf = content.contains("\r\n");
    println!("Windows换行符 (\\r\\n): {}", if has_crlf { "有（需要转换）" } else { "无" });

    // 清理私钥内容
    let cleaned = content
        .strip_prefix('\u{FEFF}')
        .unwrap_or(&content)
        .replace("\r\n", "\n")
        .trim()
        .to_string();

    println!("\n清理后大小: {} 字节", cleaned.len());

    // 显示前150个字符
    let preview_len = std::cmp::min(150, cleaned.len());
    println!("\n前{}字符预览:", preview_len);
    println!("{}", &cleaned[..preview_len]);

    // 尝试解析私钥
    use ssh_key::PrivateKey;
    match PrivateKey::from_openssh(&cleaned) {
        Ok(key) => {
            println!("\n✅ 私钥解析成功！");
            println!("算法: {:?}", key.algorithm());
            println!("公钥指纹: {}", key.public_key().fingerprint(ssh_key::HashAlg::Sha256));
        }
        Err(e) => {
            println!("\n❌ 私钥解析失败: {}", e);
            println!("\n可能的原因:");
            println!("1. 私钥文件格式不正确（不是OpenSSH格式）");
            println!("2. 私钥文件已损坏");
            println!("3. 私钥有密码保护（需要提供密码）");
        }
    }
}