//! WAV / PCM helpers (16-bit PCM mono/stereo → f32 mono @ 48k).

use anyhow::{bail, Context, Result};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

#[derive(Debug, Clone)]
pub struct Audio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Audio {
    pub fn mono(&self) -> Vec<f32> {
        if self.channels == 1 {
            return self.samples.clone();
        }
        let ch = self.channels as usize;
        let frames = self.samples.len() / ch;
        let mut out = Vec::with_capacity(frames);
        for i in 0..frames {
            let mut s = 0f32;
            for c in 0..ch {
                s += self.samples[i * ch + c];
            }
            out.push(s / ch as f32);
        }
        out
    }

    pub fn resample_linear(&self, target_rate: u32) -> Vec<f32> {
        if self.sample_rate == target_rate {
            return self.samples.clone();
        }
        let src = self.mono();
        let ratio = target_rate as f64 / self.sample_rate as f64;
        let n_out = ((src.len() as f64) * ratio) as usize;
        let mut out = Vec::with_capacity(n_out);
        for i in 0..n_out {
            let pos = i as f64 / ratio;
            let i0 = pos.floor() as usize;
            let i1 = (i0 + 1).min(src.len().saturating_sub(1));
            let t = (pos - i0 as f64) as f32;
            if i0 >= src.len() {
                out.push(0.0);
            } else {
                out.push(src[i0] * (1.0 - t) + src[i1] * t);
            }
        }
        out
    }

    pub fn canonical_mono_48k(&self) -> Vec<f32> {
        if self.sample_rate == 48_000 {
            self.mono()
        } else {
            self.resample_linear(48_000)
        }
    }
}

pub fn peak_dbfs(samples: &[f32]) -> f64 {
    let mut peak = 0f32;
    for &s in samples {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
    }
    if peak <= 0.0 {
        return -120.0;
    }
    20.0 * (peak as f64).log10()
}

pub fn rms_dbfs(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return -120.0;
    }
    let mut sum = 0f64;
    for &s in samples {
        sum += (s as f64) * (s as f64);
    }
    let rms = (sum / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        return -120.0;
    }
    20.0 * rms.log10()
}

pub fn format_dbfs(v: f64) -> String {
    if v <= -119.0 {
        return "-inf".into();
    }
    format!("{:.2}", v)
}

pub fn read_wav_bytes(data: &[u8]) -> Result<Audio> {
    read_wav(Cursor::new(data))
}

pub fn read_wav<R: Read + Seek>(mut r: R) -> Result<Audio> {
    let mut tag = [0u8; 4];
    r.read_exact(&mut tag).context("wav: short header")?;
    if &tag != b"RIFF" {
        bail!("wav: not a RIFF/WAVE file");
    }
    let mut u32b = [0u8; 4];
    r.read_exact(&mut u32b)?;
    r.read_exact(&mut tag)?;
    if &tag != b"WAVE" {
        bail!("wav: not a RIFF/WAVE file");
    }

    let mut fmt: Option<(u16, u16, u32, u16, u16)> = None;
    let mut data: Option<Vec<u8>> = None;

    loop {
        let mut id = [0u8; 4];
        match r.read_exact(&mut id) {
            Ok(()) => {}
            Err(_) => break,
        }
        r.read_exact(&mut u32b)?;
        let size = u32::from_le_bytes(u32b) as usize;
        match &id {
            b"fmt " => {
                let mut buf = vec![0u8; size];
                r.read_exact(&mut buf)?;
                if size < 16 {
                    bail!("wav: fmt chunk too short");
                }
                let format_tag = u16::from_le_bytes([buf[0], buf[1]]);
                let channels = u16::from_le_bytes([buf[2], buf[3]]);
                let sample_rate = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
                let bits = u16::from_le_bytes([buf[14], buf[15]]);
                fmt = Some((format_tag, channels, sample_rate, bits, bits / 8));
                if size % 2 == 1 {
                    let mut p = [0u8; 1];
                    let _ = r.read_exact(&mut p);
                }
            }
            b"data" => {
                let mut buf = vec![0u8; size];
                r.read_exact(&mut buf)?;
                data = Some(buf);
                if size % 2 == 1 {
                    let mut p = [0u8; 1];
                    let _ = r.read_exact(&mut p);
                }
            }
            _ => {
                r.seek(SeekFrom::Current(size as i64))?;
                if size % 2 == 1 {
                    r.seek(SeekFrom::Current(1))?;
                }
            }
        }
    }

    let (format_tag, channels, sample_rate, bits, bytes_per_sample) =
        fmt.ok_or_else(|| anyhow::anyhow!("wav: no fmt chunk found"))?;
    let data = data.ok_or_else(|| anyhow::anyhow!("wav: no data chunk"))?;

    if format_tag != 1 && format_tag != 0xFFFE {
        bail!("wav: unsupported format tag 0x{:04X}", format_tag);
    }
    if channels == 0 {
        bail!("wav: invalid channel count");
    }

    let samples: Vec<f32> = match (bits, bytes_per_sample) {
        (16, 2) => data
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        (32, 4) => data
            .chunks_exact(4)
            .map(|c| {
                let v = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                v as f32 / 2147483648.0
            })
            .collect(),
        (24, 3) => data
            .chunks_exact(3)
            .map(|c| {
                let v = ((c[2] as i32) << 16) | ((c[1] as i32) << 8) | (c[0] as i32);
                let v = (v << 8) >> 8;
                v as f32 / 8388608.0
            })
            .collect(),
        (8, 1) => data
            .iter()
            .map(|&b| (b as i16 - 128) as f32 / 32768.0)
            .collect(),
        _ => bail!(
            "wav: unusable format (channels={} bpf={})",
            channels,
            bytes_per_sample
        ),
    };

    Ok(Audio {
        sample_rate,
        channels,
        samples,
    })
}

pub fn read_audio_file(path: &std::path::Path) -> Result<Audio> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if data.len() >= 12 && &data[0..4] == b"RIFF" {
        return read_wav_bytes(&data);
    }
    // try mp3 via symphonia
    decode_compressed(&data).with_context(|| format!("decode {}", path.display()))
}

fn decode_compressed(data: &[u8]) -> Result<Audio> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let mss = MediaSourceStream::new(Box::new(Cursor::new(data.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow::anyhow!("no audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = 44_100u32;
    let mut channels = 1u16;
    let mut spec: Option<symphonia::core::audio::SignalSpec> = None;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if spec.is_none() {
                    let s = *audio_buf.spec();
                    sample_rate = s.rate;
                    channels = s.channels.count() as u16;
                    sample_buf = Some(SampleBuffer::new(audio_buf.capacity() as u64, *audio_buf.spec()));
                }
                if let Some(sb) = sample_buf.as_mut() {
                    sb.copy_interleaved_ref(audio_buf);
                    samples.extend_from_slice(sb.samples());
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(Audio {
        sample_rate,
        channels,
        samples,
    })
}

pub fn write_wav_i16(path: &std::path::Path, sample_rate: u32, channels: u16, samples: &[f32]) -> Result<()> {
    let mut buf = Vec::with_capacity(44 + samples.len() * 2);
    write_wav_i16_to(&mut buf, sample_rate, channels, samples)?;
    std::fs::write(path, buf)?;
    Ok(())
}

pub fn write_wav_i16_to<W: Write>(
    w: &mut W,
    sample_rate: u32,
    channels: u16,
    samples: &[f32],
) -> Result<()> {
    let n = samples.len() as u32 * 2;
    let byte_rate = sample_rate * channels as u32 * 2;
    let block_align = channels * 2;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + n).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&16u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&n.to_le_bytes())?;
    for &s in samples {
        let v = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        w.write_all(&v.to_le_bytes())?;
    }
    Ok(())
}
