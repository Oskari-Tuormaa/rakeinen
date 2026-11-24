use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use eframe::egui;
use image::RgbaImage;

use crate::engine::{self, Region, Settings};

/// How far through the pipeline a job should go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// Preprocessed and snapped to the nearest palette color, without dithering.
    Quantized,
    Dithered,
}

pub struct Output {
    pub generation: u64,
    pub stage: Stage,
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
    stage: Stage,
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
                let (region, processed) = match job.stage {
                    Stage::Quantized => (
                        Region::full(w, h),
                        engine::quantized(&job.source, &job.settings, &is_stale),
                    ),
                    Stage::Dithered => {
                        let region = job.region.map_or(Region::full(w, h), |r| r.clamped(w, h));
                        let processed =
                            engine::process_region(&job.source, &job.settings, region, &is_stale);
                        (region, processed)
                    }
                };
                if let Some(image) = processed {
                    let output = Output {
                        generation: job.generation,
                        stage: job.stage,
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
    /// only that part of the dithered image is computed; it is ignored for the quantized stage.
    pub fn submit(
        &mut self,
        source: Arc<RgbaImage>,
        settings: Settings,
        stage: Stage,
        region: Option<Region>,
    ) -> u64 {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        let _ = self.jobs.send(Job {
            generation: self.generation,
            stage,
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use image::Rgba;

    use super::*;

    fn run(stage: Stage, region: Option<Region>) -> Output {
        let mut worker = Worker::spawn(egui::Context::default());
        let source = Arc::new(RgbaImage::from_pixel(32, 32, Rgba([140, 140, 140, 255])));
        worker.submit(source, Settings::default(), stage, region);
        let started = Instant::now();
        loop {
            if let Some(output) = worker.poll() {
                return output;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "worker timed out"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn distinct_colors(img: &RgbaImage) -> usize {
        let mut colors: Vec<_> = img.pixels().map(|p| p.0).collect();
        colors.sort_unstable();
        colors.dedup();
        colors.len()
    }

    #[test]
    fn quantized_stage_snaps_without_dithering() {
        let output = run(Stage::Quantized, None);
        assert_eq!(output.stage, Stage::Quantized);
        assert!(output.full);
        // Flat mid-gray snaps to one palette color, where dithering would mix two.
        assert_eq!(distinct_colors(&output.image), 1);
    }

    #[test]
    fn dithered_stage_mixes_palette_colors() {
        let output = run(Stage::Dithered, None);
        assert_eq!(output.stage, Stage::Dithered);
        assert_eq!(distinct_colors(&output.image), 2);
    }

    #[test]
    fn quantized_stage_ignores_the_region() {
        let region = Region {
            x: 4,
            y: 4,
            w: 8,
            h: 8,
        };
        let output = run(Stage::Quantized, Some(region));
        assert!(output.full);
        assert_eq!((output.image.width(), output.image.height()), (32, 32));
    }

    #[test]
    fn dithered_stage_honours_the_region() {
        let region = Region {
            x: 4,
            y: 4,
            w: 8,
            h: 8,
        };
        let output = run(Stage::Dithered, Some(region));
        assert!(!output.full);
        assert_eq!(output.region, region);
        assert_eq!((output.image.width(), output.image.height()), (8, 8));
    }
}
