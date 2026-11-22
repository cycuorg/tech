//! Audio CD → 單一 FLAC（無損）
//!
//! - 讀取整片 CD 並串接
//! - 可手動填寫曲目資訊 / 封面
//! - 可從 MusicBrainz + Cover Art Archive 自動查詢專輯、藝人、出版商、年份、曲目列表與封面

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use cd_da_reader::{CdReader, Toc};
use eframe::egui;
use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use flacenc::source::MemSource;
use sha1::{Digest, Sha1};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

fn load_chinese_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\msjh.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/PingFang.ttc",
    ];
    let mut fonts = egui::FontDefinitions::default();
    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert("chinese".to_owned(), egui::FontData::from_owned(data).into());
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
            eprintln!("已載入中文字型: {path}");
            break;
        }
    }
    ctx.set_fonts(fonts);
}

// ---------- MusicBrainz Disc ID ----------

fn musicbrainz_disc_id(toc: &Toc) -> Result<String> {
    // MusicBrainz Disc ID algorithm:
    // https://musicbrainz.org/doc/Disc_ID_Calculation
    // Uses first/last track numbers, lead-out, and per-track offsets.
    // All offsets are in "frames" = LBA + 150 (MSF includes 2s pre-gap).
    //
    // cd-da-reader provides exact leadout_lba and first_track/last_track.

    let first = toc.first_track;
    let last = toc.last_track;
    if first == 0 || last == 0 || first > last {
        return Err(anyhow!("TOC 音軌範圍無效 (first={}, last={})", first, last));
    }

    // Build map track_number -> start_lba for ALL tracks in TOC
    // (MusicBrainz uses disc track numbers, including gaps as 0 if missing)
    let mut offsets: [u32; 100] = [0; 100];
    for t in &toc.tracks {
        if t.number >= 1 && t.number <= 99 {
            offsets[t.number as usize] = t.start_lba.saturating_add(150);
        }
    }

    let leadout_frames = toc.leadout_lba.saturating_add(150);

    // SHA-1 over uppercase hex ASCII:
    // first(2) + last(2) + leadout(8) + offsets[1..=99] each 8 hex digits
    let mut s = format!("{:02X}{:02X}{:08X}", first, last, leadout_frames);
    for n in 1u8..=99 {
        s.push_str(&format!("{:08X}", offsets[n as usize]));
    }

    let mut hasher = Sha1::new();
    hasher.update(s.as_bytes());
    let hash = hasher.finalize();
    let b64 = general_purpose::STANDARD.encode(hash);
    // MusicBrainz uses URL-safe variant with specific substitutions
    Ok(b64.replace('+', ".").replace('/', "_").replace('=', "-"))
}

// ---------- App metadata / worker messages ----------

#[derive(Clone, Default)]
struct TrackMeta {
    title: String,
    artist: String,
    album: String,
    year: String,
    genre: String,
    comment: String,
    publisher: String,
    cover_path: Option<PathBuf>,
    cover_bytes: Option<Vec<u8>>,
    mb_release_id: Option<String>,
}

enum WorkerMsg {
    Progress {
        current: usize,
        total: usize,
        msg: String,
    },
    LookupDone(TrackMeta),
    Done {
        path: PathBuf,
    },
    Error(String),
}

fn mb_client() -> Result<reqwest::blocking::Client> {
    // MusicBrainz 要求可識別的 User-Agent，並限制約 1 req/sec
    reqwest::blocking::Client::builder()
        .user_agent("cd_to_flac/0.2.1 (CD ripper; contact: local-user)")
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .context("建立 HTTP 客戶端失敗")
}

fn http_get_json(client: &reqwest::blocking::Client, url: &str) -> Result<serde_json::Value> {
    let mut last_err = None;
    for attempt in 1..=5 {
        let resp = match client.get(url).send() {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(anyhow!("連線失敗: {e}"));
                std::thread::sleep(std::time::Duration::from_millis(800 * attempt));
                continue;
            }
        };
        let status = resp.status();
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status.is_server_error()
        {
            last_err = Some(anyhow!("伺服器忙碌 ({status})，重試中…"));
            // 指數退避，遵守 MusicBrainz 限流
            std::thread::sleep(std::time::Duration::from_millis(1000 * attempt as u64));
            continue;
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(anyhow!("NOT_FOUND"));
        }
        if !status.is_success() {
            return Err(anyhow!("HTTP {status} for {url}"));
        }
        let v: serde_json::Value = resp.json().context("解析 JSON 失敗")?;
        // 禮貌限流：成功後略等
        std::thread::sleep(std::time::Duration::from_millis(1100));
        return Ok(v);
    }
    Err(last_err.unwrap_or_else(|| anyhow!("MusicBrainz 多次重試仍失敗")))
}

fn release_to_meta(release: &serde_json::Value, client: &reqwest::blocking::Client) -> TrackMeta {
    let title = release
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let id = release
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let artist = release
        .get("artist-credit")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .map(|ac| {
            ac.get("name")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    ac.get("artist")
                        .and_then(|a| a.get("name"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();

    let year = release
        .get("date")
        .and_then(|v| v.as_str())
        .map(|d| d.chars().take(4).collect::<String>())
        .unwrap_or_default();

    let publisher = release
        .get("label-info")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|li| li.get("label"))
        .and_then(|l| l.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut track_lines = Vec::new();
    if let Some(media) = release.get("media").and_then(|v| v.as_array()) {
        for m in media {
            if let Some(tracks) = m.get("tracks").and_then(|v| v.as_array()) {
                for t in tracks {
                    let num = t.get("number").and_then(|v| v.as_str()).unwrap_or("");
                    let ttitle = t
                        .get("title")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            t.get("recording")
                                .and_then(|r| r.get("title"))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("");
                    if !ttitle.is_empty() {
                        track_lines.push(format!("{num}. {ttitle}"));
                    }
                }
            }
        }
    }
    let comment = if track_lines.is_empty() {
        String::new()
    } else {
        format!("曲目列表:\n{}", track_lines.join("\n"))
    };

    let mut meta = TrackMeta {
        title: title.clone(),
        artist,
        album: title,
        year,
        genre: String::new(),
        comment,
        publisher,
        cover_path: None,
        cover_bytes: None,
        mb_release_id: if id.is_empty() { None } else { Some(id.clone()) },
    };

    if !id.is_empty() {
        let cover_url = format!("https://coverartarchive.org/release/{id}/front-500");
        // Cover Art Archive 也可能 503，簡單重試
        for attempt in 1..=3 {
            match client.get(&cover_url).send() {
                Ok(r) if r.status().is_success() => {
                    if let Ok(bytes) = r.bytes() {
                        if !bytes.is_empty() {
                            meta.cover_bytes = Some(bytes.to_vec());
                        }
                    }
                    break;
                }
                _ => {
                    std::thread::sleep(std::time::Duration::from_millis(600 * attempt));
                }
            }
        }
    }

    meta
}

/// 依 Disc ID 查詢；失敗時可改用目錄編號 / 關鍵字
fn fetch_musicbrainz(disc_id: &str, catalog_hint: Option<&str>) -> Result<TrackMeta> {
    let client = mb_client()?;

    // 1) Disc ID 查詢
    let disc_url = format!(
        "https://musicbrainz.org/ws/2/discid/{disc_id}?inc=artists+labels+recordings+artist-credits&fmt=json"
    );
    match http_get_json(&client, &disc_url) {
        Ok(v) => {
            if let Some(arr) = v.get("releases").and_then(|r| r.as_array()) {
                if let Some(rel) = arr.first() {
                    return Ok(release_to_meta(rel, &client));
                }
            }
        }
        Err(e) if e.to_string().contains("NOT_FOUND") => {
            // 繼續走備援
        }
        Err(e) => {
            // 503 等：若有目錄編號仍嘗試備援，否則回傳錯誤
            if catalog_hint.is_none() {
                return Err(e);
            }
        }
    }

    // 2) 目錄編號備援（例如 EUR 253017 / 253017）
    if let Some(cat) = catalog_hint {
        let cat_clean = cat
            .replace("EUR", "")
            .replace("eur", "")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>();
        if !cat_clean.is_empty() {
            let q = urlencoding_minimal(&format!("catno:{cat_clean}"));
            let search_url = format!(
                "https://musicbrainz.org/ws/2/release?query={q}&fmt=json&limit=5"
            );
            if let Ok(v) = http_get_json(&client, &search_url) {
                if let Some(arr) = v.get("releases").and_then(|r| r.as_array()) {
                    if let Some(rel) = arr.first() {
                        // 搜尋結果可能沒有完整 media/labels，再依 id 拉一次詳情
                        if let Some(rid) = rel.get("id").and_then(|x| x.as_str()) {
                            let detail_url = format!(
                                "https://musicbrainz.org/ws/2/release/{rid}?inc=artists+labels+recordings+artist-credits&fmt=json"
                            );
                            if let Ok(detail) = http_get_json(&client, &detail_url) {
                                return Ok(release_to_meta(&detail, &client));
                            }
                        }
                        return Ok(release_to_meta(rel, &client));
                    }
                }
            }
        }
    }

    Err(anyhow!(
        "MusicBrainz 查無此片或服務暫時不可用。\n\
         Disc ID: {disc_id}\n\
         可稍後再試，或手動填寫曲目資訊。\n\
         若有目錄編號可填在「目錄編號」欄後再查詢。"
    ))
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}


/// 將 VORBIS_COMMENT / PICTURE 直接插入 FLAC metadata 區（不依賴 lofty 寫入）
fn embed_metadata(path: &PathBuf, meta: &TrackMeta) -> Result<()> {
    let data = std::fs::read(path)
        .with_context(|| format!("讀取 FLAC 失敗: {}", path.display()))?;

    if data.len() < 8 || &data[0..4] != b"fLaC" {
        return Err(anyhow!("不是有效的 FLAC 檔案"));
    }

    // 解析既有 metadata blocks，找出音訊起點
    let mut pos = 4usize;
    let mut blocks: Vec<(u8, Vec<u8>)> = Vec::new(); // (type, body) 不含 header
    let mut last_seen = false;
    while pos + 4 <= data.len() && !last_seen {
        let header0 = data[pos];
        let is_last = (header0 & 0x80) != 0;
        let block_type = header0 & 0x7F;
        let length = ((data[pos + 1] as usize) << 16)
            | ((data[pos + 2] as usize) << 8)
            | (data[pos + 3] as usize);
        pos += 4;
        if pos + length > data.len() {
            return Err(anyhow!("FLAC metadata 區塊長度異常"));
        }
        let body = data[pos..pos + length].to_vec();
        pos += length;
        // 略過舊的 VORBIS_COMMENT(4) 與 PICTURE(6)，稍後重建
        if block_type != 4 && block_type != 6 {
            blocks.push((block_type, body));
        }
        last_seen = is_last;
    }
    let audio_start = pos;
    let audio = &data[audio_start..];

    // --- 組 VORBIS_COMMENT body ---
    let mut comments: Vec<(String, String)> = Vec::new();
    if !meta.title.trim().is_empty() {
        comments.push(("TITLE".into(), meta.title.trim().into()));
    }
    if !meta.artist.trim().is_empty() {
        comments.push(("ARTIST".into(), meta.artist.trim().into()));
    }
    if !meta.album.trim().is_empty() {
        comments.push(("ALBUM".into(), meta.album.trim().into()));
    }
    if !meta.year.trim().is_empty() {
        comments.push(("DATE".into(), meta.year.trim().into()));
    }
    if !meta.genre.trim().is_empty() {
        comments.push(("GENRE".into(), meta.genre.trim().into()));
    }
    if !meta.publisher.trim().is_empty() {
        comments.push(("LABEL".into(), meta.publisher.trim().into()));
        comments.push(("ORGANIZATION".into(), meta.publisher.trim().into()));
    }
    if !meta.comment.trim().is_empty() {
        comments.push(("DESCRIPTION".into(), meta.comment.trim().into()));
        comments.push(("COMMENT".into(), meta.comment.trim().into()));
    }

    let vendor = "cd_to_flac 0.2.2";
    let mut vorbis_body = Vec::new();
    let vb = vendor.as_bytes();
    vorbis_body.extend_from_slice(&(vb.len() as u32).to_le_bytes());
    vorbis_body.extend_from_slice(vb);
    vorbis_body.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for (k, v) in &comments {
        let entry = format!("{k}={v}");
        let eb = entry.as_bytes();
        vorbis_body.extend_from_slice(&(eb.len() as u32).to_le_bytes());
        vorbis_body.extend_from_slice(eb);
    }

    // --- 組 PICTURE body（若有封面）---
    let picture_body: Option<Vec<u8>> = {
        let pic_data: Option<(Vec<u8>, &str)> = if let Some(cover) = &meta.cover_path {
            let d = std::fs::read(cover)
                .with_context(|| format!("讀取封面失敗: {}", cover.display()))?;
            let mime = match cover
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .as_deref()
            {
                Some("png") => "image/png",
                Some("gif") => "image/gif",
                Some("bmp") => "image/bmp",
                _ => "image/jpeg",
            };
            Some((d, mime))
        } else if let Some(d) = &meta.cover_bytes {
            let mime = if d.len() >= 8 && d.starts_with(&[0x89, b'P', b'N', b'G']) {
                "image/png"
            } else {
                "image/jpeg"
            };
            Some((d.clone(), mime))
        } else {
            None
        };

        if let Some((img, mime)) = pic_data {
            let mut body = Vec::new();
            // picture type 3 = Front Cover (BE)
            body.extend_from_slice(&3u32.to_be_bytes());
            let mb = mime.as_bytes();
            body.extend_from_slice(&(mb.len() as u32).to_be_bytes());
            body.extend_from_slice(mb);
            // description empty
            body.extend_from_slice(&0u32.to_be_bytes());
            // width, height, depth, colors — 0 表示未知（合法）
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&(img.len() as u32).to_be_bytes());
            body.extend_from_slice(&img);
            Some(body)
        } else {
            None
        }
    };

    if comments.is_empty() && picture_body.is_none() {
        return Ok(()); // 無事可寫
    }

    // 重建檔案：fLaC + blocks + vorbis + picture + audio
    // STREAMINFO 必須是第一個；確保 blocks 裡 type0 在最前
    blocks.sort_by(|a, b| {
        // type 0 first
        (a.0 != 0).cmp(&(b.0 != 0))
    });

    let mut out = Vec::with_capacity(data.len() + vorbis_body.len() + 1024);
    out.extend_from_slice(b"fLaC");

    let mut to_write: Vec<(u8, Vec<u8>)> = blocks;
    if !comments.is_empty() {
        to_write.push((4, vorbis_body));
    }
    if let Some(pb) = picture_body {
        to_write.push((6, pb));
    }

    let n = to_write.len();
    for (i, (ty, body)) in to_write.into_iter().enumerate() {
        let is_last = i + 1 == n;
        if body.len() > 0xFF_FFFF {
            return Err(anyhow!("metadata 區塊過大"));
        }
        let mut header0 = ty & 0x7F;
        if is_last {
            header0 |= 0x80;
        }
        out.push(header0);
        out.push(((body.len() >> 16) & 0xFF) as u8);
        out.push(((body.len() >> 8) & 0xFF) as u8);
        out.push((body.len() & 0xFF) as u8);
        out.extend_from_slice(&body);
    }
    out.extend_from_slice(audio);

    std::fs::write(path, &out)
        .with_context(|| format!("寫回 FLAC 失敗: {}", path.display()))?;

    // 簡易驗證：檔頭仍是 fLaC，且含有 type 4 或 6
    let verify = std::fs::read(path)?;
    if &verify[0..4] != b"fLaC" {
        return Err(anyhow!("寫回後檔頭損壞"));
    }
    let mut vp = 4usize;
    let mut found_comment = false;
    let mut found_pic = false;
    let mut last = false;
    while vp + 4 <= verify.len() && !last {
        let h = verify[vp];
        last = (h & 0x80) != 0;
        let ty = h & 0x7F;
        let len = ((verify[vp + 1] as usize) << 16)
            | ((verify[vp + 2] as usize) << 8)
            | (verify[vp + 3] as usize);
        vp += 4 + len;
        if ty == 4 {
            found_comment = true;
        }
        if ty == 6 {
            found_pic = true;
        }
    }
    if !comments.is_empty() && !found_comment {
        return Err(anyhow!("寫入後找不到 VORBIS_COMMENT 區塊"));
    }
    if meta.cover_bytes.is_some() || meta.cover_path.is_some() {
        if !found_pic {
            return Err(anyhow!("寫入後找不到 PICTURE 區塊"));
        }
    }

    eprintln!(
        "FLAC metadata 已寫入: vorbis_comment={} picture={} comments={}",
        found_comment,
        found_pic,
        comments.len()
    );
    Ok(())
}

fn convert_whole_cd_to_flac(
    output_path: PathBuf,
    meta: TrackMeta,
    tx: Sender<WorkerMsg>,
) -> Result<()> {
    let _ = tx.send(WorkerMsg::Progress {
        current: 0,
        total: 1,
        msg: "正在開啟光碟機…".into(),
    });

    let reader = CdReader::open_default().context("無法開啟光碟機，請確認已插入 Audio CD")?;
    let toc = reader.read_toc().context("讀取 TOC 失敗")?;
    let audio_tracks: Vec<_> = toc.tracks.iter().filter(|t| t.is_audio).collect();
    if audio_tracks.is_empty() {
        return Err(anyhow!("此光碟沒有音軌"));
    }
    let total_tracks = audio_tracks.len();

    let mut all_samples: Vec<i32> = Vec::new();
    for (idx, track) in audio_tracks.iter().enumerate() {
        let _ = tx.send(WorkerMsg::Progress {
            current: idx,
            total: total_tracks,
            msg: format!("正在讀取第 {} 軌（共 {} 軌）…", track.number, total_tracks),
        });
        let pcm = reader
            .read_track(&toc, track.number)
            .with_context(|| format!("讀取第 {} 軌失敗", track.number))?;
        if pcm.len() % 2 != 0 {
            return Err(anyhow!("第 {} 軌 PCM 長度異常", track.number));
        }
        for c in pcm.chunks_exact(2) {
            all_samples.push(i16::from_le_bytes([c[0], c[1]]) as i32);
        }
        let _ = tx.send(WorkerMsg::Progress {
            current: idx + 1,
            total: total_tracks,
            msg: format!("第 {} 軌完成", track.number),
        });
    }

    let _ = tx.send(WorkerMsg::Progress {
        current: total_tracks,
        total: total_tracks,
        msg: "正在編碼無損 FLAC…".into(),
    });

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| anyhow!("FLAC 設定錯誤: {:?}", e))?;
    let source = MemSource::from_samples(&all_samples, 2, 16, 44100);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| anyhow!("FLAC 編碼失敗: {e}"))?;
    let mut sink = ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| anyhow!("寫入 FLAC 失敗: {e}"))?;
    let data = sink.as_slice();
    if data.is_empty() {
        return Err(anyhow!("沒有產生 FLAC 資料"));
    }
    std::fs::write(&output_path, data)
        .with_context(|| format!("寫入檔案失敗: {}", output_path.display()))?;

    let has_meta = !meta.title.trim().is_empty()
        || !meta.artist.trim().is_empty()
        || !meta.album.trim().is_empty()
        || !meta.year.trim().is_empty()
        || !meta.genre.trim().is_empty()
        || !meta.comment.trim().is_empty()
        || !meta.publisher.trim().is_empty()
        || meta.cover_path.is_some()
        || meta.cover_bytes.is_some();

    if has_meta {
        let _ = tx.send(WorkerMsg::Progress {
            current: total_tracks,
            total: total_tracks,
            msg: "正在寫入曲目資訊與封面…".into(),
        });
        embed_metadata(&output_path, &meta)?;
    }

    let _ = tx.send(WorkerMsg::Done { path: output_path });
    Ok(())
}

fn lookup_from_cd(tx: Sender<WorkerMsg>, catalog_hint: Option<String>) {
    let result = (|| -> Result<TrackMeta> {
        let _ = tx.send(WorkerMsg::Progress {
            current: 0,
            total: 1,
            msg: "正在讀取 TOC 並計算 Disc ID…".into(),
        });
        let reader = CdReader::open_default().context("無法開啟光碟機")?;
        let toc = reader.read_toc().context("讀取 TOC 失敗")?;
        let disc_id = musicbrainz_disc_id(&toc)?;
        let _ = tx.send(WorkerMsg::Progress {
            current: 0,
            total: 1,
            msg: format!(
                "Disc ID: {disc_id}（音軌 {}–{}，lead-out LBA {}），查詢中…",
                toc.first_track, toc.last_track, toc.leadout_lba
            ),
        });
        let meta = fetch_musicbrainz(&disc_id, catalog_hint.as_deref())?;
        Ok(meta)
    })();

    match result {
        Ok(meta) => {
            let _ = tx.send(WorkerMsg::LookupDone(meta));
        }
        Err(e) => {
            let _ = tx.send(WorkerMsg::Error(format!("網路查詢失敗: {e:#}")));
        }
    }
}

struct App {
    status: String,
    progress: f32,
    is_working: bool,
    rx: Option<Receiver<WorkerMsg>>,
    last_output: Option<PathBuf>,
    meta: TrackMeta,
    /// 手動輸入的目錄編號，Disc ID 查不到時當備援
    catalog_hint: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            status: "可手動填寫資訊，或按「從網路查詢」自動取得專輯/封面".into(),
            progress: 0.0,
            is_working: false,
            rx: None,
            last_output: None,
            meta: TrackMeta::default(),
            catalog_hint: String::new(),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut messages = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }
        for msg in messages {
            match msg {
                WorkerMsg::Progress { current, total, msg } => {
                    self.progress = if total > 0 { current as f32 / total as f32 } else { 0.0 };
                    self.status = msg;
                }
                WorkerMsg::LookupDone(meta) => {
                    self.is_working = false;
                    self.meta = meta;
                    self.status = if self.meta.cover_bytes.is_some() {
                        "已從 MusicBrainz 取得專輯資訊與封面，可再修改後轉檔".into()
                    } else {
                        "已取得專輯資訊（未找到封面），可再修改後轉檔".into()
                    };
                    self.rx = None;
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
                    self.status = format!("錯誤：{e}");
                    self.rx = None;
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Audio CD → 單一 FLAC（無損）");
            ui.label("支援從 MusicBrainz 自動取得專輯、出版商、曲目列表與封面");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("目錄編號：");
                ui.add(
                    egui::TextEdit::singleline(&mut self.catalog_hint)
                        .desired_width(160.0)
                        .hint_text("例如 EUR 253017"),
                );
                if ui
                    .add_enabled(!self.is_working, egui::Button::new("從網路查詢專輯資訊"))
                    .clicked()
                {
                    self.is_working = true;
                    self.status = "開始查詢…".into();
                    let (tx, rx) = mpsc::channel();
                    self.rx = Some(rx);
                    let cat = if self.catalog_hint.trim().is_empty() {
                        None
                    } else {
                        Some(self.catalog_hint.trim().to_string())
                    };
                    thread::spawn(move || lookup_from_cd(tx, cat));
                }
                if self.meta.mb_release_id.is_some() {
                    ui.label(
                        egui::RichText::new("（已載入 MusicBrainz）")
                            .color(egui::Color32::from_rgb(0, 120, 0))
                            .size(12.0),
                    );
                }
            });

            ui.add_space(6.0);
            ui.group(|ui| {
                ui.label(egui::RichText::new("曲目 / 專輯資訊").strong());
                ui.horizontal(|ui| {
                    ui.label("標題：");
                    ui.add(egui::TextEdit::singleline(&mut self.meta.title).desired_width(300.0));
                });
                ui.horizontal(|ui| {
                    ui.label("藝人：");
                    ui.add(egui::TextEdit::singleline(&mut self.meta.artist).desired_width(300.0));
                });
                ui.horizontal(|ui| {
                    ui.label("專輯：");
                    ui.add(egui::TextEdit::singleline(&mut self.meta.album).desired_width(300.0));
                });
                ui.horizontal(|ui| {
                    ui.label("出版商：");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.meta.publisher).desired_width(280.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("年份：");
                    ui.add(egui::TextEdit::singleline(&mut self.meta.year).desired_width(80.0));
                    ui.label("  類型：");
                    ui.add(egui::TextEdit::singleline(&mut self.meta.genre).desired_width(140.0));
                });
                ui.label("備註 / 曲目列表：");
                ui.add(
                    egui::TextEdit::multiline(&mut self.meta.comment)
                        .desired_width(380.0)
                        .desired_rows(4),
                );

                ui.horizontal(|ui| {
                    ui.label("封面：");
                    let label = if self.meta.cover_bytes.is_some() {
                        "（已從網路下載）".to_string()
                    } else if let Some(p) = &self.meta.cover_path {
                        p.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| p.display().to_string())
                    } else {
                        "（尚未選擇）".into()
                    };
                    ui.label(label);
                    if ui.button("選擇圖片…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("圖片", &["jpg", "jpeg", "png", "gif", "bmp"])
                            .pick_file()
                        {
                            self.meta.cover_path = Some(path);
                            self.meta.cover_bytes = None;
                        }
                    }
                    if (self.meta.cover_path.is_some() || self.meta.cover_bytes.is_some())
                        && ui.button("清除").clicked()
                    {
                        self.meta.cover_path = None;
                        self.meta.cover_bytes = None;
                    }
                });
            });

            ui.add_space(10.0);
            let btn = egui::Button::new(egui::RichText::new("整片轉為FLAC").size(20.0))
                .min_size(egui::vec2(200.0, 44.0));
            if ui.add_enabled(!self.is_working, btn).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("cd_full.flac")
                    .add_filter("FLAC", &["flac"])
                    .save_file()
                {
                    self.is_working = true;
                    self.progress = 0.0;
                    self.status = "開始轉換…".into();
                    self.last_output = None;
                    let meta = self.meta.clone();
                    let (tx, rx) = mpsc::channel();
                    self.rx = Some(rx);
                    thread::spawn(move || {
                        if let Err(e) = convert_whole_cd_to_flac(path, meta, tx.clone()) {
                            let _ = tx.send(WorkerMsg::Error(format!("{e:#}")));
                        }
                    });
                }
            }

            ui.add_space(8.0);
            if self.is_working {
                ui.add(egui::ProgressBar::new(self.progress).show_percentage().desired_width(320.0));
            }
            ui.label(
                egui::RichText::new(&self.status).size(14.0).color(if self.status.starts_with("錯誤") {
                    egui::Color32::RED
                } else if self.status.starts_with("完成") || self.status.starts_with("已") {
                    egui::Color32::from_rgb(0, 140, 0)
                } else {
                    egui::Color32::DARK_GRAY
                }),
            );
            if let Some(path) = &self.last_output {
                ui.hyperlink_to(
                    "開啟檔案所在資料夾",
                    path.parent().unwrap_or(path).to_string_lossy(),
                );
            }
            ui.add_space(8.0);
            ui.separator();
            ui.label(
                egui::RichText::new(
                    "流程：插入 CD →「從網路查詢」→ 確認/修改資訊 →「整片轉為FLAC」。\n\
                     資料來源：MusicBrainz Disc ID + Cover Art Archive。需網路連線。",
                )
                .size(12.0)
                .color(egui::Color32::GRAY),
            );
        });

        if self.is_working {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([540.0, 640.0])
            .with_min_inner_size([480.0, 520.0])
            .with_title("整片 CD 轉 FLAC（MusicBrainz）"),
        ..Default::default()
    };
    eframe::run_native(
        "整片 CD 轉 FLAC（MusicBrainz）",
        options,
        Box::new(|cc| {
            load_chinese_fonts(&cc.egui_ctx);
            Ok(Box::new(App::default()))
        }),
    )
}
