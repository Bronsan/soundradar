//! ZIP-based sound-effect library (.srz).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSpec {
    pub kind: String,
    #[serde(rename = "sampleRate")]
    pub sample_rate: u32,
    #[serde(rename = "frameSize")]
    pub frame_size: u32,
    #[serde(rename = "hopSize")]
    pub hop_size: u32,
    pub window: String,
    #[serde(rename = "melBands")]
    pub mel_bands: u32,
    #[serde(rename = "fMinHz")]
    pub f_min_hz: f64,
    #[serde(rename = "fMaxHz")]
    pub f_max_hz: f64,
    #[serde(rename = "peaksPerSec")]
    pub peaks_per_sec: u32,
}

impl Default for FeatureSpec {
    fn default() -> Self {
        Self {
            kind: "mel-goertzel-v1".into(),
            sample_rate: 48_000,
            frame_size: 2048,
            hop_size: 512,
            window: "hann".into(),
            mel_bands: 64,
            f_min_hz: 40.0,
            f_max_hz: 16_000.0,
            peaks_per_sec: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleOrigin {
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(default)]
    pub container: String,
    #[serde(rename = "formatTag", default)]
    pub format_tag: String,
    #[serde(rename = "sampleRate", default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub channels: u16,
    #[serde(rename = "bitsPerSample", default)]
    pub bits_per_sample: u16,
    #[serde(default)]
    pub duration_s: f64,
    #[serde(default)]
    pub frames: u32,
    #[serde(default)]
    pub bytes: u32,
    #[serde(default)]
    pub resampled: bool,
    #[serde(default)]
    pub downmixed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub file: String,
    #[serde(rename = "offsetS", default)]
    pub offset_s: f64,
    #[serde(rename = "lenS", default)]
    pub len_s: f64,
    #[serde(rename = "gainDb", default)]
    pub gain_db: f64,
    #[serde(default)]
    pub source: String,
    #[serde(rename = "addedAt", default)]
    pub added_at: String,
    #[serde(default)]
    pub origin: Option<SampleOrigin>,
    #[serde(rename = "storedSampleRate", default)]
    pub stored_sample_rate: u32,
    #[serde(rename = "storedChannels", default)]
    pub stored_channels: u16,
    #[serde(rename = "storedBitsPerSample", default)]
    pub stored_bits_per_sample: u16,
    #[serde(default)]
    pub frames: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub threshold: f32,
    #[serde(rename = "cooldownMs", default)]
    pub cooldown_ms: u32,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub samples: Vec<Sample>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub feature: FeatureSpec,
    pub items: Vec<String>,
}

pub struct Store {
    pub path: PathBuf,
    pub manifest: Manifest,
    pub items: BTreeMap<String, Item>,
    pub order: Vec<String>,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_bytes(path.to_path_buf(), &data)
    }

    pub fn from_bytes(path: PathBuf, data: &[u8]) -> Result<Store> {
        let cur = Cursor::new(data);
        let mut zip = zip::ZipArchive::new(cur).context("ZIP/SRZ")?;
        let mut manifest: Option<Manifest> = None;
        let mut items = BTreeMap::new();
        let mut order = Vec::new();
        let mut blobs = BTreeMap::new();

        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            let name = file.name().to_string();
            let mut buf = Vec::with_capacity(file.size() as usize + 8);
            file.read_to_end(&mut buf)?;
            if name == "manifest.json" {
                manifest = Some(serde_json::from_slice(&buf).context("manifest.json")?);
            } else if name.ends_with("/meta.json") {
                let it: Item = serde_json::from_slice(&buf)
                    .with_context(|| format!("meta.json {}", name))?;
                order.push(it.id.clone());
                items.insert(it.id.clone(), it);
            } else {
                blobs.insert(name, buf);
            }
        }

        let mut manifest = manifest.ok_or_else(|| anyhow::anyhow!("missing manifest.json"))?;
        if manifest.items.is_empty() {
            manifest.items = order.clone();
        } else {
            order = manifest.items.clone();
        }

        Ok(Store {
            path,
            manifest,
            items,
            order,
            blobs,
        })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.get(id)
    }

    pub fn sample_wav(&self, item_id: &str, sample_file: &str) -> Option<&[u8]> {
        let key = format!("items/{}/{}", item_id, sample_file);
        self.blobs.get(&key).map(|v| v.as_slice())
    }

    pub fn icon_png(&self, item_id: &str) -> Option<&[u8]> {
        let key = format!("items/{}/icon.png", item_id);
        self.blobs.get(&key).map(|v| v.as_slice())
    }

    pub fn stats(&self) -> (usize, usize) {
        let samples: usize = self.items.values().map(|i| i.samples.len()).sum();
        (self.items.len(), samples)
    }
}

pub fn default_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let candidate = dir.join("data").join("library.srz");
    if candidate.exists() {
        return candidate;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let c2 = cwd.join("data").join("library.srz");
    if c2.exists() {
        return c2;
    }
    // prefer next to the exe even if missing — caller will create it
    candidate
}

/// Open library; if missing, create an empty one on disk so first-run never dies.
pub fn open_or_create(path: &Path) -> Result<Store> {
    if path.exists() {
        return Store::open(path);
    }
    let store = create_empty(path)?;
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    save(path, &store)?;
    Ok(Store::open(path).unwrap_or(store))
}

pub fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:08x}", (t as u32) ^ 0x9e3779b9)
}

/// Write store back to .srz (zip).
pub fn save(path: &Path, store: &Store) -> Result<()> {
    let tmp = path.with_extension("srz.tmp");
    {
        let file = std::fs::File::create(&tmp)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts)?;
        zip.write_all(&serde_json::to_vec_pretty(&store.manifest)?)?;
        for id in &store.order {
            let it = store
                .items
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("missing item {}", id))?;
            zip.start_file(format!("items/{}/meta.json", id), opts)?;
            zip.write_all(&serde_json::to_vec_pretty(it)?)?;
            let icon_key = format!("items/{}/icon.png", id);
            if let Some(icon) = store.blobs.get(&icon_key) {
                zip.start_file(icon_key, opts)?;
                zip.write_all(icon)?;
            }
            for s in &it.samples {
                let key = format!("items/{}/{}", id, s.file);
                if let Some(data) = store.blobs.get(&key) {
                    zip.start_file(key, opts)?;
                    zip.write_all(data)?;
                }
            }
        }
        zip.finish()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn placeholder_icon() -> Vec<u8> {
    // 1x1 PNG
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE, 0xD4, 0xEF, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}

pub fn is_empty_store(path: &Path) -> bool {
    !path.exists()
}

pub fn create_empty(path: &Path) -> Result<Store> {
    let now = chrono_like_now();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    Ok(Store {
        path: path.to_path_buf(),
        manifest: Manifest {
            schema: 1,
            name: "未命名音效库".into(),
            created_at: now,
            feature: FeatureSpec::default(),
            items: vec![],
        },
        items: BTreeMap::new(),
        order: vec![],
        blobs: BTreeMap::new(),
    })
}

fn chrono_like_now() -> String {
    // RFC3339-ish UTC without chrono dep
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (y, mo, dd, h, mi, s) = civil_from_unix(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.0000000Z",
        y, mo, dd, h, mi, s
    )
}

fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi, s)
}

pub fn now_rfc3339() -> String {
    chrono_like_now()
}
