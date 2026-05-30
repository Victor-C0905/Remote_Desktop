{
    "version": "2.0.0",
    "tasks": [
        {
            "label": "run-agent",
            "type": "shell",
            "command": "powershell",
            "args": [
                "-Command",
                "Get-Process -Name 'agent' -ErrorAction SilentlyContinue | Stop-Process -Force; cd '${workspaceFolder}/agent'; cargo run --release"
            ],
            "options": {
                "cwd": "${workspaceFolder}/agent"
            },
            "group": {
                "kind": "build",
                "isDefault": false
            },
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": false,
                "panel": "dedicated",
                "showReuseMessage": false,
                "clear": false
            },
            "isBackground": true,
            "problemMatcher": {
                "owner": "rust",
                "pattern": {
                    "regexp": "^(warning|error)(?:\$$(.*?)\$$)?:\\s+(.*)$",
                    "severity": 1,
                    "code": 2,
                    "message": 3
                },
                "background": {
                    "activeOnStart": true,
                    "beginsPattern": "^\\s*Compiling",
                    "endsPattern": "^\\s*(Finished|Running|INFO)"
                }
            },
            "detail": "运行 Agent 服务端 (单实例，专用终端)"
        },
        {
            "label": "stop-agent",
            "type": "shell",
            "command": "powershell",
            "args": [
                "-Command",
                "Get-Process -Name 'agent' -ErrorAction SilentlyContinue | Stop-Process -Force; Write-Host 'Agent 进程已停止'"
            ],
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": false,
                "panel": "dedicated",
                "showReuseMessage": false
            },
            "detail": "停止 Agent 进程"
        },
        {
            "label": "run-tauri-dev",
            "type": "shell",
            "command": "powershell",
            "args": [
                "-Command",
                "Get-Process -Name 'gnome-remote' -ErrorAction SilentlyContinue | Stop-Process -Force; cd '${workspaceFolder}'; npm run tauri dev"
            ],
            "options": {
                "cwd": "${workspaceFolder}"
            },
            "group": {
                "kind": "build",
                "isDefault": true
            },
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": false,
                "panel": "dedicated",
                "showReuseMessage": false,
                "clear": false
            },
            "isBackground": true,
            "problemMatcher": {
                "owner": "rust",
                "pattern": {
                    "regexp": "^(warning|error)(?:\$$(.*?)\$$)?:\\s+(.*)$",
                    "severity": 1,
                    "code": 2,
                    "message": 3
                },
                "background": {
                    "activeOnStart": true,
                    "beginsPattern": "^\\s*Compiling",
                    "endsPattern": "^\\s*(Finished|Running|Local:)"
                }
            },
            "detail": "运行 Tauri 客户端开发模式 (单实例，专用终端)"
        },
        {
            "label": "stop-tauri-dev",
            "type": "shell",
            "command": "powershell",
            "args": [
                "-Command",
                "Get-Process -Name 'gnome-remote' -ErrorAction SilentlyContinue | Stop-Process -Force; Write-Host 'Tauri 开发进程已停止'"
            ],
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": false,
                "panel": "dedicated",
                "showReuseMessage": false
            },
            "detail": "停止 Tauri 开发进程"
        },
        {
            "label": "run-all",
            "dependsOn": ["run-agent", "run-tauri-dev"],
            "dependsOrder": "sequence",
            "group": "build",
            "detail": "同时启动 Agent 和 Tauri 客户端"
        },
        {
            "label": "stop-all",
            "dependsOn": ["stop-agent", "stop-tauri-dev"],
            "dependsOrder": "parallel",
            "detail": "停止所有运行中的进程"
        },
        {
            "label": "build-agent-release",
            "type": "shell",
            "command": "cargo",
            "args": ["build", "--release"],
            "options": { "cwd": "${workspaceFolder}/agent" },
            "group": "build",
            "presentation": { "panel": "shared" },
            "problemMatcher": "$rustc",
            "detail": "编译 Agent (Release)"
        },
        {
            "label": "build-tauri-release",
            "type": "shell",
            "command": "npm",
            "args": ["run", "tauri", "build"],
            "options": { "cwd": "${workspaceFolder}" },
            "group": "build",
            "presentation": { "panel": "shared" },
            "problemMatcher": "$rustc",
            "detail": "编译 Tauri 客户端 (Release)"
        },
        {
            "label": "clean-build",
            "type": "shell",
            "command": "cargo",
            "args": ["clean"],
            "options": { "cwd": "${workspaceFolder}/agent" },
            "group": "build",
            "detail": "清理 Agent 构建缓存"
        },
        {
            "label": "rebuild-agent",
            "dependsOn": ["clean-build", "build-agent-release"],
            "dependsOrder": "sequence",
            "group": "build",
            "detail": "清理并重新编译 Agent"
        }
    ]
}