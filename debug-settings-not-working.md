# Debug Session: settings-not-working

**Session ID:** `settings-not-working`
**Status:** [OPEN]
**Created:** 2026-05-28

## Problem Description

**Symptoms:**
1. 设置页面的所有功能（主题切换、壁纸选择）无法生效
2. 窗口无法移动（标题栏拖拽不工作）

**Expected:**
1. 主题切换应该改变整体颜色
2. 壁纸选择应该改变桌面背景
3. 窗口标题栏应该可以拖拽移动窗口

## Hypotheses

### H1: CSS 变量未正确应用到 DOM
- **Observation Point:** `document.documentElement.getAttribute("data-theme")`
- **Expected:** 切换主题后应该看到 `data-theme="dark"` 或 `data-theme="light"`

### H2: 壁纸样式被其他 CSS 覆盖
- **Observation Point:** `.desktop-area` 的实际 `background` 值
- **Expected:** 应该是 React 设置的壁纸值

### H3: Tauri 窗口配置缺少 data-tauri-drag-region
- **Observation Point:** 标题栏元素的 `data-tauri-drag-region` 属性
- **Expected:** 应该存在该属性

### H4: 状态更新后组件未重新渲染
- **Observation Point:** `useTheme` 和 `useWallpaper` hook 的状态值
- **Expected:** 状态变化后组件应该重新渲染

### H5: localStorage 写入失败
- **Observation Point:** localStorage 中的 `quirel-theme` 和 `quirel-wallpaper`
- **Expected:** 切换后应该能看到存储的值

## Instrumentation Plan

1. 在 `useTheme` hook 中添加日志，监控主题状态变化
2. 在 `useWallpaper` hook 中添加日志，监控壁纸状态变化
3. 在 Desktop 组件中添加日志，检查壁纸样式是否正确传递
4. 检查窗口标题栏的拖拽属性

## Evidence Log

(待收集)

## Analysis

(待分析)

## Fix

(待修复)

## Verification

(待验证)