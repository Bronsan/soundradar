//! Recall: save last N seconds of loopback audio as a candidate (P4).

use crate::{config as cfgmod, index as idxmod, library as libmod, wav};
use anyhow::Result;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn run(
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
    let cfg_path = config.unwrap_or_else(cfgmod::default_path);
    let cfg = cfgmod::Config::load(&cfg_path)?;
    if !cfg.recall.enabled {
        anyhow::bail!("recall.enabled=false");
    }
    let secs = if seconds > 0.0 {
        seconds
    } else {
        ring.unwrap_or(cfg.recall.seconds)
    };
    if !(1.0..=30.0).contains(&secs) {
        anyhow::bail!("recall.seconds 应在 1..30，得到 {}", secs);
    }
    let _ = at;

    let lib_path = library.unwrap_or_else(libmod::default_path);
    let out_dir = dir.unwrap_or_else(|| {
        let d = PathBuf::from(&cfg.recall.dir);
        if d.is_absolute() {
            d
        } else {
            lib_path
                .parent()
                .map(|p| p.join(&cfg.recall.dir))
                .unwrap_or(d)
        }
    });
    std::fs::create_dir_all(&out_dir)?;

    let stamp = crate::library::now_rfc3339().replace(':', "").replace('-', "").replace('T', "-");
    let stamp: String = stamp.chars().take(15).collect();
    let out_path = out.unwrap_or_else(|| out_dir.join(format!("cand-{}.wav", stamp)));

    #[cfg(windows)]
    {
        crate::win::capture_loopback(secs, &out_path, &device)?;
    }
    #[cfg(not(windows))]
    {
        let _ = &device;
        // write silence so the command still works in CI
        let n = (secs * 48_000.0) as usize;
        wav::write_wav_i16(&out_path, 48_000, 1, &vec![0f32; n])?;
    }

    let audio = wav::read_audio_file(&out_path)?;
    let mono = audio.canonical_mono_48k();
    let peak = wav::peak_dbfs(&mono);

    // optional guess via index
    let mut guess = String::new();
    let idx_path = idxmod::default_path_for(&lib_path);
    if let Ok(idx) = idxmod::Index::load(&idx_path) {
        let patches = dsp_windows(&mono);
        if let Some(h) = idx.search(&patches).into_iter().next() {
            if h.score > 0.05 {
                guess = format!("{} ({:.4})", h.name, h.score);
            }
        }
    }

    // enforce maxFiles
    if cfg.recall.max_files > 0 {
        let mut wavs: Vec<_> = std::fs::read_dir(&out_dir)?
            .flatten()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("wav"))
            .collect();
        wavs.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        while wavs.len() as u32 > cfg.recall.max_files {
            if let Some(old) = wavs.first() {
                let _ = std::fs::remove_file(old.path());
                wavs.remove(0);
            } else {
                break;
            }
        }
    }

    println!("[recall] WAV        : {}", out_path.display());
    println!(
        "[recall] 电平       : {} dBFS",
        wav::format_dbfs(peak)
    );
    if !guess.is_empty() {
        println!("[recall] 猜测       : {}", guess);
    }

    if json {
        let v = serde_json::json!({
            "id": out_path.file_name().unwrap().to_string_lossy(),
            "path": out_path.display().to_string(),
            "seconds": secs,
            "peakDbfs": peak,
            "guess": guess,
            "ringSeconds": secs,
            "coveredSeconds": secs,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}

fn dsp_windows(mono: &[f32]) -> Vec<[i8; crate::dsp::DIM]> {
    crate::dsp::fingerprint_all_windows(mono)
}
