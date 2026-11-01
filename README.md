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

## 开发

```bash
pnpm install
pnpm tauri dev
```

校验：

```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test
pnpm build
```

## 文档

- [架构方案](docs/architecture.md)

## 许可

个人项目，未授权分发。

© luoqingci
