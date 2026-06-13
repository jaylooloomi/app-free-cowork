/// ollama.com/api/tags 的名稱不含 cloud 後綴;經本機 daemon 使用需補:
/// 無 tag(無':')→ `name:cloud`;有 tag → `name:tag-cloud`(實證:gpt-oss:120b-cloud)。
pub fn to_local_name(catalog_name: &str) -> String {
    if catalog_name.contains(':') { format!("{catalog_name}-cloud") } else { format!("{catalog_name}:cloud") }
}

pub fn parse_cloud_models(api_tags_json: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(api_tags_json).ok()?;
    Some(v.get("models")?.as_array()?
        .iter()
        .filter_map(|m| m.get("name")?.as_str().map(to_local_name))
        .collect())
}

/// Fallback chain when the configured model leaves the catalog. Free-tier
/// access verified empirically (2026-06-12/13). Default is minimax-m2.5 — light
/// on the GPU-time free quota; qwen3-coder-next is the agentic fallback. For
/// images switch to a vision model (qwen3-vl) via the picker. minimax-m2.7 /
/// qwen3.5 are subscription-gated (HTTP 403) — never default to them.
pub const FALLBACKS: [&str; 2] = ["minimax-m2.5:cloud", "qwen3-coder-next:cloud"];
pub const CATALOG_URL: &str = "https://ollama.com/api/tags";

/// Models empirically verified to respond on the free plan (full catalog scan
/// 2026-06-13, all 42 cloud models classified). Used by the picker to label
/// tiers; anything not listed here and not learned at runtime shows "unknown".
pub const VERIFIED_FREE: [&str; 27] = [
    "qwen3-vl:235b-cloud", "qwen3-vl:235b-instruct-cloud", "qwen3-coder-next:cloud",
    "qwen3-next:80b-cloud", "qwen3-coder:480b-cloud", "minimax-m2.5:cloud", "glm-4.7:cloud",
    "gpt-oss:120b-cloud", "gpt-oss:20b-cloud", "gemma3:4b-cloud", "gemma3:12b-cloud",
    "gemma3:27b-cloud", "gemma4:31b-cloud", "ministral-3:3b-cloud", "ministral-3:8b-cloud",
    "ministral-3:14b-cloud", "devstral-2:123b-cloud", "devstral-small-2:24b-cloud",
    "cogito-2.1:671b-cloud", "nemotron-3-nano:30b-cloud", "rnj-1:8b-cloud",
    // 2026-06-13 補掃
    "glm-4.6:cloud", "minimax-m2:cloud", "minimax-m2.1:cloud", "minimax-m3:cloud",
    "nemotron-3-super:cloud", "nemotron-3-ultra:cloud",
];

/// Models empirically verified to be subscription-gated (HTTP 403 "requires a
/// subscription"; full catalog scan 2026-06-13). The picker labels these
/// "需訂閱" and the default filter hides them.
pub const VERIFIED_SUBSCRIPTION: [&str; 15] = [
    "minimax-m2.7:cloud", "qwen3.5:397b-cloud", "deepseek-v3.1:671b-cloud",
    "deepseek-v3.2:cloud", "glm-5:cloud", "kimi-k2:1t-cloud", "mistral-large-3:675b-cloud",
    // 2026-06-13 補掃
    "deepseek-v4-flash:cloud", "deepseek-v4-pro:cloud", "gemini-3-flash-preview:cloud",
    "glm-5.1:cloud", "kimi-k2-thinking:cloud", "kimi-k2.5:cloud", "kimi-k2.6:cloud",
    "kimi-k2.7-code:cloud",
];

/// 特殊哨符:用使用者自己的 Anthropic 帳號直接跑真正的 Claude(不經 Ollama)。
/// 不是 Ollama 雲端目錄裡的模型,所以 choose_model / tier 都要特別處理。
pub const CLAUDE_MODEL: &str = "claude";

/// 回傳 (要用的模型, 若有改動的中文通知)。catalog 為空(離線/未取得)時不改動。
pub fn choose_model(configured: &str, catalog: &[String]) -> (String, Option<String>) {
    // claude 走 Anthropic 帳號、不在 Ollama 目錄裡 — 永遠原樣保留,不做 fallback。
    if configured == CLAUDE_MODEL {
        return (configured.to_string(), None);
    }
    if catalog.is_empty() || catalog.iter().any(|c| c == configured) {
        return (configured.to_string(), None);
    }
    let pick = FALLBACKS.iter()
        .find(|f| catalog.iter().any(|c| c == *f))
        .map(|f| f.to_string())
        .unwrap_or_else(|| catalog[0].clone());
    let notice = format!("模型 {configured} 已不在雲端目錄,改用 {pick}");
    (pick, Some(notice))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TAGS: &str = r#"{"models":[
        {"name":"minimax-m2.7","model":"minimax-m2.7","modified_at":"2026-03-01T00:00:00Z","size":0,"details":{"family":"minimax"}},
        {"name":"gpt-oss:120b"},
        {"name":"qwen3-coder-next"}
    ]}"#;
    #[test]
    fn parses_api_tags_to_local_cloud_names() {
        let names = parse_cloud_models(TAGS).unwrap();
        assert_eq!(names, vec!["minimax-m2.7:cloud", "gpt-oss:120b-cloud", "qwen3-coder-next:cloud"]);
        assert!(parse_cloud_models("not json").is_none());
    }
    #[test]
    fn choose_model_prefers_configured_then_fallbacks_then_first() {
        let cat: Vec<String> = vec!["minimax-m2.5:cloud".into(), "qwen3-coder-next:cloud".into(), "glm-5:cloud".into()];
        let (m, notice) = choose_model("minimax-m2.5:cloud", &cat);
        assert_eq!(m, "minimax-m2.5:cloud"); assert!(notice.is_none());
        // FALLBACKS[0] (minimax-m2.5) is in this catalog → picked
        let (m, notice) = choose_model("dead-model:cloud", &cat);
        assert_eq!(m, "minimax-m2.5:cloud"); assert!(notice.is_some());
        let cat2: Vec<String> = vec!["glm-5:cloud".into()];
        let (m, notice) = choose_model("dead-model:cloud", &cat2);
        assert_eq!(m, "glm-5:cloud"); assert!(notice.is_some());
        let (m, notice) = choose_model("anything:cloud", &[]);
        assert_eq!(m, "anything:cloud"); assert!(notice.is_none()); // 離線:不亂改,交給 runtime
    }
}
