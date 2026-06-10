export const S = {
  placeholder: "想要我做什麼?(例:幫我整理桌面,並且建立資料夾分類)",
  statusReady: (model: string) => `就緒 · ${model}`,
  statusNeedsSetup: "首次使用:送出後將自動安裝必要元件",
  statusDegraded: (d: string) => `注意:${d}`,
  statusOffline: "離線 — 雲端模型需要網路連線",
  launched: "已啟動,可關閉此面板",
  empty: "請輸入需求",
};
