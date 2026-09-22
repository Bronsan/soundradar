//! Overlay mode: realtime hits with optional demo cycle (P3).

use crate::{dsp, index as idxmod, library as libmod, wav};
use anyhow::Result;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn run(
    library: Option<PathBuf>,
    device: String,
    seconds: f64,
    demo: bool,
    wav_path: Option<PathBuf>,
    x: Option<i32>,
    y: Option<i32>,
    size: Option<u32>,
    opacity: Option<f32>,
    csv: Option<PathBuf>,
    min_score: f64,
    hold_item: Option<String>,
    hold_seconds: f64,
) -> Result<()> {
    let lib_path = library.unwrap_or_else(libmod::default_path);
    let idx = idxmod::Index::load(&idxmod::default_path_for(&lib_path))?;
    let _ = (x, y, size, opacity, hold_item, hold_seconds);

    println!("[overlay] 音效库     : {}", lib_path.display());
    println!("[overlay] 条目数     : {}", idx.items.len());

    if demo {
        println!("[overlay] demo 模式  : 轮播整个音效库");
        for it in &idx.items {
            println!(
                "[overlay]   #{:<2} {:<24} threshold={:.2}",
                it.sample_start + 1,
                crate::truncate(&it.name, 24),
                it.threshold
            );
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        return Ok(());
    }

    let audio = match &wav_path {
        Some(p) => wav::read_audio_file(p)?,
        None => {
            let secs = if seconds > 0.0 { seconds } else { 10.0 };
            #[cfg(windows)]
            {
                let tmp = std::env::temp_dir().join("soundradar_overlay.wav");
                crate::win::capture_loopback(secs, &tmp, &device)?;
                wav::read_audio_file(&tmp)?
            }
            #[cfg(not(windows))]
            {
                let _ = &device;
                anyhow::bail!("overlay: use --wav on this platform");
            }
        }
    };
    let mono = audio.canonical_mono_48k();
    let patches = dsp::fingerprint_all_windows(&mono);
    let hits = idx.search(&patches);
    for h in hits.iter().filter(|h| h.score as f64 >= min_score).take(3) {
        println!(
            "[overlay] 命中 {:<20} {:.4} @ {:.0} ms",
            crate::truncate(&h.name, 20),
            h.score,
            h.at_ms
        );
        #[cfg(windows)]
        {
            let _ = crate::win::show_hit(&h.name, h.score, None);
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
        std::fs::write(csv_path, out)?;
    }
    Ok(())
}
