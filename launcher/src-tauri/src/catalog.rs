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
/// access verified empirically (2026-06-12/13). qwen3-vl is vision-capable so
/// pasted images work; qwen3-coder-next is the lighter agentic specialist.
/// minimax-m2.7 / qwen3.5 are subscription-gated (HTTP 403) — never default to them.
pub const FALLBACKS: [&str; 2] = ["qwen3-vl:235b-cloud", "qwen3-coder-next:cloud"];
pub const CATALOG_URL: &str = "https://ollama.com/api/tags";

/// Models empirically verified to respond on the free plan (2026-06-12/13).
/// Used by the model picker to label tiers; anything not listed here and not
/// learned as subscription-gated at runtime is shown as "unknown".
pub const VERIFIED_FREE: [&str; 5] = [
    "qwen3-vl:235b-cloud",
    "qwen3-coder-next:cloud",
    "qwen3-next:80b-cloud",
    "minimax-m2.5:cloud",
    "glm-4.7:cloud",
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
        // FALLBACKS[0] (qwen3-vl) not in this catalog → falls to FALLBACKS[1] qwen3-coder-next
        let (m, notice) = choose_model("dead-model:cloud", &cat);
        assert_eq!(m, "qwen3-coder-next:cloud"); assert!(notice.is_some());
        let cat2: Vec<String> = vec!["glm-5:cloud".into()];
        let (m, notice) = choose_model("dead-model:cloud", &cat2);
        assert_eq!(m, "glm-5:cloud"); assert!(notice.is_some());
        let (m, notice) = choose_model("anything:cloud", &[]);
        assert_eq!(m, "anything:cloud"); assert!(notice.is_none()); // 離線:不亂改,交給 runtime
    }
}
