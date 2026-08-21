# PowerShell 脚本：启动 Quirel 开发环境
Write-Host "🚀 启动 Quirel 开发环境..." -ForegroundColor Green

# 检查 Rust 是否已安装
if (!(Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Host "❌ 未找到 Cargo，请先安装 Rust 工具链" -ForegroundColor Red
    exit 1
}

# 检查 Node.js 是否已安装
if (!(Get-Command "npm" -ErrorAction SilentlyContinue)) {
    Write-Host "❌ 未找到 NPM，请先安装 Node.js" -ForegroundColor Red
    exit 1
}

Write-Host "📋 步骤 1: 构建 Quireld..." -ForegroundColor Yellow
Set-Location -Path ".\agent"
Start-Process -FilePath "cargo" -ArgumentList "watch", "-x", "run" -NoNewWindow
Start-Sleep -Seconds 3

Write-Host "📋 步骤 2: 启动 Tauri 前端..." -ForegroundColor Yellow
Set-Location -Path ".."
Start-Process -FilePath "npm" -ArgumentList "run", "tauri", "dev" -NoNewWindow

Write-Host "✅ 开发环境已启动！" -ForegroundColor Green
Write-Host "💡 Quireld 将在 http://localhost:8443 运行" -ForegroundColor Cyan
Write-Host "💡 Tauri 应用将在默认浏览器中打开" -ForegroundColor Cyan