//! 字数统计 command。纯函数，无状态，便于前端在输入过程中即时调用。

use crate::engine::md;

#[tauri::command]
pub fn count_words(text: String, count_punctuation: Option<bool>) -> md::WordCount {
    md::count_words(&text, count_punctuation.unwrap_or(false))
}
