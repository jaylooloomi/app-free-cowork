<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { api, type StatusDto, type QueueDto, type ModelEntry, type Settings } from "./api";
  import { S } from "./strings";

  let input = $state("");
  let status = $state<StatusDto | null>(null);
  let settings = $state<Settings | null>(null);
  let error = $state("");
  let busy = $state(false);
  let history: string[] = $state([]);
  let hIndex = $state(-1);
  let el: HTMLInputElement | null = $state(null);
  let rootEl: HTMLElement | null = $state(null);
  let dropdownEl: HTMLElement | null = $state(null);
  let chipsEl: HTMLElement | null = $state(null);

  // v1.1 state
  let queue = $state<QueueDto | null>(null);
  let models = $state<ModelEntry[]>([]);
  let dropdownOpen = $state(false);
  let listening = $state(false);
  let transient = $state("");
  let transientTimer: number | undefined;
  let listenTimer: number | undefined;
  // 貼上/拖入的圖片暫存路徑(送出時帶入 prompt)
  let attachments = $state<string[]>([]);

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
    try {
      settings = await api.getSettings();
    } catch {
      // 保留上次成功值;從未成功時 micTooltip / ✓ 走預設 fallback
    }
  }

  // 世代計數器:queue-changed 連發時,過期的 queueList 回應不可覆蓋較新的結果
  let queueGen = 0;
  async function refreshQueue() {
    const gen = ++queueGen;
    try {
      const q = await api.queueList();
      if (gen === queueGen) queue = q;
    } catch {
      if (gen === queueGen) queue = null;
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
      attachments = [];
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
      if (!busy) api.hidePalette().catch(() => {});
    };
    window.addEventListener("blur", onBlur);
    // 視窗高度貼齊內容(佇列、模型選單):下限 110、上限 420。
    // ResizeObserver 在 observe() 時會立刻送出初始觀測,首次顯示前就會收斂到內容高度。
    // 後端只設定位置、不設定大小,不會互相干擾。
    const win = getCurrentWindow();
    let lastH = 0;
    const ro = new ResizeObserver(() => {
      if (!rootEl) return;
      const h = Math.min(420, Math.max(110, Math.ceil(rootEl.offsetHeight)));
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

  // 模型選單開啟期間監聽 document pointerdown:點在選單與 chips 之外就關閉。
  // 用 $effect 讓所有關閉路徑(選取、Esc、palette-shown 重置)都自動解除監聽。
  $effect(() => {
    if (!dropdownOpen) return;
    const onPointerDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (dropdownEl?.contains(t) || chipsEl?.contains(t)) return;
      dropdownOpen = false;
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  });

  async function submit() {
    if (busy || offline) return;
    if (!input.trim() && attachments.length === 0) return;
    error = "";
    busy = true;
    try {
      // 有附圖時把圖片路徑接進 prompt,讓 Claude Code 讀檔看圖
      const fullPrompt = attachments.length
        ? `${attachments.map((p) => `請看這張圖片:${p}`).join("\n")}\n${input}`.trim()
        : input;
      // "launched"/"wizard" 時後端已隱藏面板;"queued" 保持開啟並提示已入列
      const outcome = await api.submitPrompt(fullPrompt);
      if (outcome === "queued") {
        input = "";
        attachments = [];
        hIndex = -1;
        showTransient(S.queuedToast);
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // 貼上圖片:存成暫存檔,顯示為可移除的附件卡。
  async function onPaste(e: ClipboardEvent) {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const it of items) {
      if (it.kind === "file" && it.type.startsWith("image/")) {
        e.preventDefault();
        const file = it.getAsFile();
        if (!file) continue;
        try {
          const buf = new Uint8Array(await file.arrayBuffer());
          const ext = (it.type.split("/")[1] || "png").toLowerCase();
          const path = await api.savePastedImage(Array.from(buf), ext);
          attachments = [...attachments, path];
        } catch (err) {
          error = String(err);
        }
      }
    }
  }

  function removeAttachment(i: number) {
    attachments = attachments.filter((_, idx) => idx !== i);
  }

  // Escape 走全域(svelte:window)— 不管焦點在哪都能關閉選單/面板,並處理層級:
  // 選單開啟時先關選單,再按一次才隱藏面板。
  function onGlobalKey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (dropdownOpen) {
      dropdownOpen = false;
      return;
    }
    api.hidePalette().catch(() => {});
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      submit();
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
    // Win+H 需要插入點(caret)在輸入框內,先把焦點放回輸入框再觸發。
    // 按鈕本身的 mousedown 已被 preventDefault,點擊不會搶走焦點。
    el?.focus();
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
      const list = await api.listModelsUi();
      models = list.sort((a, b) => a.name.localeCompare(b.name));
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
      if (settings) settings = { ...settings, model: name };
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
    anthropic: S.tierAnthropic,
  };

  // claude 哨符在選單顯示成易讀名稱
  function modelLabel(name: string): string {
    return name === "claude" ? "Claude(Anthropic 官方)" : name;
  }

  const hasQueue = $derived(!!queue && (queue.running !== null || queue.queued.length > 0));

  const micTip = S.micTooltip();

  // ✓ 以設定檔的 model 為準(offline 時 status.model 可能不可靠),退回 status?.model
  const currentModel = $derived(settings?.model || status?.model);
</script>

<svelte:window onkeydown={onGlobalKey} />

<main class="palette" bind:this={rootEl}>
  {#if attachments.length > 0}
    <div class="attachments">
      {#each attachments as path, i (path)}
        <span class="attach">
          <span class="attach-name">🖼 {path.split(/[\\/]/).pop()}</span>
          <button class="attach-x" onclick={() => removeAttachment(i)} title={S.attachRemoveTip} aria-label={S.attachRemoveTip}>✕</button>
        </span>
      {/each}
      <span class="attach-hint">{S.attachHint}</span>
    </div>
  {/if}
  <div class="input-row">
    <input
      bind:this={el}
      bind:value={input}
      placeholder={S.placeholder}
      onkeydown={onKey}
      onpaste={onPaste}
      oninput={() => listening && stopListening()}
      disabled={busy || offline}
    />
    <button
      class="mic"
      class:listening
      onclick={onMic}
      onmousedown={(e) => e.preventDefault()}
      disabled={busy || offline}
      title={micTip}
      aria-label={micTip}
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

  <!-- 選單開啟時暫時隱藏佇列,避免合計高度頂到 420 上限被裁切 -->
  {#if hasQueue && queue && !dropdownOpen}
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
    <div class="chips" bind:this={chipsEl}>
      {#if planLabel}
        <button class="chip" onclick={openPlanPage} title={S.planTooltip}>{planLabel}</button>
      {/if}
      {#if status}
        <button class="chip model" onclick={toggleDropdown}>{modelLabel(status.model)} ▾</button>
      {/if}
    </div>
  </div>

  {#if dropdownOpen}
    <div class="dropdown" bind:this={dropdownEl}>
      {#each models as m (m.name)}
        <button class="opt" onclick={() => pickModel(m.name)}>
          <span class="check">{m.name === currentModel ? "✓" : ""}</span>
          <span class="name">{modelLabel(m.name)}</span>
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
  .attachments {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .attach {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: #ddd;
    background: var(--panel-bg);
    border: 1px solid var(--panel-border);
    border-radius: 6px;
    padding: 3px 6px 3px 8px;
    max-width: 240px;
  }
  .attach-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .attach-x {
    border: none;
    background: none;
    color: #888;
    font-size: 11px;
    line-height: 1;
    padding: 0;
    cursor: pointer;
  }
  .attach-x:hover {
    color: #f7768e;
  }
  .attach-hint {
    font-size: 11px;
    color: #888;
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
  .badge.anthropic {
    color: #c8a2ff;
    background: rgba(200, 162, 255, 0.15);
  }
</style>
