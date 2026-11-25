use crate::sample_tags::{MetadataWords, categories, found, tokens};
use crate::scope::{Source, check_cancel, extension};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::FormatOptions,
    io::{MediaSource, MediaSourceStream},
    meta::{Metadata, MetadataOptions},
    probe::Hint,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Analysis {
    #[serde(default)]
    pub(crate) revision: u32,
    pub seconds: Option<f64>,
    pub tags: Vec<String>,
    pub error: Option<String>,
}

pub(crate) const ANALYSIS_REVISION: u32 = 3;

pub fn analyze(source: &Source, cancel: &AtomicBool) -> Result<Analysis> {
    check_cancel(cancel)?;
    let input = AudioInput::open(source)?;
    let stream = MediaSourceStream::new(Box::new(input), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&extension(&source.path));
    let mut probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut embedded = MetadataWords::default();
    if let Some(metadata) = probed.metadata.get() {
        collect_metadata(metadata, &mut embedded);
    }
    let mut format = probed.format;
    collect_metadata(format.metadata(), &mut embedded);
    let track = format.default_track().context("No audio track found.")?;
    let id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or_default();
    let frames = track.codec_params.n_frames.filter(|frames| *frames > 0);
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut signal = Signal::default();
    let mut samples = None;
    let mut rate = sample_rate;
    loop {
        check_cancel(cancel)?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != id {
            continue;
        }
        let data = decoder.decode(&packet)?;
        let spec = *data.spec();
        rate = spec.rate;
        ensure!(
            rate > 0 && spec.channels.count() > 0,
            "Invalid audio stream format."
        );
        let buffer =
            samples.get_or_insert_with(|| SampleBuffer::<f32>::new(data.capacity() as u64, spec));
        if buffer.capacity() < data.capacity() * spec.channels.count() {
            *buffer = SampleBuffer::new(data.capacity() as u64, spec);
        }
        buffer.copy_interleaved_ref(data);
        signal.ingest(buffer.samples(), spec.channels.count(), rate);
        if frames.is_some() && signal.frames >= u64::from(rate) * 20 {
            break;
        }
    }
    ensure!(signal.frames > 0 && rate > 0, "No audio to decode.");
    collect_metadata(format.metadata(), &mut embedded);
    let seconds = frames.unwrap_or(signal.frames) as f64 / f64::from(rate);
    let mut tags = categories(&source.path, &source.scope.path, &embedded);
    let words = tokens(&format!(
        "{} {}",
        relative_name(&source.path, &source.scope.path),
        embedded.text.join(" ")
    ));
    tags.push(length_tag(seconds));
    tags.push(signal.brightness(rate));
    tags.push(signal.shape(seconds, &words));
    Ok(Analysis {
        revision: ANALYSIS_REVISION,
        seconds: Some(seconds),
        tags,
        error: None,
    })
}

pub fn unavailable(source: &Source, error: &anyhow::Error) -> Analysis {
    Analysis {
        revision: ANALYSIS_REVISION,
        tags: categories(&source.path, &source.scope.path, &MetadataWords::default()),
        error: Some(format!("{error:#}")),
        seconds: None,
    }
}

pub fn decode_preview(source: &Source) -> Result<impl rodio::Source + Send + 'static> {
    let input = AudioInput::open(source)?;
    let length = input.length;
    Ok(rodio::Decoder::builder()
        .with_data(BufReader::new(input))
        .with_byte_len(length)
        .with_seekable(true)
        .build()?)
}

struct AudioInput {
    file: File,
    length: u64,
    empty_wav_lists: Vec<u64>,
}

impl AudioInput {
    fn open(source: &Source) -> Result<Self> {
        let file = source.open()?;
        let length = file.metadata()?.len();
        let empty_wav_lists = if matches!(extension(&source.path).as_str(), "wav" | "wave") {
            empty_wav_list_offsets(&file, length)?
        } else {
            Vec::new()
        };
        Ok(Self {
            file,
            length,
            empty_wav_lists,
        })
    }
}

fn empty_wav_list_offsets(file: &File, length: u64) -> io::Result<Vec<u64>> {
    let mut offsets = Vec::new();
    if length < 12 {
        return Ok(offsets);
    }
    let mut header = [0u8; 12];
    file.read_exact_at(&mut header, 0)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Ok(offsets);
    }
    let riff_end = length.min(8 + u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())));
    let mut position = 12;
    let mut chunk = [0u8; 8];
    while position + 8 <= riff_end {
        file.read_exact_at(&mut chunk, position)?;
        let size = u64::from(u32::from_le_bytes(chunk[4..].try_into().unwrap()));
        if &chunk[..4] == b"LIST" && size == 0 {
            offsets.push(position);
        }
        position += 8 + size + (size & 1);
    }
    Ok(offsets)
}

impl Read for AudioInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.empty_wav_lists.is_empty() {
            return self.file.read(buffer);
        }
        let start = self.file.stream_position()?;
        let read = self.file.read(buffer)?;
        let end = start + read as u64;
        for &offset in &self.empty_wav_lists {
            // Expose an empty LIST as padding to the decoders without rewriting the WAV.
            for position in start.max(offset)..end.min(offset + 4) {
                buffer[(position - start) as usize] = b"JUNK"[(position - offset) as usize];
            }
        }
        Ok(read)
    }
}

impl Seek for AudioInput {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

impl MediaSource for AudioInput {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}

fn collect_metadata(metadata: Metadata<'_>, values: &mut MetadataWords) {
    use symphonia::core::meta::StandardTagKey as Tag;
    if let Some(revision) = metadata.current() {
        for tag in revision.tags().iter().take(64) {
            let field = tag.key.to_ascii_lowercase();
            let value = || -> String { tag.value.to_string().chars().take(250).collect() };
            if tag.std_key == Some(Tag::Genre) || field == "genre" {
                values.genres.push(value());
            } else if matches!(field.as_str(), "instrument" | "instruments") {
                values.instruments.push(value());
            } else if matches!(
                tag.std_key,
                Some(
                    Tag::Album
                        | Tag::Comment
                        | Tag::ContentGroup
                        | Tag::Description
                        | Tag::TrackTitle
                )
            ) || matches!(field.as_str(), "keywords" | "category")
            {
                values.text.push(value());
            }
        }
    }
}

fn relative_name(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    format!(
        "{} {}",
        crate::scope::name(root),
        relative.to_string_lossy()
    )
}

fn length_tag(seconds: f64) -> String {
    let length = if seconds < 1.0 {
        "1초 미만"
    } else if seconds < 8.0 {
        "1–8초"
    } else {
        "8초 이상"
    };
    format!("길이 · {length}")
}

#[derive(Default)]
struct Signal {
    frames: u64,
    energy: f64,
    delta: f64,
    previous: Vec<f64>,
    envelope: Vec<f64>,
    bin_energy: f64,
    bin_count: u32,
}

impl Signal {
    fn ingest(&mut self, buffer: &[f32], channels: usize, rate: u32) {
        let limit = u64::from(rate) * 20;
        self.previous.resize(channels, 0.0);
        for frame in buffer.chunks_exact(channels) {
            self.frames += 1;
            if self.frames > limit {
                continue;
            }
            let mut power = 0.0;
            for (sample, previous) in frame.iter().zip(&mut self.previous) {
                let sample = if sample.is_finite() {
                    f64::from(*sample)
                } else {
                    0.0
                };
                power += sample * sample;
                self.delta += (sample - *previous).powi(2);
                *previous = sample;
            }
            self.energy += power;
            self.bin_energy += power / channels as f64;
            self.bin_count += 1;
            if self.bin_count >= (rate / 50).max(1) {
                self.envelope
                    .push((self.bin_energy / f64::from(self.bin_count)).sqrt());
                self.bin_count = 0;
                self.bin_energy = 0.0;
            }
        }
    }

    fn brightness(&self, rate: u32) -> String {
        let label = if self.energy < 0.0001 {
            "미분류"
        } else {
            let ratio = ((self.delta / self.energy).sqrt() / 2.0).clamp(0.0, 1.0);
            let hz = ratio.asin() * f64::from(rate) / std::f64::consts::PI;
            if hz < 700.0 {
                "어두움"
            } else if hz > 2500.0 {
                "밝음"
            } else {
                "중간"
            }
        };
        format!("밝기 · {label}")
    }

    fn shape(&self, seconds: f64, words: &str) -> String {
        let loop_name = ["loop", "loops", "drumloop", "drumloops", "루프"]
            .iter()
            .any(|value| found(words, value));
        let shot_name = ["oneshot", "oneshots", "one shot", "one shots", "원샷"]
            .iter()
            .any(|value| found(words, value));
        let peak = self.envelope.iter().copied().fold(0.0f64, f64::max);
        let first = self.envelope.iter().position(|power| *power > peak * 0.25);
        let last = self.envelope.iter().rposition(|power| *power > peak * 0.25);
        let short_hit = first
            .zip(last)
            .is_some_and(|(start, end)| start < 8 && end - start < 21);
        let periodic = seconds >= 1.2 && self.repeated_envelope();
        let label = if loop_name && shot_name || self.energy < 0.0001 {
            "미분류"
        } else if loop_name && seconds >= 0.6 || !shot_name && periodic {
            "루프 후보"
        } else if shot_name && !periodic || !loop_name && (seconds < 0.7 || short_hit) {
            "원샷 후보"
        } else {
            "미분류"
        };
        format!("형태 · {label}")
    }

    fn repeated_envelope(&self) -> bool {
        let mean = self.envelope.iter().sum::<f64>() / self.envelope.len().max(1) as f64;
        let centered: Vec<f64> = self.envelope.iter().map(|v| v - mean).collect();
        if centered.iter().map(|v| v * v).sum::<f64>() < 0.002 {
            return false;
        }
        let max_lag = 100.min(centered.len() / 3);
        (14..=max_lag).any(|lag| {
            let (cross, left, right) = centered
                .iter()
                .zip(centered.iter().skip(lag))
                .fold((0.0, 0.0, 0.0), |(cross, left, right), (a, b)| {
                    (cross + a * b, left + a * a, right + b * b)
                });
            let norm = (left * right).sqrt();
            norm > 0.0 && cross / norm > 0.72
        })
    }
}
