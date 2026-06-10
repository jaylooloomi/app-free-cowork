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

pub const FALLBACKS: [&str; 2] = ["minimax-m2.7:cloud", "qwen3-coder-next:cloud"];
pub const CATALOG_URL: &str = "https://ollama.com/api/tags";

/// 回傳 (要用的模型, 若有改動的中文通知)。catalog 為空(離線/未取得)時不改動。
pub fn choose_model(configured: &str, catalog: &[String]) -> (String, Option<String>) {
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
        let cat: Vec<String> = vec!["minimax-m2.7:cloud".into(), "qwen3-coder-next:cloud".into(), "glm-5:cloud".into()];
        let (m, notice) = choose_model("minimax-m2.7:cloud", &cat);
        assert_eq!(m, "minimax-m2.7:cloud"); assert!(notice.is_none());
        let (m, notice) = choose_model("dead-model:cloud", &cat);
        assert_eq!(m, "minimax-m2.7:cloud"); assert!(notice.is_some());
        let cat2: Vec<String> = vec!["glm-5:cloud".into()];
        let (m, notice) = choose_model("dead-model:cloud", &cat2);
        assert_eq!(m, "glm-5:cloud"); assert!(notice.is_some());
        let (m, notice) = choose_model("anything:cloud", &[]);
        assert_eq!(m, "anything:cloud"); assert!(notice.is_none()); // 離線:不亂改,交給 runtime
    }
}
