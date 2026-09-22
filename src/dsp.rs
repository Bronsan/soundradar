//! mel-goertzel-v1 fingerprint: 64 Mel × 32 frames = 2048-dim.
//! Frame → DC remove → Hann → Mel energies → log(dB) → window floor →
//! mean-subtract → scale (std*32) → i8 quant.

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SIZE: usize = 1024;
pub const HOP_SIZE: usize = 256;
pub const MEL_BANDS: usize = 64;
pub const WINDOW_FRAMES: usize = 32;
pub const DIM: usize = MEL_BANDS * WINDOW_FRAMES;
pub const F_MIN_HZ: f64 = 40.0;
pub const F_MAX_HZ: f64 = 16_000.0;
pub const LOG_FLOOR_DB: f32 = -20.0;
pub const ALGO: &str = "mel-goertzel-v1";

#[inline]
pub fn hz_to_mel_htk(f: f64) -> f64 {
    2595.0 * (1.0 + f / 700.0).log10()
}

#[inline]
pub fn mel_to_hz_htk(m: f64) -> f64 {
    700.0 * (10f64.powf(m / 2595.0) - 1.0)
}

pub fn hann_periodic(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = std::f32::consts::TAU * i as f32 / n as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

pub fn n_frames(audio_len: usize) -> usize {
    if audio_len < FRAME_SIZE {
        0
    } else {
        1 + (audio_len - FRAME_SIZE) / HOP_SIZE
    }
}

pub struct Analyzer {
    pub centers: Vec<f64>,
    pub window: Vec<f32>,
    /// Triangular mel filterbank over rFFT bins: [MEL_BANDS][n_bins]
    pub fbank: Vec<Vec<f32>>,
}

impl Analyzer {
    pub fn new() -> Self {
        let m_min = hz_to_mel_htk(F_MIN_HZ);
        let m_max = hz_to_mel_htk(F_MAX_HZ);
        let mut centers = Vec::with_capacity(MEL_BANDS);
        let mut edges = Vec::with_capacity(MEL_BANDS + 2);
        for i in 0..(MEL_BANDS + 2) {
            let m = m_min + (m_max - m_min) * i as f64 / (MEL_BANDS as f64 + 1.0);
            edges.push(mel_to_hz_htk(m));
        }
        for i in 1..=MEL_BANDS {
            centers.push(edges[i]);
        }

        let n_bins = FRAME_SIZE / 2 + 1;
        let mut fbank = vec![vec![0f32; n_bins]; MEL_BANDS];
        for b in 0..MEL_BANDS {
            let l = edges[b];
            let c = edges[b + 1];
            let r = edges[b + 2];
            for k in 0..n_bins {
                let f = k as f64 * SAMPLE_RATE as f64 / FRAME_SIZE as f64;
                let up = (f - l) / (c - l).max(1e-9);
                let dn = (r - f) / (r - c).max(1e-9);
                let w = up.min(dn).max(0.0) as f32;
                fbank[b][k] = w;
            }
        }

        Self {
            centers,
            window: hann_periodic(FRAME_SIZE),
            fbank,
        }
    }

    /// In-place radix-2 real FFT via complex FFT on interleaved re/im.
    /// `re`/`im` length = FRAME_SIZE (im starts zero). Output spectrum in re/im[0..n/2].
    #[inline]
    pub fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
        let n = re.len();
        debug_assert!(n.is_power_of_two());
        // bit-reverse
        let mut j = 0usize;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let ang = -std::f32::consts::TAU / len as f32;
            let (wlen_re, wlen_im) = (ang.cos(), ang.sin());
            let half = len / 2;
            for i in (0..n).step_by(len) {
                let (mut wr, mut wi) = (1.0f32, 0.0f32);
                for k in 0..half {
                    let ur = re[i + k];
                    let ui = im[i + k];
                    let vr = re[i + k + half] * wr - im[i + k + half] * wi;
                    let vi = re[i + k + half] * wi + im[i + k + half] * wr;
                    re[i + k] = ur + vr;
                    im[i + k] = ui + vi;
                    re[i + k + half] = ur - vr;
                    im[i + k + half] = ui - vi;
                    let nwr = wr * wlen_re - wi * wlen_im;
                    wi = wr * wlen_im + wi * wlen_re;
                    wr = nwr;
                }
            }
            len <<= 1;
        }
    }

    /// One frame → 64 mel dB values (no floor yet).
    pub fn frame_log_mel(&self, input: &[f32], out: &mut [f32; MEL_BANDS]) {
        let mut re = [0f32; FRAME_SIZE];
        let mut im = [0f32; FRAME_SIZE];
        let mut mean = 0f32;
        for &x in input {
            mean += x;
        }
        mean /= FRAME_SIZE as f32;
        for i in 0..FRAME_SIZE {
            re[i] = (input[i] - mean) * self.window[i];
        }
        Self::fft_in_place(&mut re, &mut im);
        let n_bins = FRAME_SIZE / 2 + 1;
        let mut power = [0f32; 513];
        for k in 0..n_bins {
            power[k] = re[k] * re[k] + im[k] * im[k];
        }
        for b in 0..MEL_BANDS {
            let mut acc = 0f32;
            let row = &self.fbank[b];
            for k in 0..n_bins {
                acc += row[k] * power[k];
            }
            let v = acc.max(1e-20);
            out[b] = 10.0 * v.log10();
        }
    }
}

pub fn apply_window_floor(patch: &mut [f32; DIM], floor_db: f32) {
    let mut mx = f32::MIN;
    for &v in patch.iter() {
        if v > mx {
            mx = v;
        }
    }
    let floor = mx + floor_db;
    for v in patch.iter_mut() {
        if *v < floor {
            *v = floor;
        }
    }
}

/// mean-subtract, then scale by 32/std → i8 (matches original quant distribution).
pub fn quantize_mean_std(patch: &[f32; DIM]) -> [i8; DIM] {
    let mut mean = 0f32;
    for &v in patch.iter() {
        mean += v;
    }
    mean /= DIM as f32;
    let mut var = 0f32;
    for &v in patch.iter() {
        let d = v - mean;
        var += d * d;
    }
    let std = (var / DIM as f32).sqrt().max(1e-12);
    let scale = 32.0 / std;
    let mut q = [0i8; DIM];
    for i in 0..DIM {
        let x = ((patch[i] - mean) * scale).round();
        q[i] = x.clamp(-128.0, 127.0) as i8;
    }
    q
}

/// mean-subtract + L2 → i8 via *127 (used for search-side float scoring if needed).
pub fn normalize_l2(patch: &mut [f32; DIM]) {
    let mut mean = 0f32;
    for &v in patch.iter() {
        mean += v;
    }
    mean /= DIM as f32;
    for v in patch.iter_mut() {
        *v -= mean;
    }
    let mut s = 0f32;
    for &v in patch.iter() {
        s += v * v;
    }
    let n = s.sqrt();
    if n > 0.0 {
        for v in patch.iter_mut() {
            *v /= n;
        }
    }
}

pub fn extract_patch(an: &Analyzer, audio: &[f32], start_frame: usize) -> Option<([i8; DIM], u32)> {
    let nf = n_frames(audio.len());
    if start_frame + WINDOW_FRAMES > nf {
        return None;
    }
    let mut patch = [0f32; DIM];
    let mut row = [0f32; MEL_BANDS];
    for t in 0..WINDOW_FRAMES {
        let off = (start_frame + t) * HOP_SIZE;
        an.frame_log_mel(&audio[off..off + FRAME_SIZE], &mut row);
        patch[t * MEL_BANDS..(t + 1) * MEL_BANDS].copy_from_slice(&row);
    }
    apply_window_floor(&mut patch, LOG_FLOOR_DB);
    let q = quantize_mean_std(&patch);
    Some((q, start_frame as u32))
}

/// Pick the 32-frame window with max mean energy (stable representative patch).
pub fn fingerprint_best_patch(audio: &[f32]) -> Option<([i8; DIM], u32)> {
    let an = Analyzer::new();
    let nf = n_frames(audio.len());
    if nf < WINDOW_FRAMES {
        return None;
    }
    let mut best = 0usize;
    let mut best_e = f32::MIN;
    for f in 0..=(nf - WINDOW_FRAMES) {
        let mut e = 0f32;
        for t in 0..WINDOW_FRAMES {
            let off = (f + t) * HOP_SIZE;
            let mut s = 0f32;
            for &x in &audio[off..off + FRAME_SIZE] {
                s += x * x;
            }
            e += s;
        }
        if e > best_e {
            best_e = e;
            best = f;
        }
    }
    extract_patch(&an, audio, best)
}

/// Sliding windows over the whole clip (for match/live search).
/// Precomputes each frame's log-mel once (rayon-parallel), then stacks 32-frame patches.
pub fn fingerprint_all_windows(audio: &[f32]) -> Vec<[i8; DIM]> {
    use rayon::prelude::*;
    let an = Analyzer::new();
    let nf = n_frames(audio.len());
    if nf < WINDOW_FRAMES {
        return Vec::new();
    }
    let mut lm = vec![0f32; nf * MEL_BANDS];
    lm.par_chunks_mut(MEL_BANDS)
        .enumerate()
        .for_each(|(f, row)| {
            let off = f * HOP_SIZE;
            let mut r = [0f32; MEL_BANDS];
            an.frame_log_mel(&audio[off..off + FRAME_SIZE], &mut r);
            row.copy_from_slice(&r);
        });
    let n_win = nf - WINDOW_FRAMES + 1;
    let mut out = vec![[0i8; DIM]; n_win];
    out.par_iter_mut().enumerate().for_each(|(w, q)| {
        let mut patch = [0f32; DIM];
        patch.copy_from_slice(&lm[w * MEL_BANDS..(w + WINDOW_FRAMES) * MEL_BANDS]);
        apply_window_floor(&mut patch, LOG_FLOOR_DB);
        *q = quantize_mean_std(&patch);
    });
    out
}

/// Log-mel matrix (nf × 64) for streaming/live reuse.
pub fn log_mel_frames(an: &Analyzer, audio: &[f32]) -> Vec<f32> {
    let nf = n_frames(audio.len());
    let mut lm = vec![0f32; nf * MEL_BANDS];
    let mut row = [0f32; MEL_BANDS];
    for f in 0..nf {
        let off = f * HOP_SIZE;
        an.frame_log_mel(&audio[off..off + FRAME_SIZE], &mut row);
        lm[f * MEL_BANDS..(f + 1) * MEL_BANDS].copy_from_slice(&row);
    }
    lm
}

/// Build quantized patch from a precomputed log-mel window.
pub fn patch_from_lm(lm: &[f32], start_frame: usize) -> [i8; DIM] {
    let mut patch = [0f32; DIM];
    patch.copy_from_slice(&lm[start_frame * MEL_BANDS..(start_frame + WINDOW_FRAMES) * MEL_BANDS]);
    apply_window_floor(&mut patch, LOG_FLOOR_DB);
    quantize_mean_std(&patch)
}

pub fn params_json() -> String {
    format!(
        "{{\n  \"algorithm\": \"mel-goertzel-v1\",\n  \"version\": 1,\n  \"sampleRate\": 48000,\n  \"frameSize\": 1024,\n  \"hopSize\": 256,\n  \"window\": \"hann-periodic\",\n  \"melBands\": 64,\n  \"fMinHz\": 40,\n  \"fMaxHz\": 16000,\n  \"windowFrames\": 32,\n  \"norm\": \"mean-subtract+l2\",\n  \"logFloorDb\": -20,\n  \"floorMode\": \"window-relative\"\n}}"
    )
}
