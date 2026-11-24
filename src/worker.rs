use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use eframe::egui;
use image::RgbaImage;

use crate::engine::{self, Settings};

/// Target size of the longer side of a reduced-quality preview. Images up to twice this size
/// are not reduced at all.
const PREVIEW_SIDE: u32 = 768;

/// By how much to shrink an image of this size for a fast, lower-resolution preview.
pub fn preview_factor(width: u32, height: u32) -> u32 {
    (width.max(height) / PREVIEW_SIDE).max(1)
}

pub struct Output {
    pub generation: u64,
    pub image: RgbaImage,
    /// Whether the image was produced at reduced resolution.
    pub preview: bool,
}

struct Job {
    generation: u64,
    source: Arc<RgbaImage>,
    settings: Settings,
    preview: bool,
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
                let mut settings = job.settings.clone();
                let factor = if job.preview {
                    preview_factor(job.source.width(), job.source.height())
                } else {
                    1
                };
                settings.preprocess.pixel_size *= factor;
                if let Some(image) = engine::process(&job.source, &settings, &is_stale) {
                    let output = Output {
                        generation: job.generation,
                        image,
                        preview: factor > 1,
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
    pub fn submit(&mut self, source: Arc<RgbaImage>, settings: Settings, preview: bool) -> u64 {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        let _ = self.jobs.send(Job {
            generation: self.generation,
            source,
            settings,
            preview,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_images_get_no_preview_reduction() {
        assert_eq!(preview_factor(0, 0), 1);
        assert_eq!(preview_factor(800, 600), 1);
        assert_eq!(preview_factor(1500, 900), 1);
    }

    #[test]
    fn large_images_are_reduced_towards_the_preview_size() {
        assert_eq!(preview_factor(1536, 100), 2);
        assert_eq!(preview_factor(3000, 4000), 5);
    }
}
