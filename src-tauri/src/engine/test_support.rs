//! 测试用的共享夹具。

use std::path::{Path, PathBuf};

/// 仓库自带的示例稿库。
///
/// 三个端到端测试（导出 docx、人物索引、大纲摘要）依赖它，因为它同时包含
/// 目录层级、front matter 元数据、`[[双链]]` 与 `characters`/`pov`，
/// 是构造出来的 `DocInput` 覆盖不到的组合。
///
/// ## 为什么缺失时要区分环境
///
/// 这些测试原本写的是「目录不存在就 return」，本意是不让 CI 因数据文件缺失而红。
/// 但它的反面是：示例库被误删时，测试**静默通过**，覆盖率归零而 CI 依然全绿。
///
/// CI 检出的是完整仓库，示例库一定存在；缺失只可能是被误删，此时必须让 CI 红。
/// 本地裁剪仓库的开发者仍可跳过。
pub fn sample_vault() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/sample-vault");

    if root.is_dir() {
        return Some(root);
    }

    assert!(
        std::env::var_os("CI").is_none(),
        "examples/sample-vault 缺失：依赖它的端到端测试会被静默跳过，\
         覆盖率归零而 CI 依然全绿。若不是有意删除，请从 git 恢复该目录。"
    );

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 示例库存在时可取到路径() {
        // 本地若裁剪掉了示例库，这条会跳过（与 CI 的行为区分开）
        let Some(root) = sample_vault() else {
            return;
        };

        assert!(root.join("_vault.yaml").is_file(), "缺少库配置");
        assert!(root.join("小说/清籁集/卷一·听风/001-楔子.md").is_file(), "缺少示例章节");
        assert!(root.join("设定/人物/沈砚.md").is_file(), "缺少人物设定");
    }
}
