use crossbeam_channel::{Sender, unbounded};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send>;

pub struct Worker {
    jobs: Option<Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start() -> std::io::Result<Self> {
        let (jobs, queue) = unbounded::<Job>();
        let thread = thread::Builder::new().name("aster-desktop".into()).spawn(move || {
            for job in queue {
                job();
            }
        })?;
        Ok(Self { jobs: Some(jobs), thread: Some(thread) })
    }

    pub fn send(&self, job: impl FnOnce() + Send + 'static) {
        if self.jobs.as_ref().is_some_and(|jobs| jobs.send(Box::new(job)).is_err()) {
            tracing::warn!("desktop worker is unavailable");
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
