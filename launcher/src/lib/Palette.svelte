<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { api, type StatusDto, type QueueDto, type ModelEntry } from "./api";
  import { S } from "./strings";

  let input = $state("");
  let status = $state<StatusDto | null>(null);
  let error = $state("");
  let busy = $state(false);
  let history: string[] = $state([]);
  let hIndex = $state(-1);
  let el: HTMLInputElement | null = $state(null);
  let rootEl: HTMLElement | null = $state(null);

  // v1.1 state
  let queue = $state<QueueDto | null>(null);
  let models = $state<ModelEntry[]>([]);
  let dropdownOpen = $state(false);
  let listening = $state(false);
  let transient = $state("");
  let transientTimer: number | undefined;
  let listenTimer: number | undefined;

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

  async function refreshQueue() {
    try {
      queue = await api.queueList();
    } catch {
      queue = null;
    }
  }

  function showTransient(msg: string) {
    transient = msg;
    clearTimeout(transientTimer);
    transientTimer = window.setTimeout(() => (transient = ""), 3000);
  }

  function stopListening() {
    listening = false;
    clearTimeout(listenTimer);
  }

  onMount(() => {
    refresh();
    refreshQueue();
    el?.focus();
    // OS acrylic 真的套上才用玻璃樣式;查詢失敗一律退回純色
    api
      .effectsApplied()
      .then((glass) => document.body.classList.add(glass ? "fx-glass" : "fx-solid"))
      .catch(() => document.body.classList.add("fx-solid"));
    const unlistenShown = listen("palette-shown", () => {
      input = "";
      error = "";
      transient = "";
      hIndex = -1;
      dropdownOpen = false;
      stopListening();
      refresh();
      refreshQueue();
      el?.focus();
    });
    const unlistenQueue = listen("queue-changed", () => {
      refreshQueue();
    });
    // busy(任務送出中)時不自動隱藏 — 避免提交瞬間失焦把面板關掉
    const onBlur = () => {
      if (!busy) api.hidePalette();
    };
    window.addEventListener("blur", onBlur);
    // 視窗高度跟著內容長(佇列、模型選單):基準 168、上限 420。
    // 後端只設定位置、不設定大小,不會互相干擾。
    const win = getCurrentWindow();
    let lastH = 0;
    const ro = new ResizeObserver(() => {
      if (!rootEl) return;
      const h = Math.min(420, Math.max(168, Math.ceil(rootEl.offsetHeight)));
      if (h !== lastH) {
        lastH = h;
        win.setSize(new LogicalSize(640, h)).catch(() => {});
      }
    });
    if (rootEl) ro.observe(rootEl);
    return () => {
      unlistenShown.then((f) => f());
      unlistenQueue.then((f) => f());
      window.removeEventListener("blur", onBlur);
      ro.disconnect();
      clearTimeout(transientTimer);
      clearTimeout(listenTimer);
    };
  });

  async function submit() {
    if (busy || offline) return;
    error = "";
    busy = true;
    try {
      // "launched"/"wizard" 時後端已隱藏面板;"queued" 保持開啟並提示已入列
      const outcome = await api.submitPrompt(input);
      if (outcome === "queued") {
        input = "";
        hIndex = -1;
        showTransient(S.queuedToast);
      }
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

  async function onMic() {
    error = "";
    try {
      await api.startVoiceInput();
    } catch (e) {
      error = String(e);
      return;
    }
    // 無法偵測 Windows 語音輸入的實際狀態 — 只做視覺提示:
    // 輸入有變化或 10 秒後就停止脈動。
    listening = true;
    clearTimeout(listenTimer);
    listenTimer = window.setTimeout(() => (listening = false), 10000);
  }

  async function stopTask() {
    error = "";
    try {
      await api.taskStop();
    } catch (e) {
      error = String(e);
    }
  }

  async function cancelTask(id: number) {
    error = "";
    try {
      await api.queueCancel(id);
      // queue-changed 事件會刷新列表
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleDropdown() {
    if (dropdownOpen) {
      dropdownOpen = false;
      return;
    }
    error = "";
    try {
      models = await api.listModelsUi();
      dropdownOpen = true;
    } catch (e) {
      error = String(e);
    }
  }

  async function pickModel(name: string) {
    dropdownOpen = false;
    error = "";
    try {
      await api.setModel(name);
      status = await api.getStatus();
    } catch (e) {
      error = String(e);
    }
  }

  function openPlanPage() {
    api.openUrl("https://ollama.com/settings").catch((e) => (error = String(e)));
  }

  /** 按「字元」截斷(CJK / surrogate pair 安全),超長加省略號。 */
  function truncate(s: string, n = 40): string {
    const chars = [...s];
    return chars.length > n ? chars.slice(0, n).join("") + "…" : s;
  }

  const offline = $derived(status?.state === "offline");

  // offline 文案以後端 detail 為單一來源(避免前後端字串分歧)
  const statusText = $derived(
    !status
      ? ""
      : status.state === "ready"
        ? S.statusReady(status.model)
        : status.state === "needs_setup"
          ? S.statusNeedsSetup
          : status.state === "offline"
            ? status.detail
            : S.statusDegraded(status.detail),
  );

  const planLabel = $derived(
    !status?.plan
      ? ""
      : status.plan === "free"
        ? S.planFree
        : status.plan === "pro"
          ? S.planPro
          : status.plan === "max"
            ? S.planMax
            : status.plan,
  );

  const tierLabels: Record<ModelEntry["tier"], string> = {
    free: S.tierFree,
    subscription: S.tierSubscription,
    unknown: S.tierUnknown,
  };

  const hasQueue = $derived(!!queue && (queue.running !== null || queue.queued.length > 0));
</script>

<main class="palette" bind:this={rootEl}>
  <div class="input-row">
    <input
      bind:this={el}
      bind:value={input}
      placeholder={S.placeholder}
      onkeydown={onKey}
      oninput={() => listening && stopListening()}
      disabled={busy || offline}
    />
    <button
      class="mic"
      class:listening
      onclick={onMic}
      disabled={busy || offline}
      title={S.micTooltip}
      aria-label={S.micTooltip}
    >
      <svg
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
        <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
        <line x1="12" y1="19" x2="12" y2="23" />
        <line x1="8" y1="23" x2="16" y2="23" />
      </svg>
    </button>
  </div>

  {#if hasQueue && queue}
    <div class="queue">
      {#if queue.running}
        <div class="qrow running">
          <span class="qtext">▶ {truncate(queue.running.prompt)}</span>
          <span class="qstate">{S.queueRunning}</span>
          {#if queue.running.background}
            <button class="link" onclick={stopTask}>{S.queueStop}</button>
          {/if}
        </div>
      {/if}
      {#each queue.queued as t, i (t.id)}
        <div class="qrow">
          <span class="qtext">{i + 1}. {truncate(t.prompt)}</span>
          <span class="qstate">{S.queueQueued}</span>
          <button class="x" onclick={() => cancelTask(t.id)} title={S.queueCancelTip} aria-label={S.queueCancelTip}>
            ✕
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <div class="bar">
    <div class="status" class:error={!!error}>
      {error || transient || (listening ? S.voiceHint : statusText)}
    </div>
    <div class="chips">
      {#if planLabel}
        <button class="chip" onclick={openPlanPage} title={S.planTooltip}>{planLabel}</button>
      {/if}
      {#if status}
        <button class="chip model" onclick={toggleDropdown}>{status.model} ▾</button>
      {/if}
    </div>
  </div>

  {#if dropdownOpen}
    <div class="dropdown">
      {#each models as m (m.name)}
        <button class="opt" onclick={() => pickModel(m.name)}>
          <span class="check">{m.name === status?.model ? "✓" : ""}</span>
          <span class="name">{m.name}</span>
          <span class="badge {m.tier}">{tierLabels[m.tier]}</span>
        </button>
      {/each}
    </div>
  {/if}
</main>

<style>
  .palette {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px;
  }
  .input-row {
    display: flex;
    align-items: center;
    border: 1px solid var(--panel-border);
    border-radius: 8px;
    background: var(--panel-bg);
    padding-right: 8px;
  }
  .input-row:focus-within {
    border-color: #7aa2f7;
  }
  input {
    flex: 1;
    min-width: 0;
    font-size: 18px;
    padding: 12px 14px;
    border: none;
    background: transparent;
    color: #eee;
    outline: none;
  }
  .mic {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: #999;
    cursor: pointer;
    padding: 0;
  }
  .mic:hover:not(:disabled) {
    color: #eee;
    background: rgba(255, 255, 255, 0.08);
  }
  .mic:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .mic.listening {
    color: #7aa2f7;
  }
  .mic.listening::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    border: 2px solid #7aa2f7;
    animation: pulse 1.2s ease-out infinite;
    pointer-events: none;
  }
  @keyframes pulse {
    0% {
      transform: scale(0.8);
      opacity: 0.9;
    }
    100% {
      transform: scale(1.5);
      opacity: 0;
    }
  }
  .queue {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 150px;
    overflow-y: auto;
    padding: 8px 10px;
    border: 1px solid var(--panel-border);
    border-radius: 8px;
    background: var(--panel-bg);
    font-size: 13px;
    color: #ccc;
  }
  .qrow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 20px;
  }
  .qtext {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .qstate {
    flex-shrink: 0;
    font-size: 12px;
    color: #888;
  }
  .qrow.running .qstate {
    color: #9ece6a;
  }
  .link {
    flex-shrink: 0;
    border: none;
    background: none;
    color: #7aa2f7;
    font-size: 12px;
    padding: 0;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .x {
    flex-shrink: 0;
    border: none;
    background: none;
    color: #888;
    font-size: 12px;
    line-height: 1;
    padding: 0 2px;
    cursor: pointer;
  }
  .x:hover {
    color: #f7768e;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .status {
    flex: 1;
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
  .chips {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .chip {
    font-size: 12px;
    color: #ccc;
    background: var(--panel-bg);
    border: 1px solid var(--panel-border);
    border-radius: 999px;
    padding: 2px 10px;
    cursor: pointer;
    max-width: 220px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip:hover {
    border-color: #7aa2f7;
    color: #eee;
  }
  .dropdown {
    display: flex;
    flex-direction: column;
    max-height: 180px;
    overflow-y: auto;
    border: 1px solid var(--panel-border);
    border-radius: 8px;
    background: var(--panel-bg);
    padding: 4px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    background: none;
    color: #ddd;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .opt:hover {
    background: rgba(122, 162, 247, 0.15);
  }
  .check {
    width: 14px;
    flex-shrink: 0;
    color: #9ece6a;
  }
  .name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .badge {
    flex-shrink: 0;
    font-size: 11px;
    border-radius: 999px;
    padding: 1px 8px;
  }
  .badge.free {
    color: #9ece6a;
    background: rgba(158, 206, 106, 0.15);
  }
  .badge.subscription {
    color: #ff9e64;
    background: rgba(255, 158, 100, 0.15);
  }
  .badge.unknown {
    color: #aaa;
    background: rgba(255, 255, 255, 0.08);
  }
</style>
