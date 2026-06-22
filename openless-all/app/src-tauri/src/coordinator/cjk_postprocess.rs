//! 中英混用（晶晶體）後處理：英文保留原文、不亂翻、不套中文腔。
//!
//! 從 SpeakSlow `text_processing.py` 移植：
//! - `normalize_ascii_width`：全形英數 → 半形（不動中文與全形標點）。
//! - `merge_spaced_letters`：把被空白拆開的單一字母（`h e n t` → `hent`）合併，
//!   但保留正常英文詞間空白（`hello world` 不動）。
//! - `localize_english_punct`：英文為主的「整行」去中文腔（全形標點 → 半形 +
//!   句首大寫 + 獨立 i → I）；真正的中英混雜行保留中文標點。
//!
//! 整段 `apply_code_switch` 為終稿正規化，跨平台（含 Ubuntu Linux）。
//! 注意：`localize_english_punct` 是「整行」判斷，需要完整一行上下文，
//! 故只用在一次性（非串流）路徑；串流路徑只做字級簡繁轉換。

use std::sync::OnceLock;

use regex::Regex;

/// 講英文時的 uh/um/ah 常被雙語模型聽成這些中文語氣字。
const EN_FILLER_CJK: &[char] = &['啊', '嗯', '呃', '哦', '喔', '欸', '唉', '誒', '呀', '嘛'];

fn is_cjk(ch: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&ch)
}

/// 全形英文字母 / 數字 → 半形（中文與全形標點，。？！不動）。
fn normalize_ascii_width(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            let is_fw_digit = (0xFF10..=0xFF19).contains(&code);
            let is_fw_upper = (0xFF21..=0xFF3A).contains(&code);
            let is_fw_lower = (0xFF41..=0xFF5A).contains(&code);
            if is_fw_digit || is_fw_upper || is_fw_lower {
                char::from_u32(code - 0xFEE0).unwrap_or(ch)
            } else {
                ch
            }
        })
        .collect()
}

/// 合併被空白拆開的單一英文字母序列（`h e n t` → `hent`）。
///
/// 對應 Python regex `(?<![A-Za-z])([A-Za-z](?: [A-Za-z])+)(?![A-Za-z])`。
/// Rust `regex` 不支援 lookaround，故手寫掃描：
/// - 序列起點前一字不可為英文字母；
/// - 由「字母 + 空白 + 字母」連續組成，至少 2 個字母；
/// - 序列最後一個字母後不可緊跟字母（否則視為一般單字，不合併）。
fn merge_spaced_letters(text: &str) -> String {
    if !text.contains(' ') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let is_a = |c: char| c.is_ascii_alphabetic();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let prev_ok = i == 0 || !is_a(chars[i - 1]);
        if is_a(c) && prev_ok {
            let mut letters = vec![c];
            let mut k = i;
            while k + 2 < n && chars[k + 1] == ' ' && is_a(chars[k + 2]) {
                letters.push(chars[k + 2]);
                k += 2;
            }
            let followed_by_letter = k + 1 < n && is_a(chars[k + 1]);
            if letters.len() >= 2 && !followed_by_letter {
                out.extend(letters.iter());
                i = k + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 判斷一行是否「英文為主」：沒有 CJK，或僅夾雜少量語氣 filler（講英文時的
/// uh/ah 被聽成 啊/嗯）。真正的中英混雜句回傳 false（保留中文標點）。
fn is_english_dominant(line: &str) -> bool {
    let cjk_count = line.chars().filter(|&c| is_cjk(c)).count();
    let ascii_letters = line.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if cjk_count == 0 {
        return ascii_letters > 0;
    }
    ascii_letters >= 12
        && cjk_count <= 1 + ascii_letters / 15
        && line.chars().filter(|&c| is_cjk(c)).all(|c| EN_FILLER_CJK.contains(&c))
}

fn space_before_punct_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+([,.?!:;])").unwrap())
}

fn multi_space_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s{2,}").unwrap())
}

fn sentence_start_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // 句首（行首或 .?! 後接空白）的小寫字母 → 大寫。
    RE.get_or_init(|| Regex::new(r"(^|[.?!]\s+)([a-z])").unwrap())
}

fn lone_i_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bi\b").unwrap())
}

fn filler_run_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    let class: String = EN_FILLER_CJK.iter().collect();
    RE.get_or_init(move || Regex::new(&format!("[{class}]+")).unwrap())
}

/// 英文為主的行去中文腔：全形標點 → 半形 + 標點後空格 + 句首大寫 + 獨立 i → I；
/// 夾雜的中文語氣 filler 清除。真正的中英混雜行原樣保留。
fn localize_english_punct(text: &str) -> String {
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            if line.is_empty() || !is_english_dominant(line) {
                return line.to_string();
            }
            // filler → 空格
            let mut l = filler_run_re().replace_all(line, " ").into_owned();
            // 全形標點 → 半形 + 後綴空格
            for (from, to) in [
                ('，', ", "),
                ('。', ". "),
                ('？', "? "),
                ('！', "! "),
                ('：', ": "),
                ('；', "; "),
                ('、', ", "),
            ] {
                if l.contains(from) {
                    l = l.replace(from, to);
                }
            }
            l = space_before_punct_re().replace_all(&l, "$1").into_owned();
            l = multi_space_re().replace_all(&l, " ").trim().to_string();
            l = sentence_start_re()
                .replace_all(&l, |caps: &regex::Captures| {
                    format!("{}{}", &caps[1], caps[2].to_uppercase())
                })
                .into_owned();
            l = lone_i_re().replace_all(&l, "I").into_owned();
            l
        })
        .collect();
    lines.join("\n")
}

/// 終稿中英混用正規化：全形→半形 → 合併拆字 → 英文行去中文腔。
pub(super) fn apply_code_switch(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let t = normalize_ascii_width(text);
    let t = merge_spaced_letters(&t);
    localize_english_punct(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullwidth_ascii_to_halfwidth() {
        assert_eq!(normalize_ascii_width("ＡＢＣ１２３"), "ABC123");
        // 中文與全形標點不動
        assert_eq!(normalize_ascii_width("你好，世界"), "你好，世界");
    }

    #[test]
    fn merge_split_single_letters() {
        assert_eq!(merge_spaced_letters("h e n t"), "hent");
        // 正常英文詞不動
        assert_eq!(merge_spaced_letters("hello world"), "hello world");
        // 中文夾單字母
        assert_eq!(merge_spaced_letters("開 h"), "開 h"); // 單一字母、無第二個，不合併
        assert_eq!(merge_spaced_letters("用 a i 模型"), "用 ai 模型");
    }

    #[test]
    fn english_line_localized() {
        let out = apply_code_switch("hello，how are you？");
        assert_eq!(out, "Hello, how are you?");
    }

    #[test]
    fn mixed_line_keeps_chinese_punct() {
        // 中英混雜（中文為主）→ 保留全形標點，英文原文不動
        let input = "我覺得這個 feature 很棒，要 ship 嗎？";
        assert_eq!(apply_code_switch(input), input);
    }

    #[test]
    fn lone_i_capitalized() {
        assert_eq!(apply_code_switch("i think i can do it"), "I think I can do it");
    }

    #[test]
    fn empty_passthrough() {
        assert_eq!(apply_code_switch(""), "");
    }
}
