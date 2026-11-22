//! Audio CD 整片轉 MP3 視窗程式
//!
//! 功能：
//! - 自動偵測含有 Audio CD 的光碟機
//! - 讀取整片 CD 的所有音軌 PCM 資料
//! - 串接成單一 MP3 檔案（使用純 Rust 的 rusty_mp3 編碼器）
//! - 從 Windows 系統字型目錄載入中文字型，正確顯示中文介面

use anyhow::{anyhow, Context, Result};
use cd_da_reader::CdReader;
use eframe::egui;
use rusty_mp3::{Mp3Encoder, Mp3EncoderConfig};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// 從 Windows 字型目錄載入中文字型（優先微軟雅黑）
fn load_chinese_fonts(ctx: &egui::Context) {
    // Windows 常見中文字型路徑（按優先順序）
    let font_candidates = [
        r"C:\Windows\Fonts\msyh.ttc",   // 微軟雅黑
        r"C:\Windows\Fonts\msyhbd.ttc", // 微軟雅黑 Bold
        r"C:\Windows\Fonts\simhei.ttf", // 黑體
        r"C:\Windows\Fonts\simsun.ttc", // 宋體
        r"C:\Windows\Fonts\simkai.ttf", // 楷體
        r"C:\Windows\Fonts\msjh.ttc",   // 微軟正黑體
        // 也嘗試 Linux/mac 常見路徑，方便開發測試
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/PingFang.ttc",
    ];

    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = false;

    for path in font_candidates {
        if let Ok(data) = std::fs::read(path) {
            let font_data = egui::FontData::from_owned(data);
            fonts
                .font_data
                .insert("chinese".to_owned(), font_data.into());

            // 將中文字型放到 Proportional 與 Monospace 最前面
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "chinese".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "chinese".to_owned());

            loaded = true;
            eprintln!("已載入中文字型: {}", path);
            break;
        }
    }

    if !loaded {
        eprintln!("警告：找不到系統中文字型，中文可能無法正確顯示");
    }

    ctx.set_fonts(fonts);
}

/// 背景工作狀態
enum WorkerMsg {
    Progress { current: usize, total: usize, msg: String },
    Done { path: PathBuf },
    Error(String),
}

/// 將整片 Audio CD 轉成單一 MP3
fn convert_whole_cd_to_mp3(
    output_path: PathBuf,
    tx: Sender<WorkerMsg>,
) -> Result<()> {
    let _ = tx.send(WorkerMsg::Progress {
        current: 0,
        total: 1,
        msg: "正在開啟光碟機…".to_string(),
    });

    let reader = CdReader::open_default()
        .context("無法開啟預設光碟機，請確認已插入 Audio CD")?;

    let toc = reader
        .read_toc()
        .context("讀取 TOC（目錄）失敗")?;

    let audio_tracks: Vec<_> = toc
        .tracks
        .iter()
        .filter(|t| t.is_audio)
        .collect();

    if audio_tracks.is_empty() {
        return Err(anyhow!("此光碟沒有音軌"));
    }

    let total_tracks = audio_tracks.len();
    let _ = tx.send(WorkerMsg::Progress {
        current: 0,
        total: total_tracks,
        msg: format!("發現 {} 首音軌，開始讀取…", total_tracks),
    });

    // 建立 MP3 編碼器（44.1 kHz 立體聲，320 kbps CBR）
    // 320 kbps 為 MP3 標準最高位元率，相容性最好且最接近 CD 音質
    let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
        bitrate_kbps: 320,
        vbr_quality: None,
    });

    let mut mp3_data: Vec<u8> = Vec::new();

    for (idx, track) in audio_tracks.iter().enumerate() {
        let track_num = track.number;
        let _ = tx.send(WorkerMsg::Progress {
            current: idx,
            total: total_tracks,
            msg: format!("正在讀取第 {} 軌（共 {} 軌）…", track_num, total_tracks),
        });

        // 讀取整軌 raw PCM（signed 16-bit LE stereo, 44.1 kHz）
        // 格式：交錯 L R L R …，每個 sample 2 bytes little-endian
        let pcm_bytes = reader
            .read_track(&toc, track_num)
            .with_context(|| format!("讀取第 {} 軌失敗", track_num))?;

        if pcm_bytes.len() % 2 != 0 {
            return Err(anyhow!("第 {} 軌 PCM 資料長度異常", track_num));
        }

        // push_pcm_s16le 直接接受 little-endian i16 的 raw bytes
        encoder
            .push_pcm_s16le(&pcm_bytes, 2, 44100)
            .map_err(|e| anyhow!("MP3 編碼失敗: {:?}", e))?;

        // 取出已產生的 MP3 frames
        loop {
            match encoder.next_packet() {
                Ok(packet) => mp3_data.extend_from_slice(&packet),
                Err(rusty_mp3::Error::Again) => break,
                Err(e) => return Err(anyhow!("取出 MP3 封包失敗: {:?}", e)),
            }
        }

        let _ = tx.send(WorkerMsg::Progress {
            current: idx + 1,
            total: total_tracks,
            msg: format!("第 {} 軌完成", track_num),
        });
    }

    // 結束編碼並 flush 剩餘資料
    encoder.finish();
    loop {
        match encoder.next_packet() {
            Ok(packet) => mp3_data.extend_from_slice(&packet),
            Err(rusty_mp3::Error::Again) | Err(rusty_mp3::Error::Eof) => break,
            Err(e) => return Err(anyhow!("結束編碼失敗: {:?}", e)),
        }
    }

    if mp3_data.is_empty() {
        return Err(anyhow!("沒有產生任何 MP3 資料"));
    }

    std::fs::write(&output_path, &mp3_data)
        .with_context(|| format!("寫入檔案失敗: {}", output_path.display()))?;

    let _ = tx.send(WorkerMsg::Done { path: output_path });
    Ok(())
}

struct App {
    status: String,
    progress: f32,
    is_working: bool,
    rx: Option<Receiver<WorkerMsg>>,
    last_output: Option<PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            status: "請插入 Audio CD，然後按下「整片轉為mp3」".to_string(),
            progress: 0.0,
            is_working: false,
            rx: None,
            last_output: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 先把訊息全部取出，避免持有 &self.rx 的同時修改 self.rx
        let mut messages = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }

        for msg in messages {
            match msg {
                WorkerMsg::Progress { current, total, msg } => {
                    self.progress = if total > 0 {
                        current as f32 / total as f32
                    } else {
                        0.0
                    };
                    self.status = msg;
                }
                WorkerMsg::Done { path } => {
                    self.is_working = false;
                    self.progress = 1.0;
                    self.status = format!("完成！已儲存至：{}", path.display());
                    self.last_output = Some(path);
                    self.rx = None;
                }
                WorkerMsg::Error(e) => {
                    self.is_working = false;
                    self.status = format!("錯誤：{}", e);
                    self.rx = None;
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.heading("Audio CD → 單一 MP3");
                ui.add_space(10.0);
                ui.label("將整片 Audio CD 的所有音軌串接成一個大 MP3 檔案");
                ui.add_space(20.0);

                let btn = egui::Button::new(
                    egui::RichText::new("整片轉為mp3").size(22.0),
                )
                .min_size(egui::vec2(220.0, 50.0));

                if ui
                    .add_enabled(!self.is_working, btn)
                    .clicked()
                {
                    // 讓使用者選擇輸出檔案
                    let dialog = rfd::FileDialog::new()
                        .set_title("選擇 MP3 儲存位置")
                        .set_file_name("cd_full.mp3")
                        .add_filter("MP3 音訊", &["mp3"]);

                    if let Some(path) = dialog.save_file() {
                        self.is_working = true;
                        self.progress = 0.0;
                        self.status = "開始轉換…".to_string();
                        self.last_output = None;

                        let (tx, rx) = mpsc::channel();
                        self.rx = Some(rx);

                        thread::spawn(move || {
                            if let Err(e) = convert_whole_cd_to_mp3(path, tx.clone()) {
                                let _ = tx.send(WorkerMsg::Error(format!("{:#}", e)));
                            }
                        });
                    }
                }

                ui.add_space(20.0);

                if self.is_working {
                    ui.add(
                        egui::ProgressBar::new(self.progress)
                            .show_percentage()
                            .desired_width(300.0),
                    );
                    ui.add_space(8.0);
                }

                ui.label(
                    egui::RichText::new(&self.status)
                        .size(16.0)
                        .color(if self.status.starts_with("錯誤") {
                            egui::Color32::RED
                        } else if self.status.starts_with("完成") {
                            egui::Color32::from_rgb(0, 140, 0)
                        } else {
                            egui::Color32::DARK_GRAY
                        }),
                );

                if let Some(path) = &self.last_output {
                    ui.add_space(10.0);
                    ui.hyperlink_to("開啟檔案所在資料夾", path.parent().unwrap_or(path).to_string_lossy());
                }

                ui.add_space(30.0);
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        "說明：程式會自動偵測含有 Audio CD 的光碟機，\n\
                         依序讀取所有音軌並編碼為 320 kbps MP3。\n\
                         需要系統已安裝光碟機且插入 Audio CD。",
                    )
                    .size(13.0)
                    .color(egui::Color32::GRAY),
                );
            });
        });

        // 有工作時持續重繪以更新進度
        if self.is_working {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([480.0, 380.0])
            .with_min_inner_size([400.0, 320.0])
            .with_title("整片 CD 轉 MP3"),
        ..Default::default()
    };

    eframe::run_native(
        "整片 CD 轉 MP3",
        options,
        Box::new(|cc| {
            // 從 Windows（或其他系統）讀取中文字型
            load_chinese_fonts(&cc.egui_ctx);
            Ok(Box::new(App::default()))
        }),
    )
}
