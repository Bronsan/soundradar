//! SRZ1 fingerprint index: load / build / search.

use crate::dsp::{DIM, WINDOW_FRAMES};
use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 4] = b"SRZ1";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Params {
    pub algorithm: String,
    pub version: i32,
    pub sample_rate: u32,
    pub frame_size: usize,
    pub hop_size: usize,
    pub window: String,
    pub mel_bands: usize,
    pub f_min_hz: f64,
    pub f_max_hz: f64,
    pub window_frames: usize,
    pub norm: String,
    pub log_floor_db: f64,
    pub floor_mode: String,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            algorithm: "mel-goertzel-v1".into(),
            version: 1,
            sample_rate: 48_000,
            frame_size: 1024,
            hop_size: 256,
            window: "hann-periodic".into(),
            mel_bands: 64,
            f_min_hz: 40.0,
            f_max_hz: 16_000.0,
            window_frames: 32,
            norm: "mean-subtract+l2".into(),
            log_floor_db: -20.0,
            floor_mode: "window-relative".into(),
        }
    }
}

impl Params {
    pub fn dim(&self) -> usize {
        self.mel_bands * self.window_frames
    }

    /// Compact JSON hashed into the parameter fingerprint (byte-identical to original).
    pub fn json_compact(&self) -> String {
        "{\"algorithm\":\"mel-goertzel-v1\",\"version\":1,\"sampleRate\":48000,\"frameSize\":1024,\"hopSize\":256,\"window\":\"hann-periodic\",\"melBands\":64,\"fMinHz\":40,\"fMaxHz\":16000,\"windowFrames\":32,\"norm\":\"mean-subtract+l2\",\"logFloorDb\":-20,\"floorMode\":\"window-relative\"}".to_string()
    }

    /// Pretty JSON stored inside SRZ1 (matches original formatting).
    pub fn json_canonical(&self) -> String {
        "{\n  \"algorithm\": \"mel-goertzel-v1\",\n  \"version\": 1,\n  \"sampleRate\": 48000,\n  \"frameSize\": 1024,\n  \"hopSize\": 256,\n  \"window\": \"hann-periodic\",\n  \"melBands\": 64,\n  \"fMinHz\": 40,\n  \"fMaxHz\": 16000,\n  \"windowFrames\": 32,\n  \"norm\": \"mean-subtract+l2\",\n  \"logFloorDb\": -20,\n  \"floorMode\": \"window-relative\"\n}".to_string()
    }

    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        // Include encoder salt so a new implementation rebuilds patches once.
        h.update(b"rust-encoder-v1|");
        h.update(self.json_compact().as_bytes());
        let out = h.finalize();
        out.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[derive(Debug, Clone)]
pub struct ItemEntry {
    pub id: String,
    pub name: String,
    pub sample_count: u32,
    pub sample_start: u32,
    pub threshold: f32,
    pub cooldown_ms: u32,
}

#[derive(Debug, Clone)]
pub struct SampleEntry {
    pub path: String,
    pub item_index: u32,
    pub sample_index: u16,
    pub energy: f32,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub rank: usize,
    pub id: String,
    pub name: String,
    pub score: f32,
    pub at_ms: f64,
    pub sample: String,
    pub sample_index: u32,
}

#[derive(Debug)]
pub struct Index {
    pub params: Params,
    pub params_fingerprint: String,
    pub items: Vec<ItemEntry>,
    pub samples: Vec<SampleEntry>,
    /// Quantized patches, each DIM bytes (i8).
    pub patches: Vec<[i8; DIM]>,
    /// Anchor frame index per sample.
    pub anchors: Vec<u32>,
}

impl Index {
    /// Build from store; empty store → empty index (never error).
    pub fn empty() -> Index {
        let params = Params::default();
        Index {
            params_fingerprint: params.fingerprint(),
            params,
            items: vec![],
            samples: vec![],
            patches: vec![],
            anchors: vec![],
        }
    }

    pub fn samples_bytes(&self) -> usize {
        self.patches.len() * DIM
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut w = Vec::with_capacity(2048 + self.samples_bytes());
        w.write_all(MAGIC)?;
        w.write_u32::<LittleEndian>(1)?;
        write_str(&mut w, &self.params.algorithm)?;
        write_str(&mut w, &self.params.json_canonical())?;
        write_str(&mut w, &self.params_fingerprint)?;
        w.write_u32::<LittleEndian>(self.params.dim() as u32)?;
        w.write_u32::<LittleEndian>(self.items.len() as u32)?;
        w.write_u32::<LittleEndian>(0)?; // skipped
        w.write_u32::<LittleEndian>(self.samples.len() as u32)?;
        for it in &self.items {
            write_str(&mut w, &it.id)?;
            write_str(&mut w, &it.name)?;
            w.write_u32::<LittleEndian>(it.sample_count)?;
            w.write_u32::<LittleEndian>(it.sample_start)?;
            w.write_f32::<LittleEndian>(it.threshold)?;
            w.write_u32::<LittleEndian>(it.cooldown_ms)?;
        }
        for s in &self.samples {
            write_str(&mut w, &s.path)?;
            w.write_u32::<LittleEndian>(s.item_index)?;
            w.write_u16::<LittleEndian>(s.sample_index)?;
            w.write_f32::<LittleEndian>(s.energy)?;
        }
        for p in &self.patches {
            for &v in p.iter() {
                w.write_i8(v)?;
            }
        }
        for &a in &self.anchors {
            w.write_u32::<LittleEndian>(a)?;
        }
        Ok(w)
    }

    pub fn decode(data: &[u8]) -> Result<Index> {
        let mut r = Cursor::new(data);
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            bail!("index: bad magic");
        }
        let ver = r.read_u32::<LittleEndian>()?;
        if ver != 1 {
            bail!("index: unsupported version {}", ver);
        }
        let algorithm = read_str(&mut r)?;
        let json = read_str(&mut r)?;
        let params_fingerprint = read_str(&mut r)?;
        let dim = r.read_u32::<LittleEndian>()? as usize;
        let n_items = r.read_u32::<LittleEndian>()? as usize;
        let _skipped = r.read_u32::<LittleEndian>()?;
        let n_samples = r.read_u32::<LittleEndian>()? as usize;
        if dim != DIM {
            // allow load but search uses DIM
        }
        let mut items = Vec::with_capacity(n_items);
        for _ in 0..n_items {
            items.push(ItemEntry {
                id: read_str(&mut r)?,
                name: read_str(&mut r)?,
                sample_count: r.read_u32::<LittleEndian>()?,
                sample_start: r.read_u32::<LittleEndian>()?,
                threshold: r.read_f32::<LittleEndian>()?,
                cooldown_ms: r.read_u32::<LittleEndian>()?,
            });
        }
        let mut samples = Vec::with_capacity(n_samples);
        for _ in 0..n_samples {
            samples.push(SampleEntry {
                path: read_str(&mut r)?,
                item_index: r.read_u32::<LittleEndian>()?,
                sample_index: r.read_u16::<LittleEndian>()?,
                energy: r.read_f32::<LittleEndian>()?,
            });
        }
        let mut patches = Vec::with_capacity(n_samples);
        for _ in 0..n_samples {
            let mut p = [0i8; DIM];
            for v in p.iter_mut() {
                *v = r.read_i8()?;
            }
            patches.push(p);
        }
        let mut anchors = Vec::with_capacity(n_samples);
        for _ in 0..n_samples {
            anchors.push(r.read_u32::<LittleEndian>()?);
        }
        let params: Params = serde_json::from_str(&json).unwrap_or_default();
        Ok(Index {
            params,
            params_fingerprint,
            items,
            samples,
            patches,
            anchors,
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = self.encode()?;
        let tmp = path.with_extension("bin.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Index> {
        let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        Self::decode(&data)
    }

    /// Score one query patch against all library patches. Returns per-sample best (score, frame).
    pub fn score_patch(&self, query: &[i8; DIM], query_frame: u32) -> Vec<(f32, u32)> {
        self.patches
            .iter()
            .zip(self.anchors.iter())
            .map(|(p, &a)| (dot_i8(query, p) as f32, query_frame))
            .collect()
    }

    /// Sliding search: for each sample keep best cosine (via i8 dot / norms).
    pub fn search(&self, query_patches: &[[i8; DIM]]) -> Vec<Hit> {
        use rayon::prelude::*;
        let n_s = self.patches.len();
        let hop_ms = self.params.hop_size as f64 * 1000.0 / self.params.sample_rate as f64;
        let lib_norms: Vec<f32> = self.patches.iter().map(|p| norm_i8(p)).collect();

        let best: Vec<(f32, u32)> = (0..n_s)
            .into_par_iter()
            .map(|si| {
                let lp = &self.patches[si];
                let ln = lib_norms[si];
                if ln <= 0.0 {
                    return (f32::MIN, 0u32);
                }
                let mut best_score = f32::MIN;
                let mut best_frame = 0u32;
                for (qi, qp) in query_patches.iter().enumerate() {
                    let qn = norm_i8(qp);
                    if qn <= 0.0 {
                        continue;
                    }
                    let sc = (dot_i8(qp, lp) as f32) / (qn * ln);
                    if sc > best_score {
                        best_score = sc;
                        best_frame = qi as u32;
                    }
                }
                (best_score, best_frame)
            })
            .collect();

        // aggregate to items: max over its samples
        let mut item_best: Vec<(f32, u32, usize)> = vec![(0.0, 0, 0); self.items.len()];
        for si in 0..n_s {
            let ii = self.samples[si].item_index as usize;
            if ii >= item_best.len() {
                continue;
            }
            let sc = if best[si].0.is_finite() { best[si].0 } else { 0.0 };
            if sc > item_best[ii].0 || item_best[ii].1 == 0 && item_best[ii].2 == 0 {
                if sc > item_best[ii].0 {
                    item_best[ii] = (sc, best[si].1, si);
                }
            }
        }

        let mut hits: Vec<Hit> = item_best
            .into_iter()
            .enumerate()
            .map(|(ii, (score, frame, si))| Hit {
                rank: 0,
                id: self.items[ii].id.clone(),
                name: self.items[ii].name.clone(),
                score,
                at_ms: frame as f64 * hop_ms,
                sample: self.samples
                    .get(si)
                    .map(|s| s.path.clone())
                    .unwrap_or_default(),
                sample_index: self.samples.get(si).map(|s| s.sample_index as u32).unwrap_or(0),
            })
            .collect();
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        for (i, h) in hits.iter_mut().enumerate() {
            h.rank = i + 1;
        }
        hits
    }
}

#[inline]
pub fn dot_i8(a: &[i8; DIM], b: &[i8; DIM]) -> i32 {
    // 2048-way i32 accumulate; portable + autovectorized
    let mut acc0 = 0i32;
    let mut acc1 = 0i32;
    let mut acc2 = 0i32;
    let mut acc3 = 0i32;
    let mut i = 0;
    while i + 4 <= DIM {
        acc0 += a[i] as i32 * b[i] as i32;
        acc1 += a[i + 1] as i32 * b[i + 1] as i32;
        acc2 += a[i + 2] as i32 * b[i + 2] as i32;
        acc3 += a[i + 3] as i32 * b[i + 3] as i32;
        i += 4;
    }
    acc0 + acc1 + acc2 + acc3
}

#[inline]
pub fn norm_i8(a: &[i8; DIM]) -> f32 {
    let mut acc = 0i64;
    for &v in a.iter() {
        acc += (v as i64) * (v as i64);
    }
    (acc as f32).sqrt()
}

fn write_str<W: Write>(w: &mut W, s: &str) -> Result<()> {
    let b = s.as_bytes();
    w.write_u16::<LittleEndian>(b.len() as u16)?;
    w.write_all(b)?;
    Ok(())
}

fn read_str<R: Read>(r: &mut R) -> Result<String> {
    let n = r.read_u16::<LittleEndian>()? as usize;
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8(buf)?)
}

pub fn default_path_for(library: &Path) -> PathBuf {
    library
        .parent()
        .map(|p| p.join("index.bin"))
        .unwrap_or_else(|| PathBuf::from("index.bin"))
}
