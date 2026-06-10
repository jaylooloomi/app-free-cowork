<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, type StatusDto } from "./api";
  import { S } from "./strings";

  let input = $state("");
  let status = $state<StatusDto | null>(null);
  let error = $state("");
  let busy = $state(false);
  let history: string[] = $state([]);
  let hIndex = $state(-1);
  let el: HTMLInputElement | null = $state(null);

  async function refresh() {
    try {
      history = await api.getHistory();
    } catch {
      history = [];
    }
    try {
      status = await api.getStatus();
    } catch {
      status = null;
    }
  }

  onMount(() => {
    refresh();
    el?.focus();
    const unlisten = listen("palette-shown", () => {
      input = "";
      error = "";
      hIndex = -1;
      refresh();
      el?.focus();
    });
    const onBlur = () => api.hidePalette();
    window.addEventListener("blur", onBlur);
    return () => {
      unlisten.then((f) => f());
      window.removeEventListener("blur", onBlur);
    };
  });

  async function submit() {
    if (busy) return;
    error = "";
    busy = true;
    try {
      // 成功("launched"/"wizard")時後端已隱藏面板;失敗則顯示訊息並保持開啟
      await api.submitPrompt(input);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      submit();
    } else if (e.key === "Escape") {
      api.hidePalette();
    } else if (e.key === "ArrowUp" && hIndex < history.length - 1) {
      e.preventDefault();
      hIndex += 1;
      input = history[hIndex];
    } else if (e.key === "ArrowDown" && hIndex > -1) {
      e.preventDefault();
      hIndex -= 1;
      input = hIndex === -1 ? "" : history[hIndex];
    }
  }

  const statusText = $derived(
    !status
      ? ""
      : status.state === "ready"
        ? S.statusReady(status.model)
        : status.state === "needs_setup"
          ? S.statusNeedsSetup
          : S.statusDegraded(status.detail),
  );
</script>

<main class="palette">
  <input bind:this={el} bind:value={input} placeholder={S.placeholder} onkeydown={onKey} disabled={busy} />
  <div class="status" class:error={!!error}>{error || statusText}</div>
</main>

<style>
  .palette {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px;
  }
  input {
    font-size: 18px;
    padding: 12px 14px;
    border-radius: 8px;
    border: 1px solid #444;
    background: #1e1e1e;
    color: #eee;
    outline: none;
  }
  input:focus {
    border-color: #7aa2f7;
  }
  .status {
    font-size: 12px;
    color: #999;
    min-height: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status.error {
    color: #f7768e;
  }
</style>
