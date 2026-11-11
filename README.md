# Qinglai Scriptorium · 清籁斋

面向长篇小说与笔记的个人写作工作台。Tauri 2 + Rust 后端，React 19 + Milkdown 前端。

## 设计前提

**文件即真相，索引可重建。** 稿件以 Markdown + YAML front matter 存于本地磁盘，是本应用唯一的真相源；
任何数据库、索引、缓存都视为可丢弃的派生结构。这保证三件事：

1. 稿件可以用 git 做版本管理，也可以被 Obsidian / VS Code 直接打开；
2. 索引损坏不会造成数据损失，只会损失速度；
3. 换工具时不需要迁移，稿子本来就在磁盘上。

## 目录约定

```
vault/
  _vault.yaml                 # 库配置、统计口径
  小说/清籁集/
    _book.yaml                # 书名、卷结构、目标字数
    卷一·听风/
      001-楔子.md
  设定/人物/沈砚.md
  笔记/
```

按章分文件是刻意设计：单章数千字时编辑器与统计性能都不敏感，
而单文件百万字会同时撞上渲染瓶颈与版本冲突。

## 快捷键

| 键 | 作用 |
|---|---|
| `Ctrl/Cmd + K` | 命令面板：搜命令，或按章名快速跳转 |
| `F11` | 切换专注模式（隐藏目录树） |
| `Esc` | 关闭命令面板 / 清空当前检索 |

命令面板里可以直接输入章节名跳转，也可以找「导出 docx」「切换主题」「写作统计」
这类不常驻在界面上的功能；多个词用空格分隔，按「且」匹配。

## 开发

```bash
pnpm install
pnpm tauri dev        # 同时启动 Vite 与桌面窗口
```

> **debug 构建必须配 dev server。** Tauri 的 debug profile 加载 `devUrl`
> （`http://localhost:1420`），只有 release 才用 `frontendDist`。
> 直接 `cargo build` 后运行 `target/debug/*.exe`，WebView 会去连一个没启动的
> Vite，得到「无法访问此页面」的错误页——**而进程照样存活**，只看进程状态
> 会误判为启动成功。要么用 `pnpm tauri dev`，要么用 `pnpm tauri build` 出 release。

校验：

```bash
pnpm exec tsc --noEmit                              # 类型
pnpm test                                           # 前端单测（vitest）
cargo test --manifest-path src-tauri/Cargo.toml     # 后端单测
pnpm build                                          # 前端构建
```

加完测试文件后**必须重跑 `tsc`**：vitest 不做类型检查，只跑 vitest 会漏掉
`noUncheckedIndexedAccess` 之类的类型错误（CI 会红）。

### 界面冒烟测试

单元测试覆盖不到渲染层——ProseMirror 需要真实 DOM，jsdom 跑不起来。
要确认界面真的渲染出来，走 WebView2 的远程调试端口读 DOM：

```bash
# 终端 1
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" pnpm tauri dev

# 终端 2：检查空状态界面
pnpm check:ui

# 带稿库跑一遍真实功能（章节树、编辑器、双链、人物）
pnpm check:ui --vault "$(pwd)/examples/sample-vault"
```

脚本会断言：不是连接错误页、React 已挂载、编辑器有内容、无 console 报错。
退出码非 0 即失败。

## 文档

- [架构方案](docs/architecture.md)

## 许可

个人项目，未授权分发。

© luoqingci
