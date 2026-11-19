use anyhow::Result;
use eframe::egui::Context;
use std::sync::{
    Arc,
    atomic::AtomicBool,
    mpsc::{self, Receiver},
};
use std::time::{Duration, Instant};
use studio_library::{
    catalog::SampleScan,
    copying::{CopyReport, Plan},
    projects::Scan,
};

pub enum Outcome {
    Projects(Scan),
    Samples(SampleScan),
    Plan(Plan),
    Copy(CopyReport),
}

pub enum Event {
    Progress(String),
    Done(Result<Outcome>),
}

pub struct Job {
    pub label: &'static str,
    pub progress: String,
    pub cancel: Arc<AtomicBool>,
    pub events: Receiver<Event>,
}

impl Job {
    pub fn start(
        ctx: &Context,
        label: &'static str,
        work: impl FnOnce(&AtomicBool, &mut dyn FnMut(String)) -> Result<Outcome> + Send + 'static,
    ) -> Self {
        let (send, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut previous = None::<Instant>;
            let result = work(&signal, &mut |progress| {
                if previous.is_none_or(|time| time.elapsed() >= Duration::from_millis(75)) {
                    previous = Some(Instant::now());
                    let _ = send.send(Event::Progress(progress));
                    ctx.request_repaint();
                }
            });
            let _ = send.send(Event::Done(result));
            ctx.request_repaint();
        });
        Self {
            label,
            progress: String::new(),
            cancel,
            events,
        }
    }
}
