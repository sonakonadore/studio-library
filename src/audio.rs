use crate::scope::{Source, check_cancel, extension, key};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{Metadata, MetadataOptions},
    probe::Hint,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Analysis {
    pub seconds: Option<f64>,
    pub tags: Vec<String>,
    pub error: Option<String>,
}

const INSTRUMENTS: &[(&str, &str)] = &[
    ("킥", "kick bassdrum 킥"),
    ("스네어", "snare clap rimshot 스네어 클랩"),
    ("하이햇", "hihat hi-hat hi_hat 하이햇"),
    ("퍼커션", "perc percussion shaker tambourine conga"),
    ("드럼", "drum drums drumkit 드럼"),
    ("베이스", "bass 808 베이스"),
    ("신스", "synth synths lead pad arpeggio 신스"),
    ("피아노·건반", "keys keyboard piano rhodes organ 피아노"),
    ("기타", "guitar guitars gtr 기타"),
    ("보컬", "vocal vocals vox voice acapella 보컬"),
    ("현악기", "strings string violin cello"),
    ("효과음", "fx sfx riser impact foley"),
];

const GENRES: &[(&str, &str)] = &[
    (
        "힙합",
        "hiphop hip-hop hip_hop boombap boom-bap boom_bap 힙합",
    ),
    ("트랩", "trap drill 트랩"),
    ("하우스", "house deep-house 하우스"),
    ("테크노", "techno 테크노"),
    ("드럼 앤 베이스", "dnb drum-and-bass jungle"),
    ("R&B", "rnb r&b r-b rhythm-and-blues"),
    ("재즈", "jazz 재즈"),
    ("로파이", "lofi lo-fi lo_fi 로파이"),
    ("팝", "pop kpop k-pop 팝"),
    ("록", "rock punk metal 록"),
    ("앰비언트", "ambient cinematic"),
    ("UK 개러지", "ukg uk-garage"),
];

pub fn analyze(source: &Source, cancel: &AtomicBool) -> Result<Analysis> {
    check_cancel(cancel)?;
    let file = source.open()?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&extension(&source.path));
    let mut probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut embedded = Vec::new();
    if let Some(metadata) = probed.metadata.get() {
        collect_metadata(metadata, &mut embedded);
    }
    let mut format = probed.format;
    collect_metadata(format.metadata(), &mut embedded);
    let track = format.default_track().context("오디오 트랙 없음")?;
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
            "오디오 스트림 형식 오류"
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
    ensure!(signal.frames > 0 && rate > 0, "디코딩할 오디오가 없습니다.");
    collect_metadata(format.metadata(), &mut embedded);
    let seconds = frames.unwrap_or(signal.frames) as f64 / f64::from(rate);
    let mut tags = categories(&source.path, &source.scope.path, &embedded);
    let words = tokens(&format!(
        "{} {}",
        relative_name(&source.path, &source.scope.path),
        embedded.join(" ")
    ));
    tags.push(length_tag(seconds));
    tags.push(signal.brightness(rate));
    tags.push(signal.shape(seconds, &words));
    Ok(Analysis {
        seconds: Some(seconds),
        tags,
        error: None,
    })
}

pub fn unavailable(source: &Source, error: &anyhow::Error) -> Analysis {
    Analysis {
        tags: categories(&source.path, &source.scope.path, &[]),
        error: Some(format!("{error:#}")),
        seconds: None,
    }
}

fn collect_metadata(metadata: Metadata<'_>, values: &mut Vec<String>) {
    use symphonia::core::meta::StandardTagKey as Tag;
    if let Some(revision) = metadata.current() {
        for tag in revision.tags().iter().take(64) {
            if matches!(
                tag.std_key,
                Some(
                    Tag::Album
                        | Tag::Comment
                        | Tag::ContentGroup
                        | Tag::Description
                        | Tag::Genre
                        | Tag::PodcastCategory
                        | Tag::PodcastKeywords
                        | Tag::TrackTitle
                )
            ) || matches!(
                tag.key.to_ascii_lowercase().as_str(),
                "instrument" | "genre" | "keywords" | "category"
            ) {
                values.push(tag.value.to_string().chars().take(250).collect());
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

fn tokens(value: &str) -> String {
    let normalized = key(value);
    let words = normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty());
    format!(" {} ", words.collect::<Vec<_>>().join(" "))
}

fn found(words: &str, value: &str) -> bool {
    words.contains(&tokens(value))
}

fn category<'a>(primary: &str, secondary: &str, options: &[(&'a str, &str)]) -> &'a str {
    for words in [primary, secondary] {
        let mut matches = options
            .iter()
            .filter(|(_, aliases)| aliases.split_whitespace().any(|alias| found(words, alias)));
        if let Some((label, _)) = matches.next() {
            if matches.next().is_none() {
                return label;
            }
            return "미분류";
        }
    }
    "미분류"
}

fn categories(path: &Path, root: &Path, embedded: &[String]) -> Vec<String> {
    let metadata = tokens(&embedded.join(" "));
    let file = tokens(&crate::scope::name(path));
    let parent = tokens(&relative_name(path.parent().unwrap_or(root), root));
    let mut tags = Vec::new();
    for (prefix, options) in [("악기", INSTRUMENTS), ("장르", GENRES)] {
        let label = if options
            .iter()
            .any(|(_, aliases)| aliases.split_whitespace().any(|alias| found(&file, alias)))
        {
            category(&file, "", options)
        } else {
            category(&metadata, &parent, options)
        };
        tags.push(format!("{prefix} · {label}"));
    }
    tags
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
        let loop_name = ["loop", "루프"].iter().any(|value| found(words, value));
        let shot_name = ["oneshot", "one-shot", "원샷"]
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
