import time
import subprocess
import sys
import os

def ensure_dependencies():
    """確保環境依賴已安裝"""
    # 1. 檢查並安裝 playwright python 套件
    try:
        import playwright
    except ImportError:
        print("[*] 正在安裝 Playwright 套件...")
        subprocess.check_call([sys.executable, "-m", "pip", "install", "playwright"])
    
    # 2. 檢查並安裝 Chromium 瀏覽器核心
    # 我們嘗試啟動 playwright，如果失敗就執行 install
    print("[*] 正在檢查瀏覽器核心環境...")
    install_cmd = [sys.executable, "-m", "playwright", "install", "chromium"]
    try:
        # 這裡不直接啟動，而是執行安裝指令，Playwright 會自動跳過已安裝的部分
        subprocess.run(install_cmd, check=True, capture_output=True)
    except subprocess.CalledProcessError as e:
        print(f"[!] 安裝瀏覽器核心時發生錯誤，嘗試強制安裝: {e}")
        subprocess.run(install_cmd, check=True)

def scrape_and_pull():
    from playwright.sync_api import sync_playwright
    
    with sync_playwright() as p:
        print("[*] 啟動自動化引擎...")
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(user_agent="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        page = context.new_page()

        page_num = 1
        total_installed = 0

        while True:
            url = f"https://ollama.com/search?c=cloud&page={page_num}"
            print(f"\n[*] 正在掃描第 {page_num} 頁: {url}")
            
            try:
                page.goto(url, wait_until="networkidle", timeout=30000)
                
                # 取得所有模型區塊 li
                items = page.query_selector_all('li') 
                
                if not items or len(items) == 0:
                    print(f"[!] 第 {page_num} 頁沒有更多模型，掃描結束。")
                    break

                found_in_page = 0
                for item in items:
                    name_el = item.query_selector('h2, span.text-lg, a[href^="/library/"]')
                    if not name_el: continue
                    
                    # 取得純淨的模型名稱
                    raw_text = name_el.inner_text().strip()
                    base_name = raw_text.split('\n')[0].split()[0]
                    
                    content = item.inner_text().lower()
                    
                    if "cloud" in content:
                        model_full = f"{base_name}:cloud"
                        print(f"--- [偵測到] {model_full} ---")
                        
                        if run_ollama_pull(model_full):
                            found_in_page += 1
                            total_installed += 1
                        
                        time.sleep(1)

                if found_in_page == 0:
                    print(f"[!] 第 {page_num} 頁未發現新的 Cloud 模型。")
                    break
                
                page_num += 1

            except Exception as e:
                print(f"[錯誤] 處理頁面時發生異常: {e}")
                break

        browser.close()
        print(f"\n[任務完成] 總共處理了 {total_installed} 個雲端模型。")

def run_ollama_pull(model_name):
    try:
        # 直接調用系統的 ollama 指令
        process = subprocess.run(["ollama", "pull", model_name])
        return process.returncode == 0
    except FileNotFoundError:
        print("[危險] 系統找不到 'ollama' 指令，請先安裝 Ollama 並加入 PATH。")
        return False

if __name__ == "__main__":
    print("========================================")
    print("   Ollama Cloud 一鍵掃描與安裝工具")
    print("========================================\n")
    
    # 第一步：環境自癒 (Self-healing environment)
    ensure_dependencies()
    
    # 第二步：執行爬蟲安裝
    scrape_and_pull()