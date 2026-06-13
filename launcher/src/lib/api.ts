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

export interface QueueDto {
  running: RunningTask | null;
  queued: QueuedTask[];
}

export interface ModelEntry {
  name: string;
  tier: "free" | "subscription" | "unknown" | "anthropic";
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
  /** 回傳 "launched" | "queued" | "wizard";失敗時 reject(中文訊息)。"queued" 不會隱藏面板。 */
  submitPrompt: (prompt: string) => invoke<string>("submit_prompt", { prompt }),
  queueList: () => invoke<QueueDto>("queue_list"),
  queueCancel: (id: number) => invoke<void>("queue_cancel", { id }),
  /** 僅背景任務可停止;前景/閒置時 reject(中文訊息)。 */
  taskStop: () => invoke<void>("task_stop"),
  listModelsUi: () => invoke<ModelEntry[]>("list_models_ui"),
  setModel: (name: string) => invoke<void>("set_model", { name }),
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
