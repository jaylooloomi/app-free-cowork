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
  state: "ready" | "needs_setup" | "degraded";
  model: string;
  detail: string;
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
  /** 回傳 "launched" | "wizard";失敗時 reject(中文訊息)。 */
  submitPrompt: (prompt: string) => invoke<string>("submit_prompt", { prompt }),
  wizardPlan: () => invoke<WizardPlan>("wizard_plan"),
  wizardRun: (step: string) => invoke<StepResult>("wizard_run", { step }),
  wizardDone: () => invoke<void>("wizard_done"),
  listCloudModels: () => invoke<string[]>("list_cloud_models"),
  openLogs: () => invoke<void>("open_logs"),
  hidePalette: () => invoke<void>("hide_palette"),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
};
