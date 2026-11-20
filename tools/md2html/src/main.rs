use eframe::egui;
use pulldown_cmark::{html, Options, Parser};
use std::path::Path;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 720.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("Markdown → HTML 轉換器"),
        ..Default::default()
    };

    eframe::run_native(
        "Markdown to HTML",
        options,
        Box::new(|cc| {
            // 設定支援繁體中文的字型
            setup_chinese_fonts(&cc.egui_ctx);
            Ok(Box::new(MarkdownApp::default()))
        }),
    )
}

/// 載入系統中支援繁體中文的字型，解決中文顯示成亂碼 / 方塊的問題
fn setup_chinese_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 常見支援繁體中文的系統字型路徑（依平台優先順序）
    let font_paths = [
        // Windows
        "C:/Windows/Fonts/msjh.ttc",      // Microsoft JhengHei（微軟正黑體）
        "C:/Windows/Fonts/msjhbd.ttc",
        "C:/Windows/Fonts/mingliu.ttc",   // 細明體
        "C:/Windows/Fonts/kaiu.ttf",      // 標楷體
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Microsoft/Microsoft JhengHei.ttf",
        // Linux (Noto CJK / 文泉驛 等)
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        // 其他可能位置
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];

    let mut loaded = false;

    for path in font_paths {
        if Path::new(path).exists() {
            match std::fs::read(path) {
                Ok(data) => {
                    // 插入字型資料
                    fonts.font_data.insert(
                        "chinese_font".to_owned(),
                        egui::FontData::from_owned(data),
                    );

                    // 把中文字型放到優先位置（Proportional 與 Monospace 都加上）
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .insert(0, "chinese_font".to_owned());

                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .insert(0, "chinese_font".to_owned());

                    loaded = true;
                    eprintln!("已載入中文字型: {}", path);
                    break;
                }
                Err(e) => {
                    eprintln!("無法讀取字型 {}: {}", path, e);
                }
            }
        }
    }

    if !loaded {
        eprintln!("警告：找不到支援繁體中文的系統字型，中文可能顯示為方塊。");
        eprintln!("請安裝 Noto Sans CJK 或 Microsoft JhengHei 等字型。");
    }

    ctx.set_fonts(fonts);
}

struct MarkdownApp {
    markdown_input: String,
    html_output: String,
    status: String,
}

impl Default for MarkdownApp {
    fn default() -> Self {
        Self {
            markdown_input: String::from(
                "# Hello Markdown\n\n\
這是一段 **粗體** 與 *斜體* 的示範文字。\n\n\
- 第一項：支援繁體中文\n\
- 第二項：表格、刪除線、任務清單都可使用\n\n\
| 功能       | 支援情況 |\n\
|------------|----------|\n\
| 表格       | ✅       |\n\
| 刪除線     | ✅       |\n\
| 任務清單   | ✅       |\n\n\
~~這段被刪除~~\n\n\
- [x] 已完成任務\n\
- [ ] 未完成任務\n\n\
```rust\n\
fn main() {\n\
    println!(\"你好，Rust！\");\n\
}\n\
```\n",
            ),
            html_output: String::new(),
            status: String::from("準備就緒，請輸入 Markdown 後按下「轉換為 HTML」"),
        }
    }
}

impl eframe::App for MarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 上方標題列
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Markdown → HTML 轉換器");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("純 Rust · egui + pulldown-cmark").weak());
                });
            });
            ui.add_space(4.0);
        });

        // 下方狀態列
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(&self.status);
            });
            ui.add_space(4.0);
        });

        // 中央主要區域
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);

            // ===== 轉換按鈕（固定在上方，永遠看得到） =====
            ui.vertical_centered(|ui| {
                let button = egui::Button::new(
                    egui::RichText::new("  轉換為 HTML  ").size(18.0).strong(),
                )
                .min_size(egui::vec2(240.0, 44.0));

                if ui.add(button).clicked() {
                    self.convert();
                }
            });

            ui.add_space(10.0);

            // 左右分割：左邊輸入、右邊輸出
            ui.columns(2, |columns| {
                // ===== 左側：Markdown 輸入區 =====
                columns[0].group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Markdown 輸入");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("清空").clicked() {
                                self.markdown_input.clear();
                                self.status = "已清空輸入區".to_string();
                            }
                        });
                    });
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("input_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.markdown_input)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(22)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("在此貼上或輸入 Markdown 內容..."),
                            );
                        });
                });

                // ===== 右側：HTML 輸出區 =====
                columns[1].group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("HTML 輸出");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("複製 HTML").clicked() {
                                ui.output_mut(|o| o.copied_text = self.html_output.clone());
                                self.status = "HTML 已複製到剪貼簿".to_string();
                            }
                        });
                    });
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("output_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut output = self.html_output.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut output)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(22)
                                    .font(egui::TextStyle::Monospace)
                                    .interactive(false),
                            );
                        });
                });
            });
        });
    }
}

impl MarkdownApp {
    fn convert(&mut self) {
        if self.markdown_input.trim().is_empty() {
            self.html_output.clear();
            self.status = "輸入區是空的，請先輸入 Markdown".to_string();
            return;
        }

        // 啟用常用的 Markdown 擴充功能
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

        let parser = Parser::new_ext(&self.markdown_input, options);

        let mut html_output = String::new();
        html::push_html(&mut html_output, parser);

        // 將 h1、h2、h3 全部降級為 h4（包含可能帶有屬性的情況）
        html_output = demote_headings_to_h4(html_output);

        self.html_output = html_output;
        self.status = format!(
            "轉換完成！輸入 {} 字元 → 輸出 {} 字元（h1/h2/h3 已改為 h4）",
            self.markdown_input.len(),
            self.html_output.len()
        );
    }
}

/// 把 HTML 中的 <h1>、<h2>、<h3> 及其結束標籤全部改成 <h4>
fn demote_headings_to_h4(html: String) -> String {
    let mut result = html;

    // 先處理結束標籤（較單純）
    result = result.replace("</h1>", "</h4>");
    result = result.replace("</h2>", "</h4>");
    result = result.replace("</h3>", "</h4>");

    // 處理開始標籤（包含可能的屬性，例如 <h1 id="xxx">）
    // 使用簡單且安全的方式：找到 <h1 / <h2 / <h3 後替換成 <h4
    for level in ["h1", "h2", "h3"] {
        let open_tag = format!("<{}", level);
        let mut new_result = String::with_capacity(result.len());
        let mut rest = result.as_str();

        while let Some(pos) = rest.find(&open_tag) {
            new_result.push_str(&rest[..pos]);
            new_result.push_str("<h4");
            rest = &rest[pos + open_tag.len()..];
        }
        new_result.push_str(rest);
        result = new_result;
    }

    result
}
