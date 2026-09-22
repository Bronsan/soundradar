//! Realtime recognition (live): file replay or loopback → sliding fingerprint → ranked hits.

use crate::{dsp, index as idxmod, library as libmod, wav};
use anyhow::Result;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn run(
    device: String,
    library: Option<PathBuf>,
    index_path: Option<PathBuf>,
    seconds: f64,
    top: usize,
    json: bool,
    csv: Option<PathBuf>,
    quiet: bool,
    wav_path: Option<PathBuf>,
) -> Result<()> {
    let lib_path = library.unwrap_or_else(libmod::default_path);
    let idx_path = index_path.unwrap_or_else(|| idxmod::default_path_for(&lib_path));
    let idx = idxmod::Index::load(&idx_path)?;

    let audio = match &wav_path {
        Some(p) => wav::read_audio_file(p)?,
        None => {
            let secs = if seconds > 0.0 { seconds } else { 5.0 };
            #[cfg(windows)]
            {
                let tmp = std::env::temp_dir().join("soundradar_live.wav");
                crate::win::capture_loopback(secs, &tmp, &device)?;
                wav::read_audio_file(&tmp)?
            }
            #[cfg(not(windows))]
            {
                let _ = &device;
                anyhow::bail!("live: use --wav on this platform");
            }
        }
    };

    let mono = audio.canonical_mono_48k();
    let hop_ms = dsp::HOP_SIZE as f64 * 1000.0 / dsp::SAMPLE_RATE as f64;
    let frames = dsp::n_frames(mono.len());
    let windows = frames.saturating_sub(dsp::WINDOW_FRAMES) + 1;

    if !quiet {
        println!(
            "[live] 来源       : {}",
            wav_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| device.clone())
        );
        println!(
            "[live] 音效库     : {}（{} 条目）",
            lib_path.display(),
            idx.items.len()
        );
        println!(
            "[live] 参数指纹   : {}…",
            &idx.params_fingerprint[..12.min(idx.params_fingerprint.len())]
        );
        println!("[live] 开始识别…  按 Ctrl+C 停止");
    }

    let t0 = std::time::Instant::now();
    let patches = dsp::fingerprint_all_windows(&mono);
    let hits = idx.search(&patches);
    let elapsed = t0.elapsed().as_secs_f64() * 1000.0;

    if json {
        let v = serde_json::json!({
            "frames": frames,
            "windows": windows,
            "windowMs": dsp::WINDOW_FRAMES as f64 * hop_ms,
            "hopMs": hop_ms,
            "elapsedMs": elapsed,
            "top": hits.iter().take(top).map(|h| serde_json::json!({
                "rank": h.rank, "id": h.id, "name": h.name, "score": h.score, "atMs": h.at_ms
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        for h in hits.iter().take(top) {
            println!(
                "#{:<2} {:<24} {:.4}  @ {:.0} ms",
                h.rank,
                crate::truncate(&h.name, 24),
                h.score,
                h.at_ms
            );
        }
        if !quiet {
            println!("[live] 耗时       : {:.2} ms（{} 窗）", elapsed, windows);
        }
    }

    if let Some(csv_path) = csv {
        let mut out = String::from("rank,id,name,score,atMs\n");
        for h in &hits {
            out.push_str(&format!(
                "{},{},{},{:.6},{:.2}\n",
                h.rank, h.id, h.name, h.score, h.at_ms
            ));
        }
        std::fs::write(&csv_path, out)?;
        if !quiet {
            println!("[live] CSV        : {}", csv_path.display());
        }
    }
    Ok(())
}
