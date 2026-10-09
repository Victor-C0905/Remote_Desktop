@echo off
echo 🚀 启动 Quirel 开发环境...
echo.

REM 检查 Rust 是否已安装
where cargo >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo ❌ 未找到 Cargo，请先安装 Rust 工具链
    pause
    exit /b 1
)

REM 检查 Node.js 是否已安装
where npm >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo ❌ 未找到 NPM，请先安装 Node.js
    pause
    exit /b 1
)

echo 📋 步骤 1: 检查并安装依赖...
cd /d "%~dp0\.."

REM 安装前端依赖
echo Installing frontend dependencies...
npm install

echo.
echo 📋 步骤 2: 启动 Quireld (在新窗口中)...
start "Quireld" cmd /k "cd /d %~dp0\..\agent && cargo run"

echo.
echo 📋 步骤 3: 启动 Tauri 开发服务器 (在新窗口中)...
start "Quirel Tauri" cmd /k "cd /d %~dp0\.. && npm run tauri dev"

echo.
echo ✅ 开发环境已启动！
echo 💡 两个窗口将分别运行 Quireld 和 Tauri 应用
pause