<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api } from "$lib/api";

  let sentences = $state<string[]>([]);
  let shown = $state<number>(-1); // 已顯示到第幾句(含)
  let speaking = $state(false);
  let fading = $state(false);
  let preferredVoice = "";

  let bars = $state<number[]>(Array.from({ length: 28 }, () => 7));
  let raf = 0;
  function animateWave() {
    bars = bars.map(() => (speaking ? 10 + Math.random() * 36 : 7));
    raf = requestAnimationFrame(animateWave);
  }

  function splitSentences(text: string): string[] {
    return text
      .split(/(?<=[。!?！?；])|\n/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  function pickVoice(voices: SpeechSynthesisVoice[]): SpeechSynthesisVoice | null {
    if (preferredVoice) {
      const m = voices.find((v) => v.name === preferredVoice);
      if (m) return m;
    }
    return (
      voices.find((v) => v.lang === "zh-TW") ||
      voices.find((v) => /^zh/i.test(v.lang)) ||
      voices.find((v) => /HsiaoChen|Hanhan|Zhiwei|Chinese|Mandarin/i.test(v.name)) ||
      null
    );
  }

  function speakFrom(i: number) {
    if (i >= sentences.length) {
      finish();
      return;
    }
    shown = i;
    const u = new SpeechSynthesisUtterance(sentences[i]);
    u.lang = "zh-TW";
    const v = pickVoice(speechSynthesis.getVoices());
    if (v) u.voice = v;
    u.rate = 1.0;
    u.pitch = 1.05;
    u.onstart = () => (speaking = true);
    u.onend = () => {
      speaking = false;
      setTimeout(() => speakFrom(i + 1), 260);
    };
    u.onerror = () => {
      // 沒語音/被擋:仍逐句往下顯示(無聲),確保字幕走完
      speaking = false;
      setTimeout(() => speakFrom(i + 1), 700);
    };
    speechSynthesis.speak(u);
  }

  function finish() {
    speaking = false;
    fading = true;
    setTimeout(async () => {
      try {
        await api.announcerDone();
      } catch {}
    }, 1400);
  }

  function announce(text: string) {
    speechSynthesis.cancel();
    fading = false;
    sentences = splitSentences(text);
    shown = -1;
    if (sentences.length === 0) {
      finish();
      return;
    }
    speakFrom(0);
  }

  onMount(() => {
    // 透明視窗:讓 OS acrylic / 桌面透出,只顯示玻璃面板
    document.body.classList.add("announcer-body");
    // 取使用者偏好語音(可空)
    api
      .getSettings()
      .then((s) => (preferredVoice = s.announce_voice || ""))
      .catch(() => {});
    // 預熱語音清單(getVoices 首次可能為空)
    void speechSynthesis.getVoices();
    animateWave();

    const un = listen<{ text: string }>("announce", (e) => announce(e.payload.text));
    return () => {
      cancelAnimationFrame(raf);
      un.then((f) => f());
    };
  });
</script>

<div class="wrap" class:fading>
  <div class="panel">
    <div class="head">
      <div class="orb" class:on={speaking}><i></i><span class="ring"></span></div>
      <div class="title">FreeCowork<b>任務完成</b></div>
    </div>
    <div class="subs">
      {#each sentences as s, i}
        <div class="line" class:show={i <= shown} class:active={i === shown}>{s}</div>
      {/each}
    </div>
    <div class="wave">
      {#each bars as h}<span style="height:{h}px"></span>{/each}
    </div>
  </div>
</div>

<style>
  :global(html),
  :global(body.announcer-body) {
    background: transparent !important;
    overflow: hidden;
  }
  .wrap {
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 14px;
    transition: opacity 0.5s, transform 0.5s;
  }
  .wrap.fading {
    opacity: 0;
    transform: translateY(12px) scale(0.98);
  }
  .panel {
    width: 100%;
    background: rgba(26, 28, 38, 0.55);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 18px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5), inset 0 1px 0 rgba(255, 255, 255, 0.08);
    padding: 16px 20px 18px;
    color: #eef1f8;
    font-family: "Segoe UI", "Microsoft JhengHei", sans-serif;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }
  .orb {
    position: relative;
    width: 34px;
    height: 34px;
    flex: none;
  }
  .orb i {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: radial-gradient(circle at 35% 30%, #cfe0ff, #8ab4ff 45%, #b388ff 100%);
    box-shadow: 0 0 20px rgba(138, 180, 255, 0.55);
  }
  .orb .ring {
    position: absolute;
    inset: -6px;
    border-radius: 50%;
    border: 2px solid rgba(138, 180, 255, 0.5);
    opacity: 0;
  }
  .orb.on .ring {
    animation: ring 1.4s ease-out infinite;
  }
  .orb.on i {
    animation: pulse 1.1s ease-in-out infinite;
  }
  @keyframes ring {
    0% {
      transform: scale(0.7);
      opacity: 0.7;
    }
    100% {
      transform: scale(1.5);
      opacity: 0;
    }
  }
  @keyframes pulse {
    0%,
    100% {
      transform: scale(1);
    }
    50% {
      transform: scale(1.08);
    }
  }
  .title {
    font-size: 12px;
    color: #aeb6c9;
  }
  .title b {
    display: block;
    color: #fff;
    font-size: 15px;
    font-weight: 600;
  }
  .subs {
    min-height: 64px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
  }
  .line {
    font-size: 18px;
    line-height: 1.5;
    font-weight: 500;
    color: #f4f7ff;
    opacity: 0;
    transform: translateY(8px);
    filter: blur(5px);
    transition: opacity 0.45s, transform 0.45s, filter 0.45s;
  }
  .line.show {
    opacity: 0.5;
    transform: none;
    filter: none;
  }
  .line.active {
    opacity: 1;
  }
  .wave {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    height: 30px;
    margin-top: 10px;
  }
  .wave span {
    width: 4px;
    border-radius: 2px;
    background: linear-gradient(180deg, #8ab4ff, #b388ff);
    opacity: 0.85;
    transition: height 0.09s ease;
  }
</style>
