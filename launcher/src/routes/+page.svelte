<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Palette from "$lib/Palette.svelte";
  import Wizard from "$lib/Wizard.svelte";
  import Settings from "$lib/Settings.svelte";

  // SPA 模式(+layout.ts 已關閉 SSR),此處只會在 Tauri webview 內執行
  const label = getCurrentWindow().label;
</script>

{#if label === "palette"}
  <Palette />
{:else if label === "wizard"}
  <Wizard />
{:else}
  <Settings />
{/if}
