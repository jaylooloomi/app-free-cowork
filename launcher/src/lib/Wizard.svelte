<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, type StepResult } from "./api";
  import { S } from "./strings";

  type Row = { step: string; state: "pending" | "running" | "ok" | "fail"; detail: string };
  let rows: Row[] = $state([]);
  let done = $state(false);
  let failed = $state(false);
  /** signin 是否已由使用者按鈕觸發(本輪)。失敗重試會重設,回到按鈕等待。 */
  let signinRequested = $state(false);
  /** run() 停在 signin 等待使用者 → 顯示登入按鈕 */
  let awaitingSignin = $state(false);
  /** run() 正在執行步驟中 → 停用按鈕避免重入 */
  let busy = $state(false);
  /** finish() 進行中 → 停用「開始使用」 */
  let finishing = $state(false);
  /** wizardDone 失敗訊息(保留免責聲明與按鈕讓使用者重試) */
  let finishError = $state("");
  /** wizardPlan 失敗訊息(顯示重試按鈕) */
  let initError = $state("");

  async function init() {
    done = false;
    failed = false;
    signinRequested = false;
    awaitingSignin = false;
    finishError = "";
    initError = "";
    rows = [];
    let plan;
    try {
      plan = await api.wizardPlan();
    } catch (e) {
      initError = String(e);
      return;
    }
    rows = plan.steps.map((step) => ({ step, state: "pending" as const, detail: "" }));
    run();
  }

  onMount(() => {
    // 視窗啟動時皆為隱藏狀態,不可自動安裝 — 等後端真正顯示時才初始化
    const unlisten = listen("wizard-shown", () => {
      // 閒置(尚未開始/已完成/全部成功)→ 重新取得計畫;進行中則保留進度
      if (rows.length === 0 || done || rows.every((r) => r.state === "ok")) {
        init();
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  /** 從第一個尚未成功的步驟開始依序執行;signin 未經使用者觸發時停下等按鈕。 */
  async function run() {
    failed = false;
    awaitingSignin = false;
    busy = true;
    try {
      for (let i = 0; i < rows.length; i++) {
        if (rows[i].state === "ok") continue;
        if (rows[i].step === "signin" && !signinRequested) {
          awaitingSignin = true; // 該列維持 pending,顯示登入按鈕
          return;
        }
        rows[i].state = "running";
        rows[i].detail = "";
        let r: StepResult;
        try {
          r = await api.wizardRun(rows[i].step);
        } catch (e) {
          r = { ok: false, detail: String(e) };
        }
        rows[i].state = r.ok ? "ok" : "fail";
        rows[i].detail = r.detail;
        if (!r.ok) {
          failed = true;
          return;
        }
      }
      done = true;
    } finally {
      busy = false;
    }
  }

  function startSignin() {
    if (busy) return;
    signinRequested = true;
    run(); // run() 會把 signin 設為 running、await 後自動繼續
  }

  function retry() {
    if (busy) return;
    const i = rows.findIndex((r) => r.state === "fail");
    if (i < 0) return;
    rows[i].state = "pending";
    rows[i].detail = "";
    if (rows[i].step === "signin") signinRequested = false; // 回到按鈕等待
    run();
  }

  async function finish() {
    if (finishing) return;
    finishing = true;
    finishError = "";
    try {
      await api.wizardDone(); // 後端啟動暫存需求成功後才隱藏精靈
    } catch (e) {
      finishError = String(e); // 保留免責聲明與按鈕,使用者可重試
    } finally {
      finishing = false;
    }
  }
</script>

<main class="wizard">
  <h1>{S.wizardTitle}</h1>
  <p class="hint">{S.wizardHint}</p>
  <ul>
    {#each rows as row (row.step)}
      <li class={row.state}>
        <span class="mark"
          >{row.state === "ok" ? "✓" : row.state === "fail" ? "✗" : row.state === "running" ? "…" : "·"}</span
        >
        {S.wizardStepLabels[row.step] ?? row.step}
        {#if row.step === "signin" && awaitingSignin && row.state === "pending"}
          <button onclick={startSignin} disabled={busy}>{S.wizardSignin}</button>
        {/if}
        {#if row.detail && row.state === "fail"}<div class="detail">{row.detail}</div>{/if}
      </li>
    {/each}
  </ul>
  {#if initError}
    <div class="err">{initError}</div>
    <button onclick={init}>{S.wizardInitRetry}</button>
  {/if}
  {#if failed}<button onclick={retry} disabled={busy}>{S.wizardRetry}</button>{/if}
  {#if done}
    <div class="disclaimer">{S.wizardDisclaimer}</div>
    <button class="primary" onclick={finish} disabled={finishing}>{S.wizardStart}</button>
    {#if finishError}<div class="err">{finishError}</div>{/if}
  {/if}
</main>

<style>
  .wizard {
    padding: 24px;
    color: #eee;
  }
  h1 {
    font-size: 20px;
    margin: 0 0 4px;
  }
  .hint {
    color: #999;
    font-size: 13px;
  }
  ul {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  li {
    font-size: 15px;
  }
  li.ok .mark {
    color: #9ece6a;
  }
  li.fail .mark {
    color: #f7768e;
  }
  li.running .mark {
    color: #7aa2f7;
  }
  .mark {
    display: inline-block;
    width: 20px;
  }
  .detail {
    color: #f7768e;
    font-size: 12px;
    margin-left: 20px;
    white-space: pre-wrap;
  }
  .disclaimer {
    background: #2a2a2a;
    border-radius: 8px;
    padding: 12px;
    font-size: 13px;
    color: #ccc;
    margin: 12px 0;
  }
  .err {
    color: #f7768e;
    font-size: 13px;
    white-space: pre-wrap;
    margin: 8px 0;
  }
  button {
    padding: 8px 14px;
    border-radius: 8px;
    border: 1px solid #444;
    background: #2a2a2a;
    color: #eee;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.primary {
    background: #7aa2f7;
    color: #111;
    border: none;
  }
</style>
