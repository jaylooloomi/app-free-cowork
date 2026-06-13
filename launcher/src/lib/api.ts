import { invoke } from "@tauri-apps/api/core";

export interface Settings {
  hotkey: string;
  model: string;
  cautious_mode: boolean;
  background_mode: boolean;
  working_dir: string;
  autostart: boolean;
  history: string[];
  signin_state: "Unknown" | "Yes" | "No";
  known_subscription_models?: string[];
  known_free_models?: string[];
  known_broken_models?: string[];
  /** 介面語言 */
  locale: "zh-TW" | "en";
  /** 面板開著時啟動語音輸入的快捷鍵(預設 "Alt+J") */
  voice_hotkey: string;
  /** 面板開著時啟動框選截圖的快捷鍵(預設 "Alt+K") */
  capture_hotkey: string;
  /** 自訂助手個性系統提示;留空 = 內建預設 */
  system_prompt: string;
}

export interface StatusDto {
  state: "ready" | "needs_setup" | "degraded" | "offline";
  model: string;
  detail: string;
  /** 帳號方案("free"/"pro"/"max"…);null = 尚未取得 */
  plan: string | null;
}

export interface QueuedTask {
  id: number;
  prompt: string;
}

export interface RunningTask {
  id: number;
  prompt: string;
  background: boolean;
  pid: number | null;
}

/** 已完成、等待使用者打勾移除的任務 */
export interface CompletedTask {
  id: number;
  prompt: string;
  /** true = 成功(exit 0);false = 失敗 */
  ok: boolean;
}

export interface QueueDto {
  running: RunningTask | null;
  queued: QueuedTask[];
  completed: CompletedTask[];
}

export interface ModelEntry {
  name: string;
  tier: "free" | "subscription" | "unknown" | "anthropic" | "broken" | "incompatible";
}

export interface ScanSummary {
  free: number;
  subscription: number;
  broken: number;
  scanned: number;
  skipped: number;
}

export interface StepResult {
  ok: boolean;
  detail: string;
}

export interface WizardPlan {
  steps: string[];
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (newSettings: Settings) => invoke<void>("save_settings", { newSettings }),
  getStatus: () => invoke<StatusDto>("get_status"),
  getHistory: () => invoke<string[]>("get_history"),
  /** 回傳 "launched" | "queued" | "wizard";失敗時 reject(中文訊息)。"queued" 不會隱藏面板。
   *  workdir:本次任務的工作資料夾(由「選資料夾」鈕指定),null = 用設定預設。 */
  submitPrompt: (prompt: string, workdir?: string | null) =>
    invoke<string>("submit_prompt", { prompt, workdir: workdir ?? null }),
  /** 開原生資料夾選擇對話框,回傳路徑;取消回 null */
  pickFolder: () => invoke<string | null>("pick_folder"),
  /** 隱藏設定視窗(走後端,免前端 window.hide capability) */
  hideSettings: () => invoke<void>("hide_settings"),
  queueList: () => invoke<QueueDto>("queue_list"),
  queueCancel: (id: number) => invoke<void>("queue_cancel", { id }),
  /** 打勾移除一筆已完成項目 */
  dismissCompleted: (id: number) => invoke<void>("dismiss_completed", { id }),
  /** 框選截圖:觸發 Windows 框選,回傳暫存 PNG 路徑;取消/逾時回 null */
  captureScreenshot: () => invoke<string | null>("capture_screenshot"),
  /** 僅背景任務可停止;前景/閒置時 reject(中文訊息)。 */
  taskStop: () => invoke<void>("task_stop"),
  listModelsUi: () => invoke<ModelEntry[]>("list_models_ui"),
  setModel: (name: string) => invoke<void>("set_model", { name }),
  /** 主動掃描目錄中未知 tier 的模型,回傳統計。進度經由 "scan-progress" 事件、結束經由 "scan-done"。 */
  scanModels: () => invoke<ScanSummary>("scan_models"),
  /** 僅白名單網址(ollama.com/settings、ollama.com/upgrade)。 */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /** 後端會聚焦面板並送出 Win+H 啟動 Windows 語音輸入。 */
  startVoiceInput: () => invoke<void>("start_voice_input"),
  /** OS acrylic 效果是否真的套上(決定 fx-glass / fx-solid)。 */
  effectsApplied: () => invoke<boolean>("effects_applied"),
  /** 把貼上的圖片位元組存成暫存檔,回傳路徑。 */
  savePastedImage: (data: number[], ext: string) => invoke<string>("save_pasted_image", { data, ext }),
  wizardPlan: () => invoke<WizardPlan>("wizard_plan"),
  wizardRun: (step: string) => invoke<StepResult>("wizard_run", { step }),
  wizardDone: () => invoke<void>("wizard_done"),
  listCloudModels: () => invoke<string[]>("list_cloud_models"),
  openLogs: () => invoke<void>("open_logs"),
  hidePalette: () => invoke<void>("hide_palette"),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
};
