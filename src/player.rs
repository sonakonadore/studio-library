use eframe::egui::Context;
use rodio::{OutputStream, OutputStreamBuilder, Sink};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use studio_library::audio::decode_preview;
use studio_library::scope::Source;

enum Command {
    Play(Source, f32),
    Stop,
    Volume(f32),
}

pub enum Playback {
    Playing(PathBuf),
    Stopped,
    Error(String),
}

pub struct Player {
    send: Sender<Command>,
    events: Receiver<Playback>,
}

impl Player {
    pub fn new(ctx: Context) -> Self {
        let (send, commands) = mpsc::channel();
        let (events, recv) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stream: Option<OutputStream> = None;
            let mut sink: Option<Sink> = None;
            let notify = |event| {
                let _ = events.send(event);
                ctx.request_repaint();
            };
            loop {
                match commands.recv_timeout(Duration::from_millis(120)) {
                    Ok(Command::Play(source, volume)) => {
                        drop(sink.take());
                        let result = (|| -> anyhow::Result<Sink> {
                            let decoded = decode_preview(&source)?;
                            if stream.is_none() {
                                let mut output = OutputStreamBuilder::open_default_stream()?;
                                output.log_on_drop(false);
                                stream = Some(output);
                            }
                            let sink = Sink::connect_new(stream.as_ref().unwrap().mixer());
                            sink.set_volume(volume);
                            sink.append(decoded);
                            Ok(sink)
                        })();
                        match result {
                            Ok(output) => {
                                sink = Some(output);
                                notify(Playback::Playing(source.path));
                            }
                            Err(error) => notify(Playback::Error(format!("재생 실패 · {error:#}"))),
                        }
                    }
                    Ok(Command::Stop) => {
                        drop(sink.take());
                        notify(Playback::Stopped);
                    }
                    Ok(Command::Volume(volume)) => {
                        if let Some(sink) = &sink {
                            sink.set_volume(volume);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if sink.as_ref().is_some_and(Sink::empty) {
                            drop(sink.take());
                            notify(Playback::Stopped);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self { send, events: recv }
    }

    pub fn play(&self, source: Source, volume: f32) {
        let _ = self.send.send(Command::Play(source, volume));
    }

    pub fn stop(&self) {
        let _ = self.send.send(Command::Stop);
    }

    pub fn volume(&self, volume: f32) {
        let _ = self.send.send(Command::Volume(volume));
    }

    pub fn events(&self) -> impl Iterator<Item = Playback> + '_ {
        self.events.try_iter()
    }
}
