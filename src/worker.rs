use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use eframe::egui;
use image::RgbaImage;

use crate::engine::{self, Settings};

pub struct Output {
    pub generation: u64,
    pub image: RgbaImage,
}

struct Job {
    generation: u64,
    source: Arc<RgbaImage>,
    settings: Settings,
}

/// Runs dithering on a background thread, always working on the newest submitted job.
pub struct Worker {
    jobs: mpsc::Sender<Job>,
    results: mpsc::Receiver<Output>,
    latest: Arc<AtomicU64>,
    generation: u64,
}

impl Worker {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (result_tx, results) = mpsc::channel::<Output>();
        let latest = Arc::new(AtomicU64::new(0));

        let latest_for_thread = latest.clone();
        thread::spawn(move || {
            while let Ok(mut job) = job_rx.recv() {
                while let Ok(next) = job_rx.try_recv() {
                    job = next;
                }
                let is_stale = || latest_for_thread.load(Ordering::Relaxed) != job.generation;
                if is_stale() {
                    continue;
                }
                if let Some(image) = engine::process(&job.source, &job.settings, &is_stale) {
                    let output = Output {
                        generation: job.generation,
                        image,
                    };
                    if result_tx.send(output).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            }
        });

        Self {
            jobs,
            results,
            latest,
            generation: 0,
        }
    }

    /// Queues a job and returns its generation, superseding any earlier job.
    pub fn submit(&mut self, source: Arc<RgbaImage>, settings: Settings) -> u64 {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        let _ = self.jobs.send(Job {
            generation: self.generation,
            source,
            settings,
        });
        self.generation
    }

    /// Invalidates in-flight work, e.g. when the source image is closed.
    pub fn cancel(&mut self) {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
    }

    /// The newest finished result, if any arrived since the last call.
    pub fn poll(&self) -> Option<Output> {
        let mut newest = None;
        while let Ok(output) = self.results.try_recv() {
            newest = Some(output);
        }
        newest
    }
}
