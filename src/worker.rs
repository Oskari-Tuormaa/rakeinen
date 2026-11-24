use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use eframe::egui;
use image::RgbaImage;

use crate::engine::{self, Region, Settings};

pub struct Output {
    pub generation: u64,
    /// The processed pixels of `region`.
    pub image: RgbaImage,
    pub region: Region,
    /// Whether `region` is the whole processed image.
    pub full: bool,
    /// The pixel size the result was made with, i.e. how many source pixels each of its
    /// pixels stands for.
    pub pixel_size: u32,
}

struct Job {
    generation: u64,
    source: Arc<RgbaImage>,
    settings: Settings,
    region: Option<Region>,
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
                let (w, h) = engine::processed_size(&job.source, &job.settings);
                let region = job.region.map_or(Region::full(w, h), |r| r.clamped(w, h));
                let processed =
                    engine::process_region(&job.source, &job.settings, region, &is_stale);
                if let Some(image) = processed {
                    let output = Output {
                        generation: job.generation,
                        image,
                        region,
                        full: region.covers(w, h),
                        pixel_size: job.settings.preprocess.pixel_size.max(1),
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

    /// Queues a job and returns its generation, superseding any earlier job. With a `region`,
    /// only that part of the processed image is computed.
    pub fn submit(
        &mut self,
        source: Arc<RgbaImage>,
        settings: Settings,
        region: Option<Region>,
    ) -> u64 {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        let _ = self.jobs.send(Job {
            generation: self.generation,
            source,
            settings,
            region,
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
