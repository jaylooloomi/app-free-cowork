<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { api, type StatusDto, type QueueDto, type ModelEntry, type Settings, type Recurrence, type ScheduleDto } from "./api";
  import { strings } from "./strings";

  let input = $state("");
  let status = $state<StatusDto | null>(null);
  let settings = $state<Settings | null>(null);
  // 介面語言以 settings.locale 為準;尚未取得設定前退回 zh-TW。
  // S 為 $derived,locale 變動(下次開啟面板重新 refresh)時整個面板會以新語言重繪。
  const S = $derived(strings(settings?.locale ?? "zh-TW"));
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
  // 可用性掃描
  let scanning = $state(false);
  let scanProgress = $state<{ done: number; total: number } | null>(null);
  // 預設只顯示可用模型(你的 Claude 帳號 + 免費);切換顯示全部
  let showAll = $state(false);
  let listening = $state(false);
  let capturing = $state(false); // 框選截圖進行中(避免重複觸發、暫停自動隱藏)
  let transient = $state("");
  let transientTimer: number | undefined;
  let listenTimer: number | undefined;
  // 貼上/拖入的圖片暫存路徑(送出時帶入 prompt)
  let attachments = $state<string[]>([]);
  // 使用者用「選資料夾」鈕指定的工作資料夾(Agent 在此作業);null = 用設定預設。
  // 黏著式:設定後持續沿用到多個任務,直到清除。
  let workdir = $state<string | null>(null);
  let picking = $state(false); // 資料夾選擇對話框進行中(暫停自動隱藏面板)

  // 排程模式:🕐 切換;送出時建立週期排程而非立即跑一次。
  let scheduleMode = $state(false);
  let recKind = $state<"every_minutes" | "every_hours" | "daily_at" | "weekly_at">("daily_at");
  let recEvery = $state(30); // every_minutes / every_hours 的數值
  let recHour = $state(9);
  let recMinute = $state(0);
  let recWeekday = $state(1); // Mon=1 .. Sun=7
  let runImmediately = $state(true);
  let schedules = $state<ScheduleDto[]>([]);

  function buildRecurrence(): Recurrence {
    if (recKind === "every_minutes") return { kind: "every_minutes", minutes: recEvery };
    if (recKind === "every_hours") return { kind: "every_hours", hours: recEvery };
    if (recKind === "daily_at") return { kind: "daily_at", hour: recHour, minute: recMinute };
    return { kind: "weekly_at", weekday: recWeekday, hour: recHour, minute: recMinute };
  }
  function recurrenceLabel(r: Recurrence): string {
    if (r.kind === "every_minutes") return S.schedEveryMin(r.minutes);
    if (r.kind === "every_hours") return S.schedEveryHour(r.hours);
    const hm = `${String(r.hour).padStart(2, "0")}:${String(r.minute).padStart(2, "0")}`;
    if (r.kind === "daily_at") return S.schedDailyAt(hm);
    return S.schedWeeklyAt(S.schedWeekdays[(r.weekday - 1) % 7], hm);
  }
  async function loadSchedules() {
    try {
      schedules = await api.listSchedules();
    } catch {
      schedules = [];
    }
  }

  // 串流結果回顯(背景模式):後端把 claude 的 stream-json 逐行轉送過來,
  // 前端解析後即時顯示助手文字、工具呼叫與最終結果。
  let taskRunning = $state(false);
  let taskText = $state(""); // 累積的助手文字(串流中)
  let taskTools = $state<string[]>([]); // 工具呼叫摘要(🔧)
  let taskResult = $state<string | null>(null); // result 行的最終文字
  let taskError = $state(false);
  let streamEl: HTMLElement | null = $state(null); // 串流文字區,用於自動捲到底
  let dismissTimer: number | undefined; // 任務完成後自動收起「執行結果」面板的計時器
  // 目前任務的世代號(= 後端 task id):只接受 gen 相符的串流事件,
  // 避免跨任務事件投遞競爭把舊任務輸出混進新任務。
  let activeGen = $state<number | null>(null);
  // 每個已完成任務的輸出快照(key = task id),供「看 detail」就地展開。
  // 串流結束時拍快照;與完成清單同生命週期(refreshQueue 會清掉已移除的)。
  let outputHistory = $state<Record<number, { text: string; tools: string[] }>>({});
  let expandedId = $state<number | null>(null); // 目前展開查看細節的已完成項目 id

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
      if (gen === queueGen) {
        queue = q;
        // 已從後端清單移除的項目,一併清掉殘留狀態:checking 動畫、輸出快照、展開狀態
        const ids = new Set(q.completed.map((c) => c.id));
        checking = checking.filter((id) => ids.has(id));
        for (const k of Object.keys(outputHistory)) {
          if (!ids.has(Number(k))) delete outputHistory[Number(k)];
        }
        if (expandedId !== null && !ids.has(expandedId)) expandedId = null;
      }
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
    loadSchedules();
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
      scanProgress = null;
      // 重新開啟面板時清掉「已完成」任務的舊輸出,回到乾淨狀態;
      // 但若仍在執行中(背景任務尚未結束)則保留,讓使用者繼續看串流。
      if (!taskRunning) resetTaskOutput(false);
      stopListening();
      refresh();
      refreshQueue();
      loadSchedules();
      el?.focus();
    });
    const unlistenQueue = listen("queue-changed", () => {
      refreshQueue();
    });
    // 掃描進度:後端對每個模型探測後 emit;結束時 scan-done 帶統計
    const unlistenScanProgress = listen<{ done: number; total: number }>("scan-progress", (e) => {
      scanProgress = { done: e.payload.done, total: e.payload.total };
    });
    const unlistenScanDone = listen<{
      free: number;
      subscription: number;
      broken: number;
      scanned: number;
      skipped: number;
    }>("scan-done", (e) => {
      scanning = false;
      scanProgress = null;
      showTransient(S.modelScanDone(e.payload));
      refreshModels();
    });
    // 串流回顯:任務啟動先記錄世代號並重置輸出區,逐行解析,結束時收尾。
    // gen 不符的事件一律忽略(過期/跨任務競爭)。
    const unlistenOutStart = listen<number>("task-output-start", (e) => {
      activeGen = e.payload;
      resetTaskOutput(true);
    });
    const unlistenOut = listen<{ gen: number; line: string }>("task-output", (e) => {
      if (e.payload.gen !== activeGen) return;
      onTaskLine(e.payload.line);
    });
    const unlistenOutEnd = listen<{ gen: number; code: number }>("task-output-end", (e) => {
      if (e.payload.gen !== activeGen) return;
      taskRunning = false;
      // 程序非正常結束又沒吐 result 行(崩潰/被殺/spawn 失敗)→ 標記為失敗
      if (taskResult === null && e.payload.code !== 0) taskError = true;
      // 拍快照:供完成清單那筆「看 detail」就地展開(result 行已先於 end 抵達)
      const text = taskResult ?? taskText;
      if (text || taskTools.length) {
        outputHistory[e.payload.gen] = { text, tools: [...taskTools] };
      }
      // 完成後約 5 秒自動收起「執行結果」面板(詳細仍保留在「已完成」清單可回看);
      // 新任務開始 / 重開面板 / 手動關閉都會經 resetTaskOutput 取消此計時器。
      clearTimeout(dismissTimer);
      dismissTimer = window.setTimeout(() => {
        if (!taskRunning) dismissOutput();
      }, 5000);
    });
    // busy(任務送出中)/ capturing(截圖中,後端會自行隱藏再顯示)時不自動隱藏
    const onBlur = () => {
      if (!busy && !capturing && !picking) api.hidePalette().catch(() => {});
    };
    window.addEventListener("blur", onBlur);
    // 視窗高度貼齊內容(佇列、模型選單、串流結果):下限 110、上限 600。
    // ResizeObserver 在 observe() 時會立刻送出初始觀測,首次顯示前就會收斂到內容高度。
    // 後端只設定位置、不設定大小,不會互相干擾。
    const win = getCurrentWindow();
    let lastH = 0;
    const ro = new ResizeObserver(() => {
      if (!rootEl) return;
      const h = Math.min(600, Math.max(110, Math.ceil(rootEl.offsetHeight)));
      if (h !== lastH) {
        lastH = h;
        win.setSize(new LogicalSize(640, h)).catch(() => {});
      }
    });
    if (rootEl) ro.observe(rootEl);
    return () => {
      unlistenShown.then((f) => f());
      unlistenQueue.then((f) => f());
      unlistenScanProgress.then((f) => f());
      unlistenScanDone.then((f) => f());
      unlistenOutStart.then((f) => f());
      unlistenOut.then((f) => f());
      unlistenOutEnd.then((f) => f());
      window.removeEventListener("blur", onBlur);
      ro.disconnect();
      clearTimeout(transientTimer);
      clearTimeout(listenTimer);
      clearTimeout(dismissTimer);
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
      // 排程模式:建立週期排程(非立即送出),清空輸入並重整排程清單後返回。
      if (scheduleMode) {
        await api.createSchedule(fullPrompt, workdir, buildRecurrence(), runImmediately);
        scheduleMode = false;
        input = "";
        attachments = [];
        hIndex = -1;
        showTransient(S.scheduledToast);
        await loadSchedules();
        return; // finally 仍會跑(busy=false、focus)
      }
      // "launched"/"wizard" 時後端已隱藏面板;"queued" 保持開啟並提示已入列
      const outcome = await api.submitPrompt(fullPrompt, workdir);
      // 背景(串流)模式啟動後面板保留顯示輸出 → 清空輸入,等下一個指令;
      // 入列時同樣清空並提示。前景模式後端已隱藏面板,清空無妨。
      if (outcome === "launched" || outcome === "queued") {
        input = "";
        attachments = [];
        hIndex = -1;
        if (outcome === "queued") showTransient(S.queuedToast);
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      // 送出時 busy=true 會 disable 輸入框使其失焦;送完(或失敗)重新 enable
      // 後把焦點還回去,方便連續下指令。等 tick 確保 DOM 已移除 disabled 再 focus。
      await tick();
      el?.focus();
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

  // ── 串流結果回顯 ──────────────────────────────────────────────────────────
  // 把一個 tool_use 區塊濃縮成一行可讀摘要:工具名 + 最具代表性的參數。
  function toolLabel(b: { name?: string; input?: Record<string, unknown> }): string {
    const name = b.name ?? "tool";
    const inp = b.input ?? {};
    const raw =
      inp.command ?? inp.file_path ?? inp.path ?? inp.pattern ?? inp.url ?? inp.description ?? "";
    const detail = typeof raw === "string" ? raw.replace(/\s+/g, " ").trim().slice(0, 64) : "";
    return detail ? `${name}: ${detail}` : name;
  }

  // 解析一行 stream-json(後端已過濾掉雜訊,只會收到 assistant / user / result /
  // post_turn_summary)。assistant 的文字累加顯示、tool_use 收進工具列;
  // result 行帶最終文字與 is_error,作為收尾。
  function onTaskLine(rawLine: string) {
    let msg: { type?: string; message?: { content?: unknown[] }; result?: unknown; is_error?: boolean };
    try {
      msg = JSON.parse(rawLine);
    } catch {
      return;
    }
    if (msg.type === "assistant") {
      for (const blk of msg.message?.content ?? []) {
        const b = blk as { type?: string; text?: string; name?: string; input?: Record<string, unknown> };
        if (b.type === "text" && typeof b.text === "string") taskText += b.text;
        else if (b.type === "tool_use") taskTools = [...taskTools, toolLabel(b)];
      }
    } else if (msg.type === "result") {
      taskResult = typeof msg.result === "string" ? msg.result : "";
      taskError = !!msg.is_error;
      taskRunning = false;
    }
  }

  function resetTaskOutput(running: boolean) {
    // 取消待執行的自動收起(新任務開始 / 重開面板 / 手動關閉都會經過這裡)
    clearTimeout(dismissTimer);
    taskText = "";
    taskTools = [];
    taskResult = null;
    taskError = false;
    taskRunning = running;
  }

  function dismissOutput() {
    resetTaskOutput(false);
  }

  // 解析快捷鍵字串(如 "Alt+J"、"Ctrl+Shift+M")並與 keydown 事件比對。
  // 修飾鍵需完全相符;主鍵比對 e.key(不分大小寫,space 特別處理)。
  function matchesHotkey(e: KeyboardEvent, combo: string): boolean {
    const parts = combo
      .split("+")
      .map((p) => p.trim().toLowerCase())
      .filter(Boolean);
    if (parts.length === 0) return false;
    const key = parts[parts.length - 1];
    const mods = parts.slice(0, -1);
    const wantAlt = mods.includes("alt");
    const wantCtrl = mods.includes("ctrl") || mods.includes("control");
    const wantShift = mods.includes("shift");
    const wantMeta =
      mods.includes("meta") || mods.includes("win") || mods.includes("cmd") || mods.includes("super");
    if (e.altKey !== wantAlt || e.ctrlKey !== wantCtrl || e.shiftKey !== wantShift || e.metaKey !== wantMeta) {
      return false;
    }
    const ek = (e.key || "").toLowerCase();
    if (key === "space") return ek === " ";
    return ek === key;
  }

  // 全域鍵(svelte:window)— 不管焦點在哪都生效:
  // 語音快捷鍵(預設 Alt+J)啟動語音輸入、截圖快捷鍵(預設 Alt+K)啟動框選截圖,
  // 兩者皆可在設定調整;Alt+H 仍是開/關面板。
  // Escape:選單開啟時先關選單,再按一次才隱藏面板。
  function onGlobalKey(e: KeyboardEvent) {
    if (matchesHotkey(e, settings?.voice_hotkey || "Alt+J")) {
      e.preventDefault();
      if (!busy && !offline) onMic();
      return;
    }
    if (matchesHotkey(e, settings?.capture_hotkey || "Alt+K")) {
      e.preventDefault();
      if (!busy && !offline && !capturing) onCapture();
      return;
    }
    if (matchesHotkey(e, settings?.schedule_hotkey || "Alt+L")) {
      e.preventDefault();
      scheduleMode = !scheduleMode;
      return;
    }
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

  // 截圖鈕:後端先隱藏面板 → 觸發 Windows 框選 → 取回剪貼簿影像 → 回傳暫存路徑;
  // 取回後面板已被後端重新顯示,這裡把圖片加進附件(同貼上圖片的流程)。
  async function onCapture() {
    if (capturing) return;
    error = "";
    capturing = true;
    try {
      const path = await api.captureScreenshot();
      if (path) attachments = [...attachments, path];
    } catch (e) {
      error = String(e);
    } finally {
      capturing = false;
      el?.focus();
    }
  }

  // 選資料夾鈕:開原生對話框 → 設為本次(及後續)任務的工作資料夾(黏著、可清除)。
  async function onPickFolder() {
    if (picking) return;
    error = "";
    picking = true;
    try {
      const dir = await api.pickFolder();
      if (dir) workdir = dir;
    } catch (e) {
      error = String(e);
    } finally {
      picking = false;
      el?.focus();
    }
  }

  function clearWorkdir() {
    workdir = null;
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

  // 已完成項目「打勾」:先加進 checking(立即顯示 ✓ + 刪除線 + 淡出動畫),
  // 約 0.5 秒後請後端移除。實際從清單消失靠 queue-changed → refreshQueue,
  // 屆時 refreshQueue 會把已不在清單的 id 從 checking 清掉(見該函式),
  // 因此不在這裡手動清,避免「移除前先閃回未打勾」的瞬間。
  let checking = $state<number[]>([]);
  function onCheckCompleted(id: number) {
    if (checking.includes(id)) return;
    checking = [...checking, id];
    window.setTimeout(() => api.dismissCompleted(id).catch((e) => (error = String(e))), 500);
  }

  // 點已完成項目的文字 → 就地展開/收合它的執行細節(僅有快照者可展開)。
  function toggleDetail(id: number) {
    if (!outputHistory[id]) return;
    expandedId = expandedId === id ? null : id;
  }

  // A-Z 排序,但 claude(anthropic)永遠置頂 — localeCompare 會把 "claude"
  // 按字串排到中間,因此先抽出 claude、其餘排序後再前插。
  function sortModels(list: ModelEntry[]): ModelEntry[] {
    const claude = list.filter((m) => m.name === "claude");
    const rest = list.filter((m) => m.name !== "claude").sort((a, b) => a.name.localeCompare(b.name));
    return [...claude, ...rest];
  }

  async function refreshModels() {
    const list = await api.listModelsUi();
    models = sortModels(list);
  }

  async function toggleDropdown() {
    if (dropdownOpen) {
      dropdownOpen = false;
      return;
    }
    error = "";
    try {
      await refreshModels();
      dropdownOpen = true;
    } catch (e) {
      error = String(e);
    }
  }

  async function scan() {
    if (scanning) return;
    error = "";
    scanning = true;
    scanProgress = null;
    try {
      // 結束統計與列表刷新由 "scan-done" 事件處理;此處只兜底錯誤。
      await api.scanModels();
    } catch (e) {
      error = String(e);
      scanning = false;
      scanProgress = null;
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

  const tierLabels: Record<ModelEntry["tier"], string> = $derived({
    free: S.tierFree,
    subscription: S.tierSubscription,
    unknown: S.tierUnknown,
    anthropic: S.tierAnthropic,
    broken: S.tierBroken,
    incompatible: S.tierIncompatible,
  });

  // ✓ 以設定檔的 model 為準(offline 時 status.model 可能不可靠),退回 status?.model
  const currentModel = $derived(settings?.model || status?.model);

  // 預設只顯示「可用」模型(你的 Claude 帳號 + 免費);顯示全部時不過濾。
  // 目前選中的模型一律保留可見,即使被篩掉。
  const visibleModels = $derived(
    showAll
      ? models
      : models.filter((m) => m.tier === "anthropic" || m.tier === "free" || m.name === currentModel),
  );

  // claude 哨符在選單顯示成易讀名稱
  function modelLabel(name: string): string {
    return name === "claude" ? S.claudeOfficial : name;
  }

  const micTip = $derived(S.micTooltip(settings?.voice_hotkey || "Alt+J"));
  const capTip = $derived(S.captureTooltip(settings?.capture_hotkey || "Alt+K"));

  // 串流中顯示累積文字;收到 result 行後改顯示其最終文字(兩者內容一致,避免重複)。
  const displayText = $derived(taskResult ?? taskText);
  const showOutput = $derived(taskRunning || taskResult !== null);
  // 串流文字增長時自動捲到底,讓最新內容可見。
  $effect(() => {
    void displayText;
    void taskTools.length;
    if (streamEl) streamEl.scrollTop = streamEl.scrollHeight;
  });
</script>

<svelte:window onkeydown={onGlobalKey} />

<main class="palette" bind:this={rootEl}>
  {#if workdir}
    <div class="attachments">
      <span class="attach workdir" title={workdir}>
        <span class="attach-name">📁 {workdir.split(/[\\/]/).pop() || workdir}</span>
        <button class="attach-x" onclick={clearWorkdir} title={S.workdirClearTip} aria-label={S.workdirClearTip}>✕</button>
      </span>
    </div>
  {/if}
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
  {#if scheduleMode}
    <div class="sched-picker">
      <select bind:value={recKind}>
        <option value="every_minutes">{S.schedKindMinutes}</option>
        <option value="every_hours">{S.schedKindHours}</option>
        <option value="daily_at">{S.schedKindDaily}</option>
        <option value="weekly_at">{S.schedKindWeekly}</option>
      </select>
      {#if recKind === "every_minutes" || recKind === "every_hours"}
        <input type="number" min="1" bind:value={recEvery} class="sp-num" />
        <span>{recKind === "every_minutes" ? S.schedUnitMin : S.schedUnitHour}</span>
      {:else}
        {#if recKind === "weekly_at"}
          <select bind:value={recWeekday}>
            {#each S.schedWeekdays as w, i (i)}<option value={i + 1}>{S.schedWeekdayPrefix}{w}</option>{/each}
          </select>
        {/if}
        <input type="number" min="0" max="23" bind:value={recHour} class="sp-num" />
        <span>:</span>
        <input type="number" min="0" max="59" bind:value={recMinute} class="sp-num" />
      {/if}
      <label class="sp-now"><input type="checkbox" bind:checked={runImmediately} /> {S.schedRunNow}</label>
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
      class="cap"
      class:active={!!workdir}
      onclick={onPickFolder}
      onmousedown={(e) => e.preventDefault()}
      disabled={busy || offline || picking}
      title={S.pickFolderTooltip}
      aria-label={S.pickFolderTooltip}
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
        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
      </svg>
    </button>
    <button
      class="cap"
      onclick={onCapture}
      onmousedown={(e) => e.preventDefault()}
      disabled={busy || offline || capturing}
      title={capTip}
      aria-label={capTip}
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
        <path d="M6 3v13a2 2 0 0 0 2 2h13" />
        <path d="M18 21V8a2 2 0 0 0-2-2H3" />
      </svg>
    </button>
    <button
      class="sched"
      class:active={scheduleMode}
      onclick={() => (scheduleMode = !scheduleMode)}
      onmousedown={(e) => e.preventDefault()}
      title={S.schedTooltip}
      aria-label={S.schedTitle}
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
        <circle cx="12" cy="12" r="9" />
        <polyline points="12 7 12 12 15 14" />
      </svg>
    </button>
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

  {#if schedules.length > 0 && !dropdownOpen}
    <div class="queue sched-list">
      <div class="qsection">{S.schedSection}</div>
      {#each schedules as s (s.id)}
        <div class="qrow" class:sched-off={!s.enabled}>
          <span class="qtext">{recurrenceLabel(s.recurrence)} · {truncate(s.prompt)}</span>
          <button class="link" onclick={() => api.setScheduleEnabled(s.id, !s.enabled).then(loadSchedules)}>
            {s.enabled ? S.schedPause : S.schedEnable}
          </button>
          <button class="x" onclick={() => api.deleteSchedule(s.id).then(loadSchedules)} title={S.schedDeleteTip} aria-label={S.schedDeleteTip}>✕</button>
        </div>
      {/each}
    </div>
  {/if}

  <!-- 選單開啟時暫時隱藏佇列,避免合計高度頂到上限被裁切 -->
  <!-- 「排隊/執行中」與「已完成」拆成兩個獨立區域:已完成區不自己捲動(隨內容長高),
       展開的細節因此只有自己那一條捲軸,不再與外層巢狀(避免兩條捲軸疊一起)。 -->
  {#if queue && !dropdownOpen}
    {#if queue.running || queue.queued.length > 0}
      <div class="queue active">
        <div class="qsection">{S.sectionActive}</div>
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
    {#if queue.completed.length > 0}
      <div class="queue completed">
        <div class="qsection">{S.sectionCompleted}</div>
        {#each queue.completed as c (c.id)}
          <div class="qrow done" class:checked={checking.includes(c.id)} class:failed={!c.ok}>
            <button
              class="check"
              onclick={() => onCheckCompleted(c.id)}
              title={S.completedDismissTip}
              aria-label={S.completedDismissTip}
            >
              {#if checking.includes(c.id)}✓{:else}{c.ok ? "○" : "✗"}{/if}
            </button>
            {#if outputHistory[c.id]}
              <button class="qtext detail-toggle" onclick={() => toggleDetail(c.id)} title={S.completedDetailTip}>
                <span class="caret">{expandedId === c.id ? "▾" : "▸"}</span>{truncate(c.prompt)}
              </button>
            {:else}
              <span class="qtext">{truncate(c.prompt)}</span>
            {/if}
            {#if !c.ok}<span class="qstate fail">{S.completedFailed}</span>{/if}
          </div>
          {#if expandedId === c.id && outputHistory[c.id]}
            <div class="qdetail">
              {#if outputHistory[c.id].tools.length}
                <div class="otools">
                  {#each outputHistory[c.id].tools as t, i (i)}<span class="otool">🔧 {t}</span>{/each}
                </div>
              {/if}
              {#if outputHistory[c.id].text}<div class="qdetail-text">{outputHistory[c.id].text}</div>{/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  {/if}

  <!-- 串流結果回顯:背景任務執行中即時顯示助手文字 / 工具呼叫,結束顯示最終結果 -->
  {#if showOutput}
    <div class="output" class:err={taskError}>
      <div class="ohead">
        <span class="ostat">
          {#if taskRunning}
            <span class="ospin" aria-hidden="true"></span>{S.outputRunning}
          {:else}
            {taskError ? S.outputFailed : S.outputDone}
          {/if}
        </span>
        <button class="ox" onclick={dismissOutput} title={S.outputDismiss} aria-label={S.outputDismiss}>✕</button>
      </div>
      {#if taskTools.length}
        <div class="otools">
          {#each taskTools as t, i (i)}<span class="otool">🔧 {t}</span>{/each}
        </div>
      {/if}
      {#if displayText}
        <div class="ostream" bind:this={streamEl}>{displayText}</div>
      {:else if taskRunning}
        <div class="owait">{S.outputWaiting}</div>
      {/if}
    </div>
  {/if}

  <div class="bar">
    <div class="status" class:error={!!error}>
      {error || transient || (capturing ? S.capturingHint : listening ? S.voiceHint : statusText)}
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
      <div class="dhead">
        <button class="scan" onclick={scan} disabled={scanning} title={S.modelOnlyUsable}>
          {#if scanning}
            {scanProgress ? S.modelScanning(scanProgress.done, scanProgress.total) : S.modelScan}
          {:else}
            {S.modelScan}
          {/if}
        </button>
        <label class="showall">
          <input type="checkbox" bind:checked={showAll} />
          {S.modelShowAll}
        </label>
      </div>
      {#each visibleModels as m (m.name)}
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
  /* ── 串流結果回顯 ── */
  .output {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: 1px solid var(--panel-border);
    border-radius: 8px;
    background: var(--panel-bg);
    padding: 8px 10px;
  }
  .output.err {
    border-color: #f7768e;
  }
  .ohead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    color: #9ece6a;
  }
  .output.err .ohead {
    color: #f7768e;
  }
  .ostat {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .ospin {
    width: 10px;
    height: 10px;
    border: 2px solid #7aa2f7;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .ox {
    border: none;
    background: none;
    color: #888;
    font-size: 11px;
    line-height: 1;
    padding: 0;
    cursor: pointer;
  }
  .ox:hover {
    color: #f7768e;
  }
  .otools {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .otool {
    font-size: 11px;
    color: #bbb;
    background: rgba(255, 255, 255, 0.06);
    border-radius: 5px;
    padding: 2px 6px;
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ostream {
    font-size: 13px;
    color: #e8e8e8;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
    max-height: 240px;
    overflow: auto;
  }
  .owait {
    font-size: 12px;
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
  .mic,
  .cap,
  .sched {
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
  .mic:hover:not(:disabled),
  .cap:hover:not(:disabled),
  .sched:hover:not(:disabled) {
    color: #eee;
    background: rgba(255, 255, 255, 0.08);
  }
  .mic:disabled,
  .cap:disabled {
    opacity: 0.4;
    cursor: default;
  }
  /* 已選工作資料夾時,資料夾鈕高亮提示 */
  .cap.active {
    color: #7aa2f7;
  }
  .sched.active {
    color: #7aa2f7;
  }
  .sched-picker {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    padding: 8px 4px 2px;
    font-size: 13px;
    color: #bbb;
  }
  .sched-picker select,
  .sched-picker input {
    background: #2a2a2a;
    color: #eee;
    border: 1px solid #444;
    border-radius: 6px;
    padding: 4px 6px;
    font: inherit;
  }
  .sched-picker .sp-num {
    width: 52px;
  }
  .sched-picker .sp-now {
    display: flex;
    align-items: center;
    gap: 4px;
    color: #999;
  }
  .qrow.sched-off {
    opacity: 0.45;
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
    padding: 8px 10px;
    border: 1px solid var(--panel-border);
    border-radius: 8px;
    background: var(--panel-bg);
    font-size: 13px;
    color: #ccc;
  }
  /* 排隊/執行中:列數多時自己捲(獨立區域,與已完成區互不影響) */
  .queue.active {
    max-height: 130px;
    overflow-y: auto;
  }
  /* 已完成:不自己捲動,隨內容(含就地展開的細節)長高。
     如此展開時只剩 .qdetail-text 一條捲軸,不會與外層巢狀。 */
  .queue.completed {
    overflow: visible;
  }
  .qsection {
    font-size: 11px;
    color: #777;
    padding-bottom: 2px;
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
  /* 已完成項目(待打勾移除):○ 成功(綠)/ ✗ 失敗(紅);打勾 → ✓ + 刪除線 + 淡出 */
  .qrow.done {
    transition: opacity 0.4s ease;
  }
  .qrow.done .qtext {
    color: #aaa;
  }
  .check {
    flex-shrink: 0;
    width: 18px;
    border: none;
    background: none;
    color: #9ece6a;
    font-size: 13px;
    line-height: 1;
    padding: 0;
    cursor: pointer;
  }
  .qrow.failed .check {
    color: #f7768e;
  }
  .check:hover {
    filter: brightness(1.4);
  }
  .qrow.checked .check {
    color: #9ece6a;
  }
  .qrow.checked .qtext {
    text-decoration: line-through;
    color: #666;
    transition: color 0.3s ease;
  }
  .qrow.checked {
    opacity: 0.45;
  }
  .qstate.fail {
    color: #f7768e;
  }
  /* 已完成項目的文字 → 可點開細節的按鈕 */
  .detail-toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    border: none;
    background: none;
    color: #aaa;
    font: inherit;
    text-align: left;
    padding: 0;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .detail-toggle:hover {
    color: #eee;
  }
  .caret {
    flex-shrink: 0;
    color: #777;
    font-size: 10px;
  }
  /* 就地展開的執行細節 */
  .qdetail {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 2px 0 4px 26px;
    padding: 8px 10px;
    border-left: 2px solid var(--panel-border);
    background: rgba(255, 255, 255, 0.03);
    border-radius: 0 6px 6px 0;
  }
  .qdetail-text {
    font-size: 12px;
    color: #ddd;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
    max-height: 180px;
    overflow: auto;
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
  .badge.incompatible {
    color: #b0b0b0;
    background: rgba(176, 176, 176, 0.12);
  }
  .badge.broken {
    color: #f7768e;
    background: rgba(247, 118, 142, 0.12);
  }
  .dhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 6px 6px;
    border-bottom: 1px solid var(--panel-border);
    margin-bottom: 4px;
  }
  .scan {
    border: 1px solid var(--panel-border);
    background: var(--panel-bg);
    color: #ccc;
    font-size: 12px;
    border-radius: 6px;
    padding: 3px 8px;
    cursor: pointer;
    white-space: nowrap;
  }
  .scan:hover:not(:disabled) {
    border-color: #7aa2f7;
    color: #eee;
  }
  .scan:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .showall {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: #aaa;
    cursor: pointer;
    white-space: nowrap;
  }
  .showall input {
    cursor: pointer;
  }
</style>
