use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use boss_sensor_platform::{self as platform, Camera, Frame, WindowTarget};
use boss_sensor_vision::{Observation, Vision};

use crate::rules::{self, Matches, Rules};

const CAPTURE_INTERVAL: Duration = Duration::from_millis(33);
const ANALYSIS_INTERVAL: Duration = Duration::from_millis(150);
const RETRY_INTERVAL: Duration = Duration::from_millis(800);

#[derive(Clone, PartialEq)]
pub struct Job {
    pub camera: String,
    pub analyze: bool,
    pub identify: bool,
    pub policy: Policy,
}

#[derive(Clone, Default, PartialEq)]
pub struct Policy {
    pub second_face: bool,
    pub templates: Vec<Vec<f32>>,
    pub notify: bool,
    pub target: Option<WindowTarget>,
}

pub struct Captured {
    pub sequence: u64,
    pub at: Instant,
    pub frame: Frame,
}

pub struct Reading {
    pub at: Instant,
    pub observation: Observation,
}

pub struct Event {
    pub matched: Matches,
    pub at: Instant,
    pub notification: Option<Result<(), String>>,
    pub switched: Option<Result<(), String>>,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub image: Option<Arc<Captured>>,
    pub reading: Option<Arc<Reading>>,
    pub camera_error: Option<String>,
    pub vision_error: Option<String>,
    pub event: Option<Arc<Event>>,
}

#[derive(Default)]
struct State {
    job: Option<Job>,
    generation: u64,
    snapshot: Snapshot,
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

pub struct Engine(Arc<Shared>);

impl Engine {
    pub fn start() -> std::io::Result<Self> {
        let engine = Self(Arc::default());
        let capture = Arc::clone(&engine.0);
        thread::Builder::new()
            .name("camera".into())
            .spawn(move || capture_loop(&capture))?;
        let analysis = Arc::clone(&engine.0);
        thread::Builder::new()
            .name("vision".into())
            .spawn(move || vision_loop(&analysis))?;
        Ok(engine)
    }

    pub fn configure(&self, job: Option<Job>) {
        let mut state = self.0.state.lock().unwrap();
        if state.job != job {
            state.job = job;
            state.generation += 1;
            state.snapshot = Snapshot::default();
            self.0.changed.notify_all();
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.0.state.lock().unwrap().snapshot.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().stopped = true;
        self.0.changed.notify_all();
    }
}

fn capture_loop(shared: &Shared) {
    let mut open_camera: Option<(String, Camera)> = None;
    let mut sequence = 0;

    loop {
        let (generation, job) = {
            let mut state = shared.state.lock().unwrap();
            while state.job.is_none() && !state.stopped {
                drop(state);
                open_camera = None;
                state = shared.state.lock().unwrap();
                state = shared
                    .changed
                    .wait_while(state, |state| state.job.is_none() && !state.stopped)
                    .unwrap();
            }
            if state.stopped {
                return;
            }
            (state.generation, state.job.clone().unwrap())
        };

        let started = Instant::now();
        let result = (|| {
            if open_camera.as_ref().is_none_or(|(id, _)| *id != job.camera) {
                open_camera = None;
                open_camera = Some((job.camera.clone(), Camera::open(&job.camera)?));
            }
            open_camera.as_mut().unwrap().1.frame()
        })();
        let failed = result.is_err();
        let mut state = shared.state.lock().unwrap();
        if state.generation == generation {
            match result {
                Ok(frame) => {
                    sequence += 1;
                    state.snapshot.image = Some(Arc::new(Captured {
                        sequence,
                        at: Instant::now(),
                        frame,
                    }));
                    state.snapshot.camera_error = None;
                }
                Err(error) => {
                    state.snapshot.image = None;
                    state.snapshot.reading = None;
                    state.snapshot.camera_error = Some(error);
                }
            }
            shared.changed.notify_all();
        }
        drop(state);

        if failed {
            open_camera = None;
        }
        let delay = if failed {
            RETRY_INTERVAL
        } else {
            CAPTURE_INTERVAL.saturating_sub(started.elapsed())
        };
        let state = shared.state.lock().unwrap();
        let _ = shared
            .changed
            .wait_timeout_while(state, delay, |state| {
                state.generation == generation && !state.stopped
            })
            .unwrap();
    }
}

fn vision_loop(shared: &Shared) {
    let mut vision = None;
    let mut last_sequence = 0;
    let mut next_analysis = Instant::now();
    let mut rules = Rules::default();
    let mut last_generation = None;
    let mut last_reading = None;

    loop {
        let (generation, job, image) = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.stopped {
                    return;
                }
                if let (Some(job), Some(image)) = (&state.job, &state.snapshot.image)
                    && job.analyze
                    && image.sequence != last_sequence
                {
                    let delay = next_analysis.saturating_duration_since(Instant::now());
                    if delay.is_zero() {
                        break (state.generation, job.clone(), Arc::clone(image));
                    }
                    let (next_state, _) = shared.changed.wait_timeout(state, delay).unwrap();
                    state = next_state;
                } else {
                    state = shared.changed.wait(state).unwrap();
                }
            }
        };

        last_sequence = image.sequence;
        let failed = {
            let analyzed = (|| {
                if vision.is_none() {
                    vision = Some(Vision::new()?);
                }
                let frame = &image.frame;
                vision.as_mut().unwrap().analyze(
                    &frame.rgb,
                    frame.width,
                    frame.height,
                    job.identify,
                )
            })();
            let failed = analyzed.is_err();
            let mut state = shared.state.lock().unwrap();
            if state.generation == generation && state.snapshot.camera_error.is_none() {
                match analyzed {
                    Ok(observation) => {
                        if last_generation != Some(generation)
                            || last_reading.is_some_and(|previous| {
                                image.at.duration_since(previous) > Duration::from_secs(2)
                            })
                        {
                            rules = Rules::default();
                        }
                        last_generation = Some(generation);
                        last_reading = Some(image.at);
                        let matched = rules.observe(rules::classify(
                            &observation,
                            job.policy.second_face,
                            job.policy.templates.iter().map(Vec::as_slice),
                        ));
                        state.snapshot.reading = Some(Arc::new(Reading {
                            at: image.at,
                            observation,
                        }));
                        state.snapshot.vision_error = None;
                        if matched.any() && image.at.elapsed() < Duration::from_secs(2) {
                            drop(state);
                            let switched = job
                                .policy
                                .target
                                .as_ref()
                                .filter(|_| job_is_current(shared, generation))
                                .map(platform::activate);
                            let notification = (job.policy.notify
                                && job_is_current(shared, generation))
                            .then(|| platform::notify(matched.label()));
                            let mut state = shared.state.lock().unwrap();
                            if state.generation == generation {
                                state.snapshot.event = Some(Arc::new(Event {
                                    matched,
                                    at: Instant::now(),
                                    notification,
                                    switched,
                                }));
                            }
                        }
                    }
                    Err(error) => {
                        rules = Rules::default();
                        state.snapshot.reading = None;
                        state.snapshot.vision_error = Some(error);
                    }
                }
            }
            failed
        };
        next_analysis = Instant::now()
            + if failed {
                RETRY_INTERVAL
            } else {
                ANALYSIS_INTERVAL
            };
    }
}

fn job_is_current(shared: &Shared, generation: u64) -> bool {
    let state = shared.state.lock().unwrap();
    state.generation == generation && !state.stopped && state.snapshot.camera_error.is_none()
}
