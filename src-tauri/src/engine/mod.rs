pub mod atomic;
pub mod characters;
pub mod export;
pub mod links;
pub mod md;
pub mod search;
pub mod stats;
pub mod vault;
pub mod versions;

/// 测试夹具。`#[cfg(test)]` 只在整个 crate 以测试模式编译时可见。
#[cfg(test)]
pub mod test_support;
