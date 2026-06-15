//! 任務完成語音播報的純邏輯:從 stream-json log 萃取一句摘要、計算 overlay
//! 在螢幕下方中央的位置。兩者皆為純函式以便單元測試(整合/視窗/TTS 為手動驗證)。

/// 把一段文字截成最多 `max` 句的口語短句:折疊空白/換行,遇到句末標點計數,
/// 達到 `max` 句或超過硬上限即停。避免把整段(含程式碼)都唸出來。
fn first_sentences(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    let mut sentences = 0;
    for ch in collapsed.chars() {
        out.push(ch);
        if matches!(ch, '。' | '!' | '?' | '.' | '…' | '；') {
            sentences += 1;
            if sentences >= max {
                break;
            }
        }
        if out.chars().count() >= 120 {
            break;
        }
    }
    out.trim().to_string()
}

/// 從 Claude Code 的 stream-json log 萃取一句任務摘要。
/// 優先用最後一個 `result` 事件(Claude Code 自帶的結尾結論),
/// 否則退回最後一則 assistant 文字。都沒有 → None(呼叫端用 fallback 句)。
pub fn extract_summary(log: &str) -> Option<String> {
    let mut last_result: Option<String> = None;
    let mut last_assistant: Option<String> = None;

    for line in log.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // 非 JSON 行直接略過
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("result") => {
                if let Some(t) = v.get("result").and_then(|r| r.as_str()) {
                    if !t.trim().is_empty() {
                        last_result = Some(t.to_string());
                    }
                }
            }
            Some("assistant") => {
                if let Some(content) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    let text: String = content
                        .iter()
                        .filter_map(|b| {
                            if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                b.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if !text.trim().is_empty() {
                        last_assistant = Some(text);
                    }
                }
            }
            _ => {}
        }
    }

    let raw = last_result.or(last_assistant)?;
    let summary = first_sentences(&raw, 2);
    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

/// 讓視窗在指定螢幕「水平置中、垂直貼近底部(留 `margin_px` 間距)」的左上角座標。
/// 與 palette 的置中共用 X 邏輯;Y 改為貼底。`saturating_sub` 防止視窗比螢幕大時 underflow。
pub fn bottom_centered_position(
    monitor_pos: tauri::PhysicalPosition<i32>,
    monitor_size: tauri::PhysicalSize<u32>,
    window_size: tauri::PhysicalSize<u32>,
    margin_px: u32,
) -> tauri::PhysicalPosition<i32> {
    let x = monitor_pos.x + ((monitor_size.width.saturating_sub(window_size.width)) / 2) as i32;
    let y = monitor_pos.y
        + (monitor_size
            .height
            .saturating_sub(window_size.height)
            .saturating_sub(margin_px)) as i32;
    tauri::PhysicalPosition { x, y }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::{PhysicalPosition, PhysicalSize};

    #[test]
    fn extracts_last_result_event() {
        let log = r#"{"type":"system","subtype":"init"}
{"type":"assistant","message":{"content":[{"type":"text","text":"在處理中…"}]}}
{"type":"result","subtype":"success","result":"我已經把面板改成開在焦點螢幕。共改兩個檔案。","is_error":false}"#;
        assert_eq!(
            extract_summary(log).unwrap(),
            "我已經把面板改成開在焦點螢幕。共改兩個檔案。"
        );
    }

    #[test]
    fn falls_back_to_last_assistant_text_when_no_result() {
        let log = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"第一段。"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"x"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"最後這段才是答案。"}]}}"#;
        assert_eq!(extract_summary(log).unwrap(), "最後這段才是答案。");
    }

    #[test]
    fn truncates_to_two_sentences() {
        let log = r#"{"type":"result","result":"第一句。第二句。第三句不該出現。"}"#;
        assert_eq!(extract_summary(log).unwrap(), "第一句。第二句。");
    }

    #[test]
    fn returns_none_when_nothing_usable() {
        assert_eq!(extract_summary(""), None);
        assert_eq!(extract_summary("not json at all\n{garbage"), None);
        assert_eq!(
            extract_summary(r#"{"type":"system","subtype":"init"}"#),
            None
        );
    }

    #[test]
    fn ignores_non_json_lines_between_events() {
        let log = "garbage line\n{\"type\":\"result\",\"result\":\"完成了。\"}\nmore garbage";
        assert_eq!(extract_summary(log).unwrap(), "完成了。");
    }

    #[test]
    fn bottom_center_primary_monitor() {
        // 1920x1080,視窗 560x200,margin 48 → x 置中、y 貼底
        let p = bottom_centered_position(
            PhysicalPosition { x: 0, y: 0 },
            PhysicalSize { width: 1920, height: 1080 },
            PhysicalSize { width: 560, height: 200 },
            48,
        );
        assert_eq!(p.x, (1920 - 560) / 2); // 680
        assert_eq!(p.y, 1080 - 200 - 48); // 832
    }

    #[test]
    fn bottom_center_offsets_secondary_monitor() {
        let p = bottom_centered_position(
            PhysicalPosition { x: 1920, y: 0 },
            PhysicalSize { width: 2560, height: 1440 },
            PhysicalSize { width: 560, height: 200 },
            48,
        );
        assert_eq!(p.x, 1920 + (2560 - 560) / 2); // 2920
        assert_eq!(p.y, 1440 - 200 - 48); // 1192
    }

    #[test]
    fn bottom_center_clamps_when_window_too_tall() {
        let p = bottom_centered_position(
            PhysicalPosition { x: 100, y: 50 },
            PhysicalSize { width: 320, height: 240 },
            PhysicalSize { width: 560, height: 400 },
            48,
        );
        assert_eq!(p.x, 100); // width saturating_sub → 0
        assert_eq!(p.y, 50); // height saturating → 0 offset
    }
}
