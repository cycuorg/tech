use cpal::traits::{DeviceTrait, HostTrait};
use eframe::egui::{self, Color32, RichText, Vec2};
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

/// 讀取 Windows「預設播放端點」的系統主音量（0.0..=1.0）。
/// 這是 Windows 音訊端點音量，不是 rodio 的數位音量。
#[cfg(windows)]
fn get_windows_endpoint_volume() -> anyhow::Result<f32> {
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        // windows 0.58 的 CoInitializeEx 回傳 HRESULT，而非 Result。
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let initialized = if hr.0 >= 0 {
            true
        } else if hr.0 == 0x8001_0106u32 as i32 {
            // RPC_E_CHANGED_MODE：執行緒已由其他元件以不同 apartment 初始化。
            // COM 仍可使用，但不能由本函式呼叫 CoUninitialize。
            false
        } else {
            return Err(anyhow::anyhow!("COM 初始化失敗，HRESULT：0x{:08X}", hr.0 as u32));
        };

        let result = (|| -> anyhow::Result<f32> {
            let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint: IAudioEndpointVolume = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
            Ok(endpoint.GetMasterVolumeLevelScalar()?.clamp(0.0, 1.0))
        })();

        // result closure 結束時，COM 介面已先行釋放，再關閉本函式建立的 COM 初始化。
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// 設定 Windows「預設播放端點」的系統主音量（0.0..=1.0）。
#[cfg(windows)]
fn set_windows_endpoint_volume(volume: f32) -> anyhow::Result<()> {
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        // windows 0.58 的 CoInitializeEx 回傳 HRESULT，而非 Result。
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let initialized = if hr.0 >= 0 {
            true
        } else if hr.0 == 0x8001_0106u32 as i32 {
            // RPC_E_CHANGED_MODE：執行緒已由其他元件以不同 apartment 初始化。
            false
        } else {
            return Err(anyhow::anyhow!("COM 初始化失敗，HRESULT：0x{:08X}", hr.0 as u32));
        };

        let result = (|| -> anyhow::Result<()> {
            let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint: IAudioEndpointVolume = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
            endpoint.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null())?;
            Ok(())
        })();

        if initialized {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
fn get_windows_endpoint_volume() -> anyhow::Result<f32> {
    anyhow::bail!("Windows 音效端點音量控制僅支援 Windows")
}

#[cfg(not(windows))]
fn set_windows_endpoint_volume(_volume: f32) -> anyhow::Result<()> {
    anyhow::bail!("Windows 音效端點音量控制僅支援 Windows")
}

const EQ_BANDS: usize = 10;
const EQ_FREQUENCIES: [f32; EQ_BANDS] = [20.0, 36.0, 65.0, 117.0, 210.0, 378.0, 680.0, 1224.0, 2203.0, 4000.0];
const EQ_MIN_DB: f32 = -12.0;
const EQ_MAX_DB: f32 = 12.0;

const THEME_NAMES: [&str; 5] = ["午夜藍", "石墨黑", "海洋青", "琥珀暖金", "珍珠白"];

#[derive(Clone, Copy)]
struct ThemePalette {
    bg: Color32,
    card: Color32,
    card_alt: Color32,
    border: Color32,
    text: Color32,
    secondary: Color32,
    muted: Color32,
    accent: Color32,
    accent_hover: Color32,
    accent_active: Color32,
    success: Color32,
    danger: Color32,
    device: Color32,
    stop_button: Color32,
    status_bg: Color32,
    widget_bg: Color32,
    widget_inactive: Color32,
    widget_hover: Color32,
    extreme_bg: Color32,
}

fn theme_palette(index: usize) -> ThemePalette {
    let rgb = |r, g, b| Color32::from_rgb(r, g, b);
    match index % THEME_NAMES.len() {
        1 => ThemePalette { // 石墨黑：冷調紫藍
            bg: rgb(17, 17, 23), card: rgb(29, 29, 39), card_alt: rgb(23, 23, 32),
            border: rgb(57, 55, 73), text: rgb(244, 241, 250), secondary: rgb(190, 183, 207),
            muted: rgb(139, 132, 160), accent: rgb(167, 139, 250), accent_hover: rgb(184, 160, 255),
            accent_active: rgb(126, 93, 215), success: rgb(91, 210, 164), danger: rgb(245, 133, 145),
            device: rgb(205, 198, 222), stop_button: rgb(65, 61, 78), status_bg: rgb(23, 23, 32),
            widget_bg: rgb(39, 38, 51), widget_inactive: rgb(55, 52, 72), widget_hover: rgb(78, 70, 104),
            extreme_bg: rgb(13, 13, 19),
        },
        2 => ThemePalette { // 海洋青
            bg: rgb(8, 24, 29), card: rgb(17, 43, 49), card_alt: rgb(13, 35, 41),
            border: rgb(35, 76, 82), text: rgb(231, 249, 247), secondary: rgb(166, 205, 202),
            muted: rgb(112, 158, 157), accent: rgb(38, 196, 176), accent_hover: rgb(75, 218, 198),
            accent_active: rgb(17, 145, 133), success: rgb(104, 220, 160), danger: rgb(255, 143, 126),
            device: rgb(190, 225, 220), stop_button: rgb(43, 75, 79), status_bg: rgb(13, 35, 41),
            widget_bg: rgb(25, 57, 62), widget_inactive: rgb(34, 78, 82), widget_hover: rgb(44, 105, 105),
            extreme_bg: rgb(5, 17, 22),
        },
        3 => ThemePalette { // 琥珀暖金
            bg: rgb(29, 22, 17), card: rgb(48, 37, 27), card_alt: rgb(39, 29, 22),
            border: rgb(87, 65, 43), text: rgb(255, 244, 225), secondary: rgb(219, 194, 160),
            muted: rgb(166, 139, 105), accent: rgb(232, 164, 73), accent_hover: rgb(247, 190, 105),
            accent_active: rgb(183, 119, 40), success: rgb(139, 205, 133), danger: rgb(239, 126, 100),
            device: rgb(235, 213, 180), stop_button: rgb(87, 62, 43), status_bg: rgb(39, 29, 22),
            widget_bg: rgb(59, 45, 32), widget_inactive: rgb(78, 58, 39), widget_hover: rgb(105, 75, 45),
            extreme_bg: rgb(21, 15, 11),
        },
        4 => ThemePalette { // 珍珠白
            bg: rgb(239, 243, 248), card: rgb(255, 255, 255), card_alt: rgb(231, 237, 245),
            border: rgb(208, 218, 231), text: rgb(31, 43, 61), secondary: rgb(76, 91, 112),
            muted: rgb(112, 126, 145), accent: rgb(39, 112, 205), accent_hover: rgb(28, 91, 177),
            accent_active: rgb(22, 76, 151), success: rgb(27, 151, 105), danger: rgb(198, 78, 67),
            device: rgb(59, 79, 104), stop_button: rgb(218, 225, 235), status_bg: rgb(231, 237, 245),
            widget_bg: rgb(239, 243, 248), widget_inactive: rgb(220, 229, 240), widget_hover: rgb(202, 217, 235),
            extreme_bg: rgb(255, 255, 255),
        },
        _ => ThemePalette { // 午夜藍（預設）
            bg: rgb(13, 18, 29), card: rgb(24, 32, 47), card_alt: rgb(19, 26, 39),
            border: rgb(43, 56, 76), text: rgb(240, 244, 252), secondary: rgb(174, 190, 212),
            muted: rgb(130, 148, 174), accent: rgb(91, 174, 255), accent_hover: rgb(115, 190, 255),
            accent_active: rgb(42, 111, 190), success: rgb(70, 210, 145), danger: rgb(235, 139, 112),
            device: rgb(185, 198, 217), stop_button: rgb(56, 65, 82), status_bg: rgb(19, 26, 39),
            widget_bg: rgb(31, 42, 60), widget_inactive: rgb(39, 53, 74), widget_hover: rgb(54, 76, 105),
            extreme_bg: rgb(16, 22, 34),
        },
    }
}

fn apply_theme(ctx: &egui::Context, index: usize) {
    let p = theme_palette(index);
    let light = index % THEME_NAMES.len() == 4;
    let mut visuals = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.card;
    visuals.extreme_bg_color = p.extreme_bg;
    visuals.faint_bg_color = p.card_alt;
    visuals.code_bg_color = p.card_alt;
    visuals.override_text_color = Some(p.text);
    visuals.hyperlink_color = p.accent_hover;
    visuals.selection.bg_fill = p.accent_active;
    visuals.selection.stroke.color = p.text;
    visuals.widgets.noninteractive.bg_fill = p.widget_bg;
    visuals.widgets.noninteractive.fg_stroke.color = p.secondary;
    visuals.widgets.inactive.bg_fill = p.widget_inactive;
    visuals.widgets.inactive.fg_stroke.color = p.text;
    visuals.widgets.hovered.bg_fill = p.widget_hover;
    visuals.widgets.hovered.fg_stroke.color = p.text;
    visuals.widgets.active.bg_fill = p.accent_active;
    visuals.widgets.active.fg_stroke.color = Color32::WHITE;
    ctx.set_visuals(visuals);
}

/// 已選取的裝置 + 對應的播放資源。OutputStream 必須保持存活。
struct PlayingDevice {
    name: String,
    _stream: OutputStream,
    sink: Sink,
    volume: f32, // 該輸出裝置的個別音量（0.0..=1.0）
}

/// 二階 peaking EQ（RBJ Audio EQ Cookbook），每個聲道各自保留濾波器狀態。
#[derive(Clone)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }
}

impl Biquad {
    fn set_peaking(&mut self, sample_rate: f32, frequency: f32, q: f32, gain_db: f32) {
        if sample_rate <= 0.0 { return; }
        let frequency = frequency.clamp(1.0, sample_rate * 0.45);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * frequency / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = (-2.0 * cos_w0) / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        // Transposed Direct Form II
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }
}

/// 每個輸出裝置各自持有一個串流解碼器，不再把整首音檔解碼成 Vec<f32>。
/// 所有裝置共用啟動閘門、跳轉命令與 EQ 參數；濾波器狀態則各自獨立。
type StreamingDecoder = Box<dyn Source<Item = f32> + Send>;

fn open_audio_decoder(path: &Path) -> anyhow::Result<Decoder<BufReader<File>>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    // 較大緩衝，利於整片 CD 轉成的大型 FLAC
    let open_reader = || -> anyhow::Result<BufReader<File>> {
        let file = File::open(path)?;
        Ok(BufReader::with_capacity(256 * 1024, file))
    };

    match ext.as_deref() {
        Some("flac") => {
            // 先試 FLAC 專用，失敗再自動偵測（相容不同後端）
            match Decoder::new_flac(open_reader()?) {
                Ok(d) => Ok(d),
                Err(e1) => Decoder::new(open_reader()?)
                    .map_err(|e2| anyhow::anyhow!("FLAC 解碼失敗: {e1}；自動偵測亦失敗: {e2}")),
            }
        }
        Some("mp3") => {
            match Decoder::new_mp3(open_reader()?) {
                Ok(d) => Ok(d),
                Err(e1) => Decoder::new(open_reader()?)
                    .map_err(|e2| anyhow::anyhow!("MP3 解碼失敗: {e1}；自動偵測亦失敗: {e2}")),
            }
        }
        _ => Decoder::new(open_reader()?).map_err(|e| anyhow::anyhow!("音訊解碼失敗: {e}")),
    }
}

fn open_stream_decoder(path: &Path, seek_to: Duration) -> anyhow::Result<StreamingDecoder> {
    let mut decoder = open_audio_decoder(path)?;

    // 優先 try_seek（FLAC/Symphonia 可快速定位）；失敗才用 skip_duration（會解碼略過，長檔很慢）
    if !seek_to.is_zero() {
        match decoder.try_seek(seek_to) {
            Ok(()) => {
                let source = decoder.convert_samples::<f32>();
                return Ok(Box::new(source));
            }
            Err(_) => {
                // 後備：重新開檔並 skip（僅短距離可接受）
                decoder = open_audio_decoder(path)?;
                let source = decoder.convert_samples::<f32>().skip_duration(seek_to);
                return Ok(Box::new(source));
            }
        }
    }

    Ok(Box::new(decoder.convert_samples::<f32>()))
}

struct GatedSource {
    path: Arc<PathBuf>,
    inner: StreamingDecoder,
    start_gate: Arc<AtomicBool>,
    seek_target: Arc<AtomicUsize>,
    seek_generation: Arc<AtomicU64>,
    seen_seek_generation: u64,
    eq_gains: Arc<[AtomicU32; EQ_BANDS]>,
    last_gains: [f32; EQ_BANDS],
    filters: Vec<Vec<Biquad>>, // [channel][band]
    channels: u16,
    sample_rate: u32,
    duration: Option<Duration>,
    cursor: usize,
    failed: bool,
}

impl GatedSource {
    fn new(
        path: Arc<PathBuf>, channels: u16, sample_rate: u32, duration: Option<Duration>,
        start_gate: Arc<AtomicBool>, seek_target: Arc<AtomicUsize>,
        seek_generation: Arc<AtomicU64>, eq_gains: Arc<[AtomicU32; EQ_BANDS]>,
    ) -> anyhow::Result<Self> {
        let inner = open_stream_decoder(path.as_path(), Duration::ZERO)?;
        let channel_count = channels.max(1) as usize;
        let seen_seek_generation = seek_generation.load(Ordering::Acquire);
        let mut source = Self {
            path, inner, start_gate, seek_target, seek_generation,
            seen_seek_generation, eq_gains, last_gains: [f32::NAN; EQ_BANDS],
            filters: vec![vec![Biquad::default(); EQ_BANDS]; channel_count],
            channels: channels.max(1), sample_rate, duration, cursor: 0, failed: false,
        };
        source.refresh_eq();
        Ok(source)
    }

    fn refresh_eq(&mut self) {
        for band in 0..EQ_BANDS {
            let gain = f32::from_bits(self.eq_gains[band].load(Ordering::Relaxed));
            if (gain - self.last_gains[band]).abs() > 0.001 || !self.last_gains[band].is_finite() {
                for channel_filters in &mut self.filters {
                    channel_filters[band].set_peaking(
                        self.sample_rate as f32, EQ_FREQUENCIES[band], 1.0, gain,
                    );
                }
                self.last_gains[band] = gain;
            }
        }
    }

    fn apply_pending_seek(&mut self) {
        let generation = self.seek_generation.load(Ordering::Acquire);
        if generation == self.seen_seek_generation { return; }

        let sample_index = self.seek_target.load(Ordering::Acquire);
        let seconds = sample_index as f64
            / self.sample_rate.max(1) as f64
            / self.channels.max(1) as f64;
        let target = Duration::from_secs_f64(seconds);

        // seek 只在使用者拖曳進度條時重開解碼器；正常播放不會反覆開檔。
        match open_stream_decoder(self.path.as_path(), target) {
            Ok(decoder) => {
                self.inner = decoder;
                self.cursor = sample_index;
                for channel_filters in &mut self.filters {
                    for filter in channel_filters { filter.z1 = 0.0; filter.z2 = 0.0; }
                }
                self.failed = false;
            }
            Err(error) => {
                eprintln!("音訊跳轉失敗：{error}");
                self.failed = true;
            }
        }
        self.seen_seek_generation = generation;
    }
}

impl Iterator for GatedSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed { return None; }
        if !self.start_gate.load(Ordering::Acquire) {
            // Sink 可先建立並預熱；閘門開啟前不消耗解碼器的音訊資料。
            return Some(0.0);
        }

        self.apply_pending_seek();
        if self.failed { return None; }

        let channel = self.cursor % self.channels as usize;
        if channel == 0 { self.refresh_eq(); }
        let mut sample = self.inner.next()?;
        for filter in &mut self.filters[channel] {
            sample = filter.process(sample);
        }
        self.cursor = self.cursor.saturating_add(1);
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if !self.start_gate.load(Ordering::Acquire) { (0, None) }
        else { self.inner.size_hint() }
    }
}

impl Source for GatedSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { self.duration }
}

struct App {
    selected_file: Option<PathBuf>,
    devices: Vec<String>,
    selected: Vec<bool>,
    device_volumes: Vec<f32>,
    is_playing: bool,
    status: String,
    volume: f32,
    playing: Vec<PlayingDevice>,
    sample_rate: u32,
    channels: u16,
    duration: Duration,
    position_offset: Duration,
    playback_started: Option<Instant>,
    start_gate: Arc<AtomicBool>,
    seek_target: Arc<AtomicUsize>,
    seek_generation: Arc<AtomicU64>,
    eq_shared: Arc<[AtomicU32; EQ_BANDS]>,
    eq_gains: [f32; EQ_BANDS],
    theme_index: usize,
}

impl Default for App {
    fn default() -> Self {
        let devices = list_devices();
        let selected = vec![false; devices.len()];
        let (volume, volume_status) = match get_windows_endpoint_volume() {
            Ok(value) => (value, "請選擇 MP3 或 FLAC 檔案，並勾選要輸出的裝置".to_string()),
            Err(error) => (0.85, format!("無法讀取 Windows 預設輸出音量（使用 85% 作為初始值）：{error}")),
        };
        Self {
            selected_file: None, device_volumes: vec![1.0; devices.len()], devices, selected, is_playing: false,
            status: volume_status,
            volume, playing: Vec::new(),
            sample_rate: 0, channels: 2, duration: Duration::ZERO,
            position_offset: Duration::ZERO, playback_started: None,
            start_gate: Arc::new(AtomicBool::new(false)),
            seek_target: Arc::new(AtomicUsize::new(0)),
            seek_generation: Arc::new(AtomicU64::new(0)),
            eq_shared: Arc::new(std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits()))),
            eq_gains: [0.0; EQ_BANDS], theme_index: 0,
        }
    }
}

fn list_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.output_devices() { Ok(devices) => devices.filter_map(|d| d.name().ok()).collect(), Err(_) => Vec::new() }
}

fn find_device_by_name(name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let devices = host.output_devices().ok()?;
    for device in devices {
        if let Ok(n) = device.name() { if n == name { return Some(device); } }
    }
    None
}

fn inspect_audio(path: &Path) -> anyhow::Result<(Option<Duration>, u32, u16)> {
    // 僅讀取解碼器資訊，不收集整首音訊樣本，因此選曲後不必等待完整解碼。
    let decoder = open_audio_decoder(path)?;
    let sample_rate = decoder.sample_rate();
    let channels = decoder.channels();
    let duration = decoder.total_duration();
    if sample_rate == 0 || channels == 0 {
        return Err(anyhow::anyhow!("音檔參數異常（sample_rate={sample_rate}, channels={channels}）"));
    }
    Ok((duration, sample_rate, channels))
}

fn format_time(time: Duration) -> String {
    let seconds = time.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn setup_chinese_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_paths = [
        "C:/Windows/Fonts/msjh.ttc", "C:/Windows/Fonts/msjhbd.ttc",
        "C:/Windows/Fonts/mingliu.ttc", "C:/Windows/Fonts/kaiu.ttf",
        "/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc", "/Library/Fonts/Microsoft/Microsoft JhengHei.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    let mut loaded = false;
    for path in font_paths {
        if Path::new(path).exists() {
            if let Ok(data) = std::fs::read(path) {
                fonts.font_data.insert("chinese_font".to_owned(), egui::FontData::from_owned(data));
                fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "chinese_font".to_owned());
                fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "chinese_font".to_owned());
                loaded = true;
                eprintln!("已載入中文字型: {}", path);
                break;
            }
        }
    }
    if !loaded { eprintln!("警告：找不到支援繁體中文的系統字型，中文可能顯示為方塊。"); }
    ctx.set_fonts(fonts);
}

impl App {
    fn current_position(&self) -> Duration {
        let position = match self.playback_started {
            Some(started) if self.is_playing => self.position_offset.saturating_add(started.elapsed()),
            _ => self.position_offset,
        };
        position.min(self.duration)
    }

    fn seek_to(&mut self, position: Duration) {
        let position = position.min(self.duration);
        self.position_offset = position;
        self.playback_started = if self.is_playing { Some(Instant::now()) } else { None };
        let sample_index = (position.as_secs_f64() * self.sample_rate as f64 * self.channels as f64) as usize;
        self.seek_target.store(sample_index, Ordering::Release);
        self.seek_generation.fetch_add(1, Ordering::AcqRel);
    }

    fn publish_eq(&self) {
        for band in 0..EQ_BANDS {
            self.eq_shared[band].store(self.eq_gains[band].to_bits(), Ordering::Relaxed);
        }
    }

    fn start_playback(&mut self) {
        let path = match &self.selected_file { Some(p) => p.clone(), None => return };
        let selected_devices: Vec<(String, f32)> = self.devices.iter()
            .zip(self.selected.iter())
            .zip(self.device_volumes.iter())
            .filter(|((_, &sel), _)| sel)
            .map(|((name, _), &volume)| (name.clone(), volume))
            .collect();
        if selected_devices.is_empty() { self.status = "請至少選擇一個輸出裝置".to_string(); return; }

        self.status = "正在開啟音訊串流…".to_string();
        let (duration, sample_rate, channels) = match inspect_audio(&path) {
            Ok(v) => v,
            Err(e) => { self.status = format!("無法讀取音檔：{}", e); return; }
        };
        if sample_rate == 0 || channels == 0 {
            self.status = "音訊格式無效".to_string();
            return;
        }

        self.duration = duration.unwrap_or(Duration::ZERO);
        self.sample_rate = sample_rate;
        self.channels = channels;
        self.position_offset = Duration::ZERO;
        self.playback_started = None;
        self.seek_target.store(0, Ordering::Release);
        self.seek_generation.fetch_add(1, Ordering::AcqRel);
        self.start_gate.store(false, Ordering::Release);
        self.publish_eq();

        let shared_path = Arc::new(path);
        let mut playing_list = Vec::new();
        let mut error_msgs = Vec::new();
        for (name, device_volume) in &selected_devices {
            match find_device_by_name(name) {
                Some(device) => match OutputStream::try_from_device(&device) {
                    Ok((stream, handle)) => match Sink::try_new(&handle) {
                        Ok(sink) => {
                            sink.set_volume(*device_volume);
                            match GatedSource::new(
                                Arc::clone(&shared_path), channels, sample_rate, duration,
                                Arc::clone(&self.start_gate), Arc::clone(&self.seek_target),
                                Arc::clone(&self.seek_generation), Arc::clone(&self.eq_shared),
                            ) {
                                Ok(source) => {
                                    sink.append(source);
                                    playing_list.push(PlayingDevice { name: name.clone(), _stream: stream, sink, volume: *device_volume });
                                }
                                Err(e) => error_msgs.push(format!("{}：MP3 串流初始化失敗 ({})", name, e)),
                            }
                        }
                        Err(e) => error_msgs.push(format!("{}：Sink 失敗 ({})", name, e)),
                    },
                    Err(e) => error_msgs.push(format!("{}：無法開啟 ({})", name, e)),
                },
                None => error_msgs.push(format!("{}：找不到裝置", name)),
            }
        }

        let success_count = playing_list.len();
        if success_count == 0 {
            self.status = format!("全部裝置開啟失敗：{}", error_msgs.join("；"));
            return;
        }

        self.playing = playing_list;
        self.is_playing = true;
        self.playback_started = Some(Instant::now());
        // 所有 Sink 和解碼器都建立後，才一次開啟共同播放閘門。
        self.start_gate.store(true, Ordering::Release);
        self.status = if error_msgs.is_empty() {
            format!("串流播放中：{} 個裝置", success_count)
        } else {
            format!("串流播放中：{} 個裝置（部分失敗：{}）", success_count, error_msgs.join("；"))
        };
    }

    fn stop_playback(&mut self) {
        for device in &self.playing { device.sink.stop(); }
        self.playing.clear();
        self.is_playing = false;
        self.playback_started = None;
        self.position_offset = Duration::ZERO;
        self.start_gate.store(false, Ordering::Release);
        self.status = "已停止播放".to_string();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let palette = theme_palette(self.theme_index);
        apply_theme(ctx, self.theme_index);
        if self.is_playing {
            let any_still = self.playing.iter().any(|d| !d.sink.empty());
            if !any_still {
                self.playing.clear();
                self.is_playing = false;
                self.position_offset = self.duration;
                self.playback_started = None;
                self.status = "播放結束".to_string();
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none()
                .fill(palette.bg)
                .inner_margin(egui::Margin::same(18.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let full_width = ui.available_width();

                        // Header
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("♫").size(32.0).color(palette.accent));
                            ui.vertical(|ui| {
                                ui.label(RichText::new("AUDIO PLAYER").size(10.0).strong().color(palette.accent));
                                ui.label(RichText::new("MP3 / FLAC 多裝置播放器").size(23.0).strong().color(palette.text));
                                ui.label(RichText::new("同步輸出 · 精準定位 · 10 段圖形等化器").size(12.0).color(palette.secondary));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (dot_color, state) = if self.is_playing {
                                    (palette.success, "PLAYING")
                                } else { (palette.muted, "READY") };
                                ui.label(RichText::new(state).size(10.0).strong().color(dot_color));
                                ui.label(RichText::new("●").size(12.0).color(dot_color));
                                ui.add_space(10.0);
                                egui::ComboBox::from_id_salt("player_theme_selector")
                                    .selected_text(format!("🎨 {}", THEME_NAMES[self.theme_index]))
                                    .width(112.0)
                                    .show_ui(ui, |ui| {
                                        for (index, name) in THEME_NAMES.iter().enumerate() {
                                            ui.selectable_value(&mut self.theme_index, index, *name);
                                        }
                                    });
                            });
                        });
                        ui.add_space(14.0);

                        // File selection card
                        egui::Frame::none().fill(palette.card)
                            .stroke(egui::Stroke::new(1.0_f32, palette.border))
                            .rounding(egui::Rounding::same(10.0)).inner_margin(egui::Margin::same(13.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("01  音樂來源").size(14.0).strong().color(palette.text));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add(egui::Button::new(RichText::new("選擇音檔…").strong())
                                            .fill(palette.accent_active)
                                            .rounding(egui::Rounding::same(6.0))).clicked() {
                                            if let Some(path) = rfd::FileDialog::new().add_filter("音訊檔案", &["mp3", "flac"])
                                                .add_filter("MP3", &["mp3"])
                                                .add_filter("FLAC", &["flac"])
                                                .pick_file() {
                                                self.stop_playback();
                                                self.selected_file = Some(path.clone());
                                                self.duration = Duration::ZERO;
                                                self.position_offset = Duration::ZERO;
                                                self.status = format!("已選擇：{}", path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                                            }
                                        }
                                    });
                                });
                                ui.add_space(7.0);
                                let file_label = self.selected_file.as_ref()
                                    .and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "尚未選擇音樂檔案".to_string());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("♫").size(18.0).color(palette.accent));
                                    ui.label(RichText::new(file_label).size(12.0).color(palette.secondary));
                                });
                            });

                        ui.add_space(10.0);

                        // Full-width playback timeline
                        egui::Frame::none().fill(palette.card)
                            .stroke(egui::Stroke::new(1.0_f32, palette.border))
                            .rounding(egui::Rounding::same(10.0)).inner_margin(egui::Margin::same(14.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("02  播放進度").size(14.0).strong().color(palette.text));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(if self.is_playing { "● 即時播放" } else { "● 待命" })
                                            .size(10.5).color(if self.is_playing { palette.success } else { palette.muted }));
                                    });
                                });
                                ui.add_space(11.0);
                                let mut seek_seconds = self.current_position().as_secs_f64() as f32;
                                let total_seconds = self.duration.as_secs_f64() as f32;
                                let elapsed_text = format_time(Duration::from_secs_f32(seek_seconds.max(0.0)));
                                let total_text = format_time(self.duration);
                                ui.horizontal(|ui| {
                                    ui.allocate_ui_with_layout(Vec2::new(48.0, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(elapsed_text).monospace().size(12.0).strong().color(palette.accent));
                                    });
                                    let slider_width = (ui.available_width() - 52.0).max(80.0);
                                    let old_slider_width = ui.spacing().slider_width;
                                    ui.spacing_mut().slider_width = slider_width;
                                    let response = ui.add_enabled(self.duration > Duration::ZERO,
                                        egui::Slider::new(&mut seek_seconds, 0.0..=total_seconds.max(0.01))
                                            .show_value(false).trailing_fill(true));
                                    ui.spacing_mut().slider_width = old_slider_width;
                                    ui.allocate_ui_with_layout(Vec2::new(48.0, 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(total_text).monospace().size(12.0).color(palette.secondary));
                                    });
                                    if response.changed() { self.seek_to(Duration::from_secs_f32(seek_seconds.max(0.0))); }
                                });
                                ui.add_space(2.0);
                                ui.label(RichText::new("拖曳進度條即可依秒定位；所有已啟用的輸出裝置會同步跳轉。")
                                    .size(10.5).color(palette.muted));
                            });

                        ui.add_space(10.0);

                        // Responsive 10-band equalizer
                        egui::Frame::none().fill(palette.card)
                            .stroke(egui::Stroke::new(1.0_f32, palette.border))
                            .rounding(egui::Rounding::same(10.0)).inner_margin(egui::Margin::same(13.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("03  音效等化器").size(14.0).strong().color(palette.text));
                                    ui.label(RichText::new("10-BAND PEQ").size(9.5).color(palette.accent));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("重設 EQ").size(10.5)).clicked() {
                                            self.eq_gains = [0.0; EQ_BANDS];
                                            self.publish_eq();
                                        }
                                    });
                                });
                                ui.add_space(5.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("主音量").size(11.0).color(palette.secondary));
                                    ui.add_space(5.0);
                                    // 音量滑桿使用扣除標籤與百分比後的全部可用寬度。
                                    let value_width = 44.0;
                                    let slider_width = (ui.available_width() - value_width - ui.spacing().item_spacing.x).max(40.0);
                                    let response = ui.add_sized(
                                        Vec2::new(slider_width, 20.0),
                                        egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                            .show_value(false).step_by(0.01),
                                    );
                                    ui.label(RichText::new(format!("{:>3.0}%", self.volume * 100.0))
                                        .monospace().size(10.5).color(palette.accent));
                                    if response.changed() {
                                        match set_windows_endpoint_volume(self.volume) {
                                            Ok(()) => {
                                                self.status = format!("Windows 預設播放裝置音量已設為 {:.0}%", self.volume * 100.0);
                                            }
                                            Err(error) => {
                                                self.status = format!("設定 Windows 系統音量失敗：{error}");
                                            }
                                        }
                                    }
                                });
                                ui.label(RichText::new("主音量直接控制 Windows 預設播放裝置；100% = Windows 端點最大音量。此設定也會影響其他使用該裝置的程式。")
                                    .size(9.5).color(palette.muted));
                                ui.add_space(9.0);
                                let eq_width = ui.available_width();
                                let column_gap = 3.0;
                                let column_width = ((eq_width - column_gap * (EQ_BANDS as f32 - 1.0)) / EQ_BANDS as f32).max(24.0);
                                let slider_height = (eq_width * 0.205).clamp(92.0, 168.0);
                                let mut eq_changed = false;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = column_gap;
                                    for band in 0..EQ_BANDS {
                                        ui.allocate_ui_with_layout(Vec2::new(column_width, slider_height + 48.0),
                                            egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.set_width(column_width);
                                                ui.label(RichText::new(format!("{}", EQ_FREQUENCIES[band] as u32)).size(9.5).color(palette.secondary));
                                                let slider = egui::Slider::new(&mut self.eq_gains[band], EQ_MIN_DB..=EQ_MAX_DB)
                                                    .vertical().show_value(false).step_by(0.5);
                                                if ui.add_sized([column_width.min(34.0), slider_height], slider).changed() { eq_changed = true; }
                                                ui.label(RichText::new(format!("{:+.1}", self.eq_gains[band])).size(9.0).monospace()
                                                    .color(if self.eq_gains[band] > 0.0 { palette.success }
                                                        else if self.eq_gains[band] < 0.0 { palette.danger }
                                                        else { palette.secondary }));
                                            });
                                    }
                                });
                                if eq_changed { self.publish_eq(); }
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Hz").size(9.0).color(palette.muted));
                                    ui.label(RichText::new("低頻").size(9.5).color(palette.muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("高頻　 ·　 -12 dB 弱化　 /　 0 dB 原音　 /　 +12 dB 強化")
                                            .size(9.5).color(palette.muted));
                                    });
                                });
                            });

                        ui.add_space(10.0);

                        // Output devices
                        egui::Frame::none().fill(palette.card)
                            .stroke(egui::Stroke::new(1.0_f32, palette.border))
                            .rounding(egui::Rounding::same(10.0)).inner_margin(egui::Margin::same(13.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("04  輸出裝置").size(14.0).strong().color(palette.text));
                                    ui.label(RichText::new(format!("{} 個可用裝置", self.devices.len())).size(10.5).color(palette.muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("↻ 重新整理").size(10.5)).clicked() {
                                            let old_volumes: Vec<(String, f32)> = self.devices.iter()
                                                .cloned().zip(self.device_volumes.iter().copied()).collect();
                                            self.devices = list_devices();
                                            self.device_volumes = self.devices.iter().map(|name| {
                                                old_volumes.iter().find(|(old_name, _)| old_name == name)
                                                    .map(|(_, volume)| *volume).unwrap_or(1.0)
                                            }).collect();
                                            self.selected = vec![false; self.devices.len()];
                                            self.status = format!("已重新整理，共 {} 個裝置", self.devices.len());
                                        }
                                    });
                                });
                                ui.add_space(5.0);
                                let mut changed_device_volumes: Vec<(String, f32)> = Vec::new();
                                egui::ScrollArea::vertical().max_height(250.0).auto_shrink([false, false]).show(ui, |ui| {
                                    if self.devices.is_empty() {
                                        ui.label(RichText::new("沒有找到音訊輸出裝置").color(palette.muted));
                                    } else {
                                        for (i, name) in self.devices.iter().enumerate() {
                                            let is_bt = name.to_lowercase().contains("bluetooth") || name.contains("藍牙")
                                                || name.to_lowercase().contains("headset") || name.to_lowercase().contains("headphone");
                                            ui.horizontal(|ui| {
                                                ui.checkbox(&mut self.selected[i], "");
                                                let color = if is_bt { palette.accent } else { palette.device };
                                                ui.label(RichText::new(name).size(11.5).color(color));
                                            });
                                            ui.horizontal(|ui| {
                                                ui.add_space(25.0);
                                                ui.label(RichText::new("輸出音量").size(9.5).color(palette.muted));
                                                let percent_width = 42.0;
                                                let slider_width = (ui.available_width() - percent_width - ui.spacing().item_spacing.x).max(40.0);
                                                let response = ui.add_sized(
                                                    Vec2::new(slider_width, 17.0),
                                                    egui::Slider::new(&mut self.device_volumes[i], 0.0..=1.0)
                                                        .show_value(false).step_by(0.01),
                                                );
                                                ui.label(RichText::new(format!("{:>3.0}%", self.device_volumes[i] * 100.0))
                                                    .monospace().size(9.5).color(palette.secondary));
                                                if response.changed() {
                                                    changed_device_volumes.push((name.clone(), self.device_volumes[i]));
                                                }
                                            });
                                            ui.add_space(5.0);
                                        }
                                    }
                                });
                                // 將各裝置的獨立音量即時套用到正在播放的 Sink。
                                for (name, volume) in changed_device_volumes {
                                    if let Some(device) = self.playing.iter_mut().find(|device| device.name == name) {
                                        device.volume = volume;
                                        device.sink.set_volume(volume);
                                    }
                                }
                            });

                        ui.add_space(12.0);
                        // Transport controls
                        ui.horizontal(|ui| {
                            let can_play = self.selected_file.is_some() && self.selected.iter().any(|&s| s) && !self.is_playing;
                            let play = egui::Button::new(RichText::new("▶  開始播放").size(13.0).strong().color(Color32::WHITE))
                                .fill(palette.accent_active).min_size(Vec2::new(142.0, 40.0))
                                .rounding(egui::Rounding::same(7.0));
                            if ui.add_enabled(can_play, play).clicked() { self.start_playback(); }
                            let stop = egui::Button::new(RichText::new("■  停止播放").size(13.0).strong())
                                .fill(palette.stop_button).min_size(Vec2::new(116.0, 40.0))
                                .rounding(egui::Rounding::same(7.0));
                            if ui.add_enabled(self.is_playing, stop).clicked() { self.stop_playback(); }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(if self.is_playing { "● 正在播放" } else { "● 已停止" }).size(11.0)
                                    .color(if self.is_playing { palette.success } else { palette.muted }));
                            });
                        });
                        ui.add_space(9.0);
                        egui::Frame::none().fill(palette.status_bg)
                            .rounding(egui::Rounding::same(6.0)).inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.set_width(full_width - 20.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new("狀態").size(10.5).strong().color(palette.muted));
                                    ui.label(RichText::new(&self.status).size(10.5).color(palette.secondary));
                                });
                            });
                    });
            });

        if self.is_playing { ctx.request_repaint_after(Duration::from_millis(80)); }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 820.0])
            .with_min_inner_size([560.0, 520.0])
            .with_title("MP3 / FLAC 多裝置播放器"),
        ..Default::default()
    };
    eframe::run_native("MP3 / FLAC 多裝置播放器", options, Box::new(|cc| {
        setup_chinese_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, 0);
        Ok(Box::new(App::default()))
    }))
}
