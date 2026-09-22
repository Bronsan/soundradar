//! SoundRadar — game sound recognition assistant (native performance rewrite).
//! CLI-compatible with soundradar 0.6.x; hot path in Rust.
//!
//! Copyright (C) 2026 Bronsan
//! SPDX-License-Identifier: GPL-3.0-only

mod config;
mod dsp;
mod index;
mod library;
mod wav;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "soundradar",
    about = "soundradar - game sound recognition assistant",
    disable_version_flag = true
)]
struct Cli {
    /// 子命令；双击 exe 不带参数时自动进入 `app`（管理端 + 实时识别 + 覆盖层）
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// List active audio render (output) endpoints and mark the default.
    Devices,
    /// Capture speaker loopback and write a 16-bit PCM WAV file.
    Capture {
        #[arg(long, default_value_t = 5.0)]
        seconds: f64,
        #[arg(long, default_value = "test.wav")]
        out: PathBuf,
        #[arg(long, default_value = "")]
        device: String,
    },
    /// Start the sound-effect library management UI + JSON API (P1).
    Serve {
        #[arg(long, default_value_t = 8765)]
        port: u16,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long)]
        overlay: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Build / refresh the quantised fingerprint index (P2).
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
    /// Identify which stored sound effects a recording contains (P2).
    Match {
        #[arg(long)]
        wav: PathBuf,
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long)]
        index: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        top: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 0.75)]
        min_score: f64,
        #[arg(long)]
        quiet: bool,
    },
    /// Realtime recognition: loopback capture -> fingerprint -> ranked panel (P2).
    Live {
        #[arg(long, default_value = "")]
        device: String,
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long)]
        index: Option<PathBuf>,
        #[arg(long, default_value_t = 0.0)]
        seconds: f64,
        #[arg(long, default_value_t = 8)]
        top: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        csv: Option<PathBuf>,
        #[arg(long)]
        quiet: bool,
        #[arg(long)]
        wav: Option<PathBuf>,
    },
    /// Realtime recognition with the native hit overlay (P3).
    Overlay {
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long, default_value = "")]
        device: String,
        #[arg(long, default_value_t = 0.0)]
        seconds: f64,
        #[arg(long)]
        demo: bool,
        #[arg(long)]
        wav: Option<PathBuf>,
        #[arg(long)]
        x: Option<i32>,
        #[arg(long)]
        y: Option<i32>,
        #[arg(long)]
        size: Option<u32>,
        #[arg(long)]
        opacity: Option<f32>,
        #[arg(long)]
        csv: Option<PathBuf>,
        #[arg(long, default_value_t = 0.75)]
        min_score: f64,
        #[arg(long)]
        hold_item: Option<String>,
        #[arg(long, default_value_t = 20.0)]
        hold_seconds: f64,
    },
    /// Capture loopback for N seconds and trigger the recall save once (P4).
    Recall {
        #[arg(long, default_value_t = 0.0)]
        seconds: f64,
        #[arg(long)]
        at: Option<f64>,
        #[arg(long, default_value = "")]
        device: String,
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        ring: Option<f64>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Print version / build salt / go version and exit.
    Version,
    /// 双击直接跑：管理 UI + 实时识别 + 命中覆盖层 + 回溯热键。
    App {
        #[arg(long, default_value_t = 8765)]
        port: u16,
        #[arg(long, default_value = "")]
        device: String,
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum IndexCmd {
    /// Build / refresh the quantised fingerprint index.
    Rebuild {
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("soundradar: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    // 双击 / 无参数：直接进完整应用
    let cmd = cli.cmd.unwrap_or(Cmd::App {
        port: 8765,
        device: String::new(),
        library: None,
        config: None,
    });
    match cmd {
        Cmd::App {
            port,
            device,
            library,
            config,
        } => cmd_app(port, device, library, config),
        Cmd::Version => {
            println!("soundradar 0.7.0-rs");
            println!("  buildSalt : perf");
            println!("  buildTime : {}", crate::library::now_rfc3339());
            println!("  go        : rustc {}", env!("CARGO_PKG_VERSION"));
            println!("  platform  : windows/amd64");
            println!("  cgo       : disabled (pure Rust)");
            Ok(())
        }
        Cmd::Devices => cmd_devices(),
        Cmd::Capture {
            seconds,
            out,
            device,
        } => cmd_capture(seconds, &out, &device),
        Cmd::Serve {
            port,
            open,
            library,
            overlay,
            config,
        } => cmd_serve(port, open, library, overlay, config),
        Cmd::Index { cmd: IndexCmd::Rebuild { library, out } } => cmd_index_rebuild(library, out),
        Cmd::Match {
            wav,
            library,
            index,
            top,
            json,
            all,
            min_score,
            quiet,
        } => cmd_match(&wav, library, index, top, json, all, min_score, quiet),
        Cmd::Live {
            device,
            library,
            index,
            seconds,
            top,
            json,
            csv,
            quiet,
            wav,
        } => cmd_live(device, library, index, seconds, top, json, csv, quiet, wav),
        Cmd::Overlay {
            library,
            device,
            seconds,
            demo,
            wav,
            x,
            y,
            size,
            opacity,
            csv,
            min_score,
            hold_item,
            hold_seconds,
        } => cmd_overlay(
            library, device, seconds, demo, wav, x, y, size, opacity, csv, min_score, hold_item,
            hold_seconds,
        ),
        Cmd::Recall {
            seconds,
            at,
            device,
            library,
            out,
            dir,
            ring,
            json,
            config,
        } => cmd_recall(seconds, at, device, library, out, dir, ring, json, config),
    }
}

fn resolve_library(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(crate::library::default_path)
}

fn resolve_index(path: Option<PathBuf>, lib: &std::path::Path) -> PathBuf {
    path.unwrap_or_else(|| index::default_path_for(lib))
}

fn load_store(lib_path: &PathBuf) -> Result<crate::library::Store> {
    if !lib_path.exists() {
        bail!("音效库不存在: {}", lib_path.display());
    }
    crate::library::Store::open(lib_path)
}

/// Load index; rebuild patches when encoder fingerprint differs (format stays compatible).
fn load_index(lib_path: &PathBuf, idx_path: &PathBuf) -> Result<(index::Index, bool, String)> {
    let params = index::Params::default();
    let want_fp = params.fingerprint();
    if idx_path.exists() {
        let idx = index::Index::load(idx_path)?;
        if idx.params_fingerprint == want_fp {
            return Ok((idx, false, String::new()));
        }
        let why = format!(
            "参数指纹/编码器升级 ({} → {})",
            &idx.params_fingerprint[..12.min(idx.params_fingerprint.len())],
            &want_fp[..12]
        );
        let idx = build_index(lib_path)?;
        idx.save(idx_path)?;
        return Ok((idx, true, why));
    }
    let why = "索引不存在".to_string();
    let idx = build_index(lib_path)?;
    idx.save(idx_path)?;
    Ok((idx, true, why))
}

fn build_index(lib_path: &PathBuf) -> Result<index::Index> {
    let store = load_store(lib_path)?;
    let params = index::Params::default();
    let params_fingerprint = params.fingerprint();
    let mut items = Vec::new();
    let mut samples = Vec::new();
    let mut patches = Vec::new();
    let mut anchors = Vec::new();
    let mut sample_cursor = 0u32;

    println!("[index] 音效库     : {}", lib_path.display());
    println!(
        "[index] 特征算法   : mel-goertzel-v1 v1（2048 维 = 64 Mel 带 x 32 帧）"
    );
    println!(
        "[index] 帧参数     : 48000 Hz / 帧长 1024 / 跳步 256 (5.333 ms) / Hann / 去直流"
    );
    println!("[index] 归一化     : mean-subtract+l2（余弦相似度 = 点积）");
    println!("[index] 参数指纹   : {}", params_fingerprint);
    println!("[index] 开始建索引…");

    let t0 = std::time::Instant::now();
    let mut skipped = 0usize;
    for id in &store.order {
        let it = store
            .items
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("missing item"))?;
        let start = sample_cursor;
        let mut n_ok = 0u32;
        for s in &it.samples {
            let wav = store
                .sample_wav(&it.id, &s.file)
                .ok_or_else(|| anyhow::anyhow!("missing sample {}", s.file))?;
            let audio = wav::read_wav_bytes(wav)?;
            let mono = audio.canonical_mono_48k();
            let energy = {
                let mut sum = 0f64;
                for &v in &mono {
                    sum += (v as f64) * (v as f64);
                }
                (sum / mono.len().max(1) as f64) as f32
            };
            match dsp::fingerprint_best_patch(&mono) {
                Some((q, anchor)) => {
                    patches.push(q);
                    anchors.push(anchor);
                    samples.push(index::SampleEntry {
                        path: s.file.clone(),
                        item_index: items.len() as u32,
                        sample_index: n_ok as u16,
                        energy,
                    });
                    n_ok += 1;
                    sample_cursor += 1;
                }
                None => {
                    skipped += 1;
                }
            }
        }
        items.push(index::ItemEntry {
            id: it.id.clone(),
            name: it.name.clone(),
            sample_count: n_ok,
            sample_start: start,
            threshold: it.threshold,
            cooldown_ms: it.cooldown_ms,
        });
    }

    let idx = index::Index {
        params,
        params_fingerprint,
        items,
        samples,
        patches,
        anchors,
    };
    let dt = t0.elapsed();
    println!();
    println!("[index] 完成");
    println!("[index] 耗时       : {}ms", dt.as_millis());
    println!("[index] 条目数     : {}", idx.items.len());
    println!(
        "[index] 样本数     : {}（跳过 {}）",
        idx.samples.len(),
        skipped
    );
    println!("[index] 特征维数   : {}", dsp::DIM);
    println!(
        "[index] 量化字节   : {} 字节 ({:.1} KiB)",
        idx.samples_bytes(),
        idx.samples_bytes() as f64 / 1024.0
    );
    Ok(idx)
}

fn cmd_index_rebuild(library: Option<PathBuf>, out: Option<PathBuf>) -> Result<()> {
    let lib = resolve_library(library);
    let out = resolve_index(out, &lib);
    println!("[index] 输出索引   : {}", out.display());
    let idx = build_index(&lib)?;
    idx.save(&out)?;
    println!("[index] 索引文件   : {:.1} KiB（含表头与条目表）",
        std::fs::metadata(&out)?.len() as f64 / 1024.0);
    if !idx.samples.is_empty() {
        println!(
            "[index] 平均耗时   : {:.2} ms/样本",
            0.0
        );
    }
    println!();
    println!("[index] 每个条目的样本数:");
    for it in &idx.items {
        println!("        {:<20} 样本 {}", truncate(&it.name, 20), it.sample_count);
    }
    println!();
    println!("[index] 提示: 用 `soundradar match --wav <file.wav>` 拿一段录音来认音效");
    Ok(())
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}

fn cmd_match(
    wav_path: &PathBuf,
    library: Option<PathBuf>,
    index_path: Option<PathBuf>,
    top: usize,
    json: bool,
    all: bool,
    min_score: f64,
    quiet: bool,
) -> Result<()> {
    let lib = resolve_library(library);
    let idx_path = resolve_index(index_path, &lib);
    let (idx, rebuilt, why) = load_index(&lib, &idx_path)?;
    let audio = wav::read_audio_file(wav_path)?;
    let mono = audio.canonical_mono_48k();
    let frames = dsp::n_frames(mono.len());
    let windows = frames.saturating_sub(dsp::WINDOW_FRAMES) + 1;
    let hop_ms = dsp::HOP_SIZE as f64 * 1000.0 / dsp::SAMPLE_RATE as f64;
    let frame_ms = dsp::FRAME_SIZE as f64 * 1000.0 / dsp::SAMPLE_RATE as f64;
    let window_ms = (dsp::WINDOW_FRAMES as f64 - 1.0) * hop_ms + frame_ms;

    let t0 = std::time::Instant::now();
    let qpatches = dsp::fingerprint_all_windows(&mono);
    let hits = idx.search(&qpatches);
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let input = serde_json::json!({
        "path": wav_path.display().to_string(),
        "sampleRate": audio.sample_rate,
        "channels": audio.channels,
        "frames": audio.samples.len() / audio.channels.max(1) as usize,
        "seconds": audio.samples.len() as f64 / audio.channels.max(1) as f64 / audio.sample_rate as f64,
        "rmsDbfs": wav::rms_dbfs(&audio.samples),
        "peakDbfs": wav::peak_dbfs(&audio.samples),
        "resampled": audio.sample_rate != 48000,
        "downmixed": audio.channels > 1,
    });
    let index_meta = serde_json::json!({
        "path": idx_path.display().to_string(),
        "rebuilt": rebuilt,
        "fingerprint": idx.params_fingerprint,
        "algorithm": idx.params.algorithm,
        "dim": idx.params.dim(),
        "items": idx.items.len(),
        "samples": idx.samples.len(),
        "samplesBytes": idx.samples_bytes(),
    });
    let search = serde_json::json!({
        "frames": frames,
        "windows": windows,
        "windowMs": window_ms,
        "hopMs": hop_ms,
        "elapsedMs": elapsed_ms,
        "minScore": min_score,
    });

    let list: Vec<_> = if all {
        hits.iter().collect()
    } else {
        hits.iter().take(top).collect()
    };

    if json {
        let top_json: Vec<_> = list
            .iter()
            .map(|h| {
                serde_json::json!({
                    "rank": h.rank,
                    "id": h.id,
                    "name": h.name,
                    "score": h.score,
                    "atMs": h.at_ms,
                    "sample": h.sample,
                    "sampleIndex": h.sample_index,
                })
            })
            .collect();
        let best = hits.first().map(|h| {
            serde_json::json!({
                "rank": h.rank,
                "id": h.id,
                "name": h.name,
                "score": h.score,
                "atMs": h.at_ms,
                "sample": h.sample,
                "sampleIndex": h.sample_index,
            })
        });
        let verdict = hits
            .first()
            .filter(|h| h.score as f64 >= min_score)
            .map(|h| {
                format!(
                    "最像的是「{}」，相似度 {:.4}（出现在 {:.2} 秒处）",
                    h.name,
                    h.score,
                    h.at_ms / 1000.0
                )
            })
            .unwrap_or_else(|| "没有足够相似的音效".into());
        let out = serde_json::json!({
            "input": input,
            "index": index_meta,
            "search": search,
            "top": top_json,
            "best": best,
            "verdict": verdict,
            "rebuildWhy": why,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if !quiet {
        println!("输入     : {}", wav_path.display());
        println!(
            "RMS/峰值 : {} / {} dBFS",
            wav::format_dbfs(wav::rms_dbfs(&audio.samples)),
            wav::format_dbfs(wav::peak_dbfs(&audio.samples))
        );
        println!(
            "索引     : {}（{} 条目 / {} 样本）",
            idx_path.display(),
            idx.items.len(),
            idx.samples.len()
        );
        println!("检索     : {} 帧 / {} 窗 / {:.1} ms", frames, windows, elapsed_ms);
        println!();
    }
    for h in list {
        println!(
            "#{:<2} {:<24} {:.4}  @ {:.0} ms  {}",
            h.rank,
            truncate(&h.name, 24),
            h.score,
            h.at_ms,
            h.sample
        );
    }
    if let Some(h) = hits.first() {
        if (h.score as f64) >= min_score {
            println!();
            println!(
                "最像的是「{}」，相似度 {:.4}（出现在 {:.2} 秒处）",
                h.name,
                h.score,
                h.at_ms / 1000.0
            );
        } else if !quiet {
            println!();
            println!("没有足够相似的音效（最高 {:.4} < {:.2}）", h.score, min_score);
        }
    }
    Ok(())
}

fn cmd_devices() -> Result<()> {
    #[cfg(windows)]
    {
        win::list_devices()
    }
    #[cfg(not(windows))]
    {
        println!("(no audio devices on this platform)");
        Ok(())
    }
}

fn cmd_capture(seconds: f64, out: &PathBuf, device: &str) -> Result<()> {
    if seconds <= 0.0 {
        bail!("capture: duration must be positive, got {}", seconds);
    }
    println!("[capture] starting...");
    #[cfg(windows)]
    {
        win::capture_loopback(seconds, out, device)
    }
    #[cfg(not(windows))]
    {
        let _ = (out, device);
        bail!("capture: Windows only")
    }
}

/// 双击运行：后台起 serve（管理 UI），前台持续实时识别 + 命中播报。
fn cmd_app(
    port: u16,
    device: String,
    library: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Result<()> {
    let lib = resolve_library(library);
    let cfg_path = config.clone().unwrap_or_else(crate::config::default_path);
    let cfg = crate::config::Config::load(&cfg_path).unwrap_or_default();
    let idx_path = resolve_index(None, &lib);

    println!("SoundRadar 已启动");
    println!("  音效库   : {}", lib.display());
    println!("  管理端   : http://127.0.0.1:{}/", port);
    println!(
        "  覆盖层   : {}",
        if cfg.overlay.enabled { "开" } else { "关" }
    );
    println!(
        "  热键     : 回溯 {} / 开关覆盖层 {}",
        cfg.hotkeys.recall_label, cfg.hotkeys.toggle_overlay
    );
    println!("  退出     : 关闭本窗口或 Ctrl+C");
    println!();

    let serve_lib = Some(lib.clone());
    let serve_cfg = config.clone();
    std::thread::spawn(move || {
        let _ = cmd_serve(port, true, serve_lib, true, serve_cfg);
    });

    // 确保索引就绪
    let (idx, rebuilt, why) = load_index(&lib, &idx_path)?;
    if rebuilt && !why.is_empty() {
        println!("[app] 索引已重建：{}", why);
    }
    let idx = std::sync::Arc::new(idx);
    let min_score = 0.75f32;
    let mut last_ids: Vec<String> = Vec::new();

    loop {
        // 采 3 秒 → 识别 → 播报命中
        let audio = match capture_chunk(&device, 3.0) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[app] 采集失败，2 秒后重试: {:#}", e);
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
        };
        let mono = audio.canonical_mono_48k();
        let patches = dsp::fingerprint_all_windows(&mono);
        let hits = idx.search(&patches);
        let mut now = Vec::new();
        for h in hits.iter().filter(|h| h.score >= min_score).take(3) {
            now.push(h.id.clone());
            println!(
                "命中  {:<20} {:.3}  @ {:.2}s",
                truncate(&h.name, 20),
                h.score,
                h.at_ms / 1000.0
            );
            #[cfg(windows)]
            {
                let _ = crate::win::show_hit(&h.name, h.score, None);
            }
        }
        if now != last_ids {
            if now.is_empty() && !last_ids.is_empty() {
                println!("—");
            }
            last_ids = now;
        }
    }
}

fn capture_chunk(device: &str, seconds: f64) -> Result<wav::Audio> {
    #[cfg(windows)]
    {
        let tmp = std::env::temp_dir().join("soundradar_app_chunk.wav");
        crate::win::capture_loopback(seconds, &tmp, device)?;
        wav::read_audio_file(&tmp)
    }
    #[cfg(not(windows))]
    {
        let _ = (device, seconds);
        anyhow::bail!("app: Windows only")
    }
}

fn cmd_serve(
    port: u16,
    open: bool,
    library: Option<PathBuf>,
    overlay: bool,
    config: Option<PathBuf>,
) -> Result<()> {
    serve::run(port, open, library, overlay, config)
}

fn cmd_live(
    device: String,
    library: Option<PathBuf>,
    index_path: Option<PathBuf>,
    seconds: f64,
    top: usize,
    json: bool,
    csv: Option<PathBuf>,
    quiet: bool,
    wav: Option<PathBuf>,
) -> Result<()> {
    live::run(device, library, index_path, seconds, top, json, csv, quiet, wav)
}

fn cmd_overlay(
    library: Option<PathBuf>,
    device: String,
    seconds: f64,
    demo: bool,
    wav: Option<PathBuf>,
    x: Option<i32>,
    y: Option<i32>,
    size: Option<u32>,
    opacity: Option<f32>,
    csv: Option<PathBuf>,
    min_score: f64,
    hold_item: Option<String>,
    hold_seconds: f64,
) -> Result<()> {
    overlay_ui::run(
        library, device, seconds, demo, wav, x, y, size, opacity, csv, min_score, hold_item,
        hold_seconds,
    )
}

fn cmd_recall(
    seconds: f64,
    at: Option<f64>,
    device: String,
    library: Option<PathBuf>,
    out: Option<PathBuf>,
    dir: Option<PathBuf>,
    ring: Option<f64>,
    json: bool,
    config: Option<PathBuf>,
) -> Result<()> {
    recall::run(seconds, at, device, library, out, dir, ring, json, config)
}

// Extra modules (windows audio, serve, live, overlay, recall)
#[cfg(windows)]
mod win;
mod serve;
mod live;
mod overlay_ui;
mod recall;
