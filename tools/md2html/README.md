# Markdown → HTML 轉換器（Rust GUI）

純 Rust 寫的簡易 Markdown 轉 HTML 桌面程式。

## 功能

- 左側多行輸入區：貼上或輸入 Markdown
- 右側輸出區：顯示轉換後的 HTML
- 一鍵轉換按鈕
- 支援表格、刪除線、任務清單、腳註等常見擴充語法
- 可複製 HTML 到剪貼簿
- **已支援繁體中文顯示**（自動載入系統中文字型）

## 中文顯示說明

程式啟動時會自動嘗試載入以下系統字型（依序）：

- Windows：微軟正黑體（Microsoft JhengHei）、細明體、標楷體
- macOS：PingFang、STHeiti
- Linux：Noto Sans CJK TC、文泉驛微米黑 等

只要你的系統有安裝上述任一款支援繁體中文的字型，介面上的中文就能正常顯示。

## 使用的套件

- [eframe](https://crates.io/crates/eframe) + [egui](https://crates.io/crates/egui)：即時模式 GUI
- [pulldown-cmark](https://crates.io/crates/pulldown-cmark)：高速 Markdown 解析與 HTML 輸出

## 編譯與執行

```bash
# 需要先安裝 Rust：https://rustup.rs
cargo run --release
```

編譯完成後，執行檔會位於 `target/release/markdown-to-html-gui`（Windows 為 `.exe`）。

## 介面操作

1. 在左側輸入區貼上 Markdown 內容
2. 按下中間的「轉換為 HTML」按鈕
3. 右側即可看到產生的 HTML
4. 需要時可點「複製 HTML」把結果放到剪貼簿

## 授權

MIT 或 Apache-2.0，可自由使用。
