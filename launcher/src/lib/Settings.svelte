<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, type Settings } from "./api";
  import { S } from "./strings";

  let s: Settings | null = $state(null);
  let models: string[] = $state([]);
  let saved = $state(false);
  let error = $state("");
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  async function load() {
    s = await api.getSettings();
    try {
      models = await api.listCloudModels();
    } catch {
      models = []; // 離線時仍可編輯其他設定
    }
    if (s && !models.includes(s.model)) models = [s.model, ...models];
  }

  onMount(() => {
    load();
    // 視窗只隱藏不關閉 → 每次重新顯示時重新載入,避免顯示過期資料
    const unlisten = listen("settings-shown", () => {
      load();
    });
    return () => {
      unlisten.then((f) => f());
      clearTimeout(savedTimer);
    };
  });

  async function save() {
    if (!s) return;
    error = "";
    saved = false;
    try {
      // 只覆寫本頁可編輯的欄位,保留其他流程改動的 history / signin_state
      const snap = $state.snapshot(s);
      const fresh = await api.getSettings();
      const merged = {
        ...fresh,
        hotkey: snap.hotkey,
        model: snap.model,
        cautious_mode: snap.cautious_mode,
        background_mode: snap.background_mode,
        working_dir: snap.working_dir,
        autostart: snap.autostart,
      };
      // 失敗(例:快捷鍵註冊失敗)→ 後端已回滾,顯示訊息,欄位保持可編輯重試
      await api.saveSettings(merged);
      saved = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (saved = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  async function openLogs() {
    try {
      await api.openLogs();
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if s}
  <main class="settings">
    <h1>{S.settingsTitle}</h1>
    <label
      >{S.settingsHotkey}
      <input bind:value={s.hotkey} placeholder={S.settingsHotkeyPlaceholder} />
      <small>{S.settingsHotkeyHint}</small></label
    >
    <label
      >{S.settingsModel}
      <select bind:value={s.model}>
        {#each models as m (m)}<option value={m}>{m}</option>{/each}
      </select></label
    >
    <label class="row"><input type="checkbox" bind:checked={s.cautious_mode} /> {S.settingsCautious}</label>
    <label class="row"><input type="checkbox" bind:checked={s.background_mode} /> {S.settingsBackground}</label>
    <label
      >{S.settingsWorkingDir}
      <input bind:value={s.working_dir} placeholder={S.settingsWorkingDirPlaceholder} /></label
    >
    <label class="row"><input type="checkbox" bind:checked={s.autostart} /> {S.settingsAutostart}</label>
    <div class="actions">
      <button class="primary" onclick={save}>{S.settingsSave}</button>
      <button onclick={openLogs}>{S.settingsOpenLogs}</button>
      {#if saved}<span class="ok">{S.settingsSaved}</span>{/if}
    </div>
    {#if error}<div class="err">{error}</div>{/if}
  </main>
{/if}

<style>
  .settings {
    padding: 24px;
    color: #eee;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  h1 {
    font-size: 20px;
    margin: 0;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 14px;
  }
  label.row {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  input:not([type="checkbox"]),
  select {
    padding: 8px;
    border-radius: 6px;
    border: 1px solid #444;
    background: #1e1e1e;
    color: #eee;
    outline: none;
  }
  input:not([type="checkbox"]):focus,
  select:focus {
    border-color: #7aa2f7;
  }
  small {
    color: #888;
  }
  .actions {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  button {
    padding: 8px 14px;
    border-radius: 8px;
    border: 1px solid #444;
    background: #2a2a2a;
    color: #eee;
    cursor: pointer;
  }
  button.primary {
    background: #7aa2f7;
    color: #111;
    border: none;
  }
  .ok {
    color: #9ece6a;
  }
  .err {
    color: #f7768e;
    font-size: 12px;
    white-space: pre-wrap;
  }
</style>
