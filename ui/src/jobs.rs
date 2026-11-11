//! Every action that takes time runs here, never on the UI thread. One place decides what is
//! running, so a button can disable itself and say what it is doing, and one place reports the
//! result. A job is keyed by a name; starting a job that is already running does nothing.

use super::State;
use std::sync::{Arc, Mutex};

pub struct Finished {
    pub key: &'static str,
    pub result: Result<String, String>,
    pub then: Option<fn(&mut State)>,
}

#[derive(Default)]
pub struct Jobs {
    running: Vec<&'static str>,
    done: Arc<Mutex<Vec<Finished>>>,
    wake: Option<rune::Waker>,
}

impl Jobs {
    pub fn set_waker(&mut self, w: rune::Waker) { self.wake = Some(w); }
    pub fn running(&self, key: &str) -> bool { self.running.contains(&key) }

    /// Run `work` on its own thread. `then` runs on the UI thread when it succeeds.
    pub fn start(&mut self, key: &'static str, then: Option<fn(&mut State)>, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
        if self.running(key) { return; }
        self.running.push(key);
        let (done, wake) = (self.done.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let result = work();
            done.lock().unwrap().push(Finished { key, result, then });
            if let Some(w) = wake { w(); }
        });
    }

    /// Finished jobs, oldest first; they stop counting as running.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        let f = std::mem::take(&mut *self.done.lock().unwrap());
        self.running.retain(|k| !f.iter().any(|x| x.key == *k));
        f
    }
}

/// Run a command to completion and turn a failure into its message.
pub fn run(cmd: &mut std::process::Command, what: &str) -> Result<(), String> {
    let o = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if o.status.success() { Ok(()) } else {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        Err(format!("{what} failed{}", if err.is_empty() { String::new() } else { format!(": {err}") }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_job_runs_once_and_reports() {
        let mut j = Jobs::default();
        j.start("x", None, || { std::thread::sleep(std::time::Duration::from_millis(50)); Ok("done".into()) });
        assert!(j.running("x"));
        j.start("x", None, || Err("second copy must not run".into()));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let f = j.take_finished();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].result, Ok("done".to_string()));
        assert!(!j.running("x"));
    }
}
