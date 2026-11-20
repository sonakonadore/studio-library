use eframe::egui::Context;
use rodio::{OutputStream, OutputStreamBuilder, Sink};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use studio_library::audio::decode_preview;
use studio_library::scope::Source;

enum Command {
    Play(u64, Source, f32),
    Stop(u64),
    Volume(f32),
}

pub enum Playback {
    Playing(PathBuf),
    Stopped,
    Error(String),
}

pub struct Player {
    send: Sender<Command>,
    events: Receiver<(u64, Playback)>,
    latest: Arc<AtomicU64>,
}

impl Player {
    pub fn new(ctx: Context) -> Self {
        let (send, commands) = mpsc::channel();
        let (events, recv) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        let worker_latest = Arc::clone(&latest);
        std::thread::spawn(move || {
            let mut stream: Option<OutputStream> = None;
            let mut sink: Option<(u64, Sink)> = None;
            let current = |request| worker_latest.load(Ordering::Relaxed) == request;
            let notify = |request, event| {
                let _ = events.send((request, event));
                ctx.request_repaint();
            };
            loop {
                match commands.recv_timeout(Duration::from_millis(120)) {
                    Ok(Command::Play(request, source, volume)) if current(request) => {
                        drop(sink.take());
                        let result = (|| -> anyhow::Result<_> {
                            let decoded = decode_preview(&source)?;
                            if stream.is_none() {
                                let mut output = OutputStreamBuilder::open_default_stream()?;
                                output.log_on_drop(false);
                                stream = Some(output);
                            }
                            Ok(decoded)
                        })();
                        if !current(request) {
                            continue;
                        }
                        match result {
                            Ok(decoded) => {
                                let output = Sink::connect_new(stream.as_ref().unwrap().mixer());
                                output.set_volume(volume);
                                output.append(decoded);
                                sink = Some((request, output));
                                notify(request, Playback::Playing(source.path));
                            }
                            Err(error) => {
                                notify(request, Playback::Error(format!("재생 실패 · {error:#}")));
                            }
                        }
                    }
                    Ok(Command::Stop(request)) if current(request) => {
                        drop(sink.take());
                        notify(request, Playback::Stopped);
                    }
                    Ok(Command::Play(..) | Command::Stop(_)) => {}
                    Ok(Command::Volume(volume)) => {
                        if let Some((_, sink)) = &sink {
                            sink.set_volume(volume);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some((request, output)) = &sink
                            && output.empty()
                        {
                            notify(*request, Playback::Stopped);
                            drop(sink.take());
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self {
            send,
            events: recv,
            latest,
        }
    }

    pub fn play(&self, source: Source, volume: f32) {
        let request = self.latest.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let _ = self.send.send(Command::Play(request, source, volume));
    }

    pub fn stop(&self) {
        let request = self.latest.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let _ = self.send.send(Command::Stop(request));
    }

    pub fn volume(&self, volume: f32) {
        let _ = self.send.send(Command::Volume(volume));
    }

    pub fn events(&self) -> impl Iterator<Item = Playback> + '_ {
        self.events.try_iter().filter_map(|(request, event)| {
            (self.latest.load(Ordering::Relaxed) == request).then_some(event)
        })
    }
}
