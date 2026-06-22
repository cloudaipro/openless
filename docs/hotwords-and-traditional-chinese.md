# 熱詞 / 自訂字典 與 繁體中文（台灣）支援

本文說明 OpenLess 兩個容易混淆的功能差異，以及繁體中文（台灣標準字）的處理路徑。
靈感與設計參考自姊妹專案 SpeakSlow。

## 熱詞 vs 字典替換：輸入端偏置 vs 輸出端修正

兩者目的不同，請依需求選用：

- **熱詞（Hotwords，輸入端偏置）**：在 ASR *辨識階段* 提高特定詞彙被選中的機率。
  適合人名、品牌、產品、專業術語等同音異字容易選錯的詞（例：聲聲慢、ChatGPT、晶晶體）。
  本質是「提示模型往這些詞偏」，不保證 100% 命中。
- **字典替換 / 校正規則（Correction Rule，輸出端修正）**：在辨識 *之後* 對文字做
  確定性的字串替換。適合「不管怎樣都要把 A 改成 B」的硬規則（例：固定錯字、統一寫法）。

簡記：**熱詞是機率偏置（可能漏），校正規則是必定替換（一定改）。** 兩者可並用。

## 熱詞各 ASR 供應商覆蓋狀況

熱詞來源統一為使用者辞書（`enabled_hotwords` / `enabled_phrases`），各供應商注入方式不同：

| 供應商 | 注入方式 | 狀態 |
| --- | --- | --- |
| Whisper 相容（whisper / siliconflow / zhipu / groq / 本地 whisper server） | `prompt` 參數（`build_prompt_from_phrases`） | ✅ 已支援 |
| Volcengine（火山引擎） | 請求內 `hotwords` context | ✅ 已支援 |
| Xiaomi MiMo | OpenAI 相容 chat：音訊前加一段 text 詞彙提示（含「請勿輸出」指示） | ⚠️ 已接線，**MiMo 端偏置行為未經實機驗證** |
| Bailian（阿里百煉 / DashScope） | 伺服器端 `vocabulary_id`（需先註冊詞表取得 ID） | ❌ 非 inline；需另做詞表註冊流程 |
| 本地 sherpa-onnx（Windows、Zipformer transducer） | `hotwords_file` + `hotwords_score` | ⏳ 規劃中（Windows 限定，且需補 `bpe.vocab` 至下載清單） |
| 本地 Apple Speech / Foundry | 引擎不支援外部熱詞 | ❌ 不適用 |

備註：
- Bailian 的 `vocabulary_id` 是 DashScope 預先註冊的詞表機制，與 inline 片語清單模型不同，
  要完整支援需另外做「詞表上傳 / 管理」的 CRUD 流程，目前不在範圍內。
- MiMo 的 text 詞彙提示帶有「請勿輸出此行」指示以降低提示文混入轉寫的風險，但實際偏置效果
  需用真實 API 驗證後才建議預設開啟。

## 繁體中文（台灣標準字）輸出

重點：**不要試圖強迫 Whisper 直接吐繁體。** Whisper 的中文語言代碼只有 `zh`，模型層級
沒有簡 / 繁之分，訓練資料以簡體為主，`initial_prompt` 只能「偏置」無法「保證」。
連 SpeakSlow 自己的本地 whisper server 也不做強制——它靠下游 OpenCC 後轉換。

OpenLess 採同樣的穩健路徑：

```
音訊 → Whisper/其他 ASR（zh，多為簡體） → OpenCC s2tw + 台標逐字修正 → 繁體（台灣）輸出
```

實作位置：`coordinator/asr_wiring.rs::apply_chinese_script_preference`

- 繁體偏好走 OpenCC `S2tw`（台灣標準字：吃≠喫、裡≠裏），簡體偏好走 `Tw2s`。
- `s2tw` 仍有少數漏網字，再以 `fix_taiwan_chars` 逐字修正（例：账→賬→**帳**）。
- 轉換器以 `OnceLock` 快取（字典載入成本高），一次性與串流路徑共用。

### 一次性 vs 串流路徑

- **一次性（非串流）路徑**：`finalize_polished_text` 會做簡繁轉換 + 中英混用（晶晶體）
  整行正規化（`cjk_postprocess::apply_code_switch`）+ 校正規則。
- **串流插入路徑**：對每個 flush 緩衝做 *字級* `s2tw` 轉換後才落字（字級映射在 chunk
  邊界上基本安全且冪等）。中英混用整行正規化與校正規則需完整上下文，串流路徑不做；
  需要完整後處理時可關閉 streaming，走一次性路徑。

## 中英混用（晶晶體）

`coordinator/cjk_postprocess.rs`（移植自 SpeakSlow `text_processing.py`）：

- 全形英數 → 半形（不動中文與全形標點）。
- 合併被空白拆開的單一英文字母（`h e n t` → `hent`），保留正常英文詞間空白。
- 英文「為主」的整行去中文腔：全形標點 → 半形、句首大寫、獨立 `i` → `I`；真正的
  中英混雜句（中文為主）保留中文全形標點、英文原文不翻。

## 跨平台

簡繁轉換（`ferrous-opencc`）、中英混用後處理（`regex`）皆在主 `[dependencies]`，
**Windows / macOS / Ubuntu Linux 皆可用**。僅本地 sherpa-onnx 熱詞為 Windows 限定。
