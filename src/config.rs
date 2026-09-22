//! config.json load/save.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCfg {
    #[serde(default)]
    pub device: String,
    #[serde(rename = "fallbackToDefault", default = "default_true")]
    pub fallback_to_default: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub monitor: u32,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_anchor")]
    pub anchor: String,
    #[serde(default = "default_size")]
    pub size: u32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(rename = "durationMs", default = "default_duration")]
    pub duration_ms: u32,
    #[serde(rename = "fadeInMs", default = "default_fade_in")]
    pub fade_in_ms: u32,
    #[serde(rename = "fadeOutMs", default = "default_fade_out")]
    pub fade_out_ms: u32,
    #[serde(rename = "maxSimultaneous", default = "default_max_sim")]
    pub max_simultaneous: u32,
    #[serde(rename = "showName", default = "default_true")]
    pub show_name: bool,
    #[serde(rename = "showScore", default = "default_true")]
    pub show_score: bool,
    #[serde(default = "default_margin")]
    pub margin: u32,
}

fn default_anchor() -> String {
    "bottom-right".into()
}
fn default_size() -> u32 {
    220
}
fn default_opacity() -> f32 {
    0.85
}
fn default_duration() -> u32 {
    1500
}
fn default_fade_in() -> u32 {
    80
}
fn default_fade_out() -> u32 {
    250
}
fn default_max_sim() -> u32 {
    3
}
fn default_margin() -> u32 {
    24
}

impl Default for OverlayCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            monitor: 0,
            x: 0,
            y: 0,
            anchor: default_anchor(),
            size: default_size(),
            opacity: default_opacity(),
            duration_ms: default_duration(),
            fade_in_ms: default_fade_in(),
            fade_out_ms: default_fade_out(),
            max_simultaneous: default_max_sim(),
            show_name: true,
            show_score: true,
            margin: default_margin(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkeys {
    #[serde(rename = "toggleOverlay", default = "default_f9")]
    pub toggle_overlay: String,
    #[serde(rename = "recallLabel", default = "default_f8")]
    pub recall_label: String,
}

fn default_f9() -> String {
    "F9".into()
}
fn default_f8() -> String {
    "F8".into()
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle_overlay: default_f9(),
            recall_label: default_f8(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_recall_sec")]
    pub seconds: f64,
    #[serde(default = "default_recall_dir")]
    pub dir: String,
    #[serde(rename = "maxFiles", default = "default_max_files")]
    pub max_files: u32,
}

fn default_recall_sec() -> f64 {
    3.0
}
fn default_recall_dir() -> String {
    "data\\candidates".into()
}
fn default_max_files() -> u32 {
    200
}

impl Default for RecallCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            seconds: default_recall_sec(),
            dir: default_recall_dir(),
            max_files: default_max_files(),
        }
    }
}

impl Default for CaptureCfg {
    fn default() -> Self {
        Self {
            device: String::new(),
            fallback_to_default: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub capture: CaptureCfg,
    #[serde(default)]
    pub overlay: OverlayCfg,
    #[serde(default)]
    pub hotkeys: Hotkeys,
    #[serde(default)]
    pub recall: RecallCfg,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "default".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            capture: CaptureCfg::default(),
            overlay: OverlayCfg::default(),
            hotkeys: Hotkeys::default(),
            recall: RecallCfg::default(),
            profile: default_profile(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Config = serde_json::from_slice(&data).context("config.json")?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

pub fn default_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let c = dir.join("config.json");
    if c.exists() {
        return c;
    }
    std::env::current_dir()
        .unwrap_or_default()
        .join("config.json")
}

pub fn parse_hotkey(s: &str) -> (u32, u32) {
    // returns (vk, mods) — simplified: F1-F24 and single letters
    let s = s.trim();
    let upper = s.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u32>() {
            if (1..=24).contains(&n) {
                return (0x70 + n - 1, 0);
            }
        }
    }
    if upper.len() == 1 {
        let c = upper.chars().next().unwrap() as u32;
        return (c, 0);
    }
    match upper.as_str() {
        "SPACE" => (0x20, 0),
        "TAB" => (0x09, 0),
        "ESCAPE" | "ESC" => (0x1B, 0),
        _ => (0, 0),
    }
}
