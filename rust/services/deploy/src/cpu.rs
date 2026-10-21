//! CPU admission (`cpuadmission.ts`): smoothed CPU use with hysteresis. It refuses new processes
//! while the machine is busy and never evicts running hunts.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;

use crate::capacity::{DeployError, Reason};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CpuStatus {
    pub percent: Option<f64>,
    pub blocked: bool,
    pub limit: f64,
    pub resume: f64,
}

#[derive(Debug)]
struct State {
    previous: Option<(f64, f64)>,
    percent: Option<f64>,
    blocked: bool,
}

#[derive(Clone)]
pub struct CpuAdmission {
    limit: f64,
    resume: f64,
    state: Arc<Mutex<State>>,
}

impl CpuAdmission {
    pub fn new(limit: f64, resume: f64) -> Result<Self, String> {
        if !(limit > 0.0 && limit <= 100.0 && resume >= 0.0 && resume < limit) {
            return Err("Invalid CPU admission thresholds".into());
        }
        Ok(Self {
            limit,
            resume,
            state: Arc::new(Mutex::new(State {
                previous: None,
                percent: None,
                blocked: false,
            })),
        })
    }

    /// One reading of the machine's cumulative idle and total CPU time.
    pub fn sample(&self, idle: f64, total: f64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let old = state.previous.replace((idle, total));
        let Some((old_idle, old_total)) = old else {
            return;
        };
        if total <= old_total || idle < old_idle {
            return;
        }
        let used = (100.0 * (1.0 - (idle - old_idle) / (total - old_total))).clamp(0.0, 100.0);
        let percent = state.percent.map_or(used, |p| p * 0.5 + used * 0.5);
        state.percent = Some(percent);
        if percent >= self.limit {
            state.blocked = true;
        } else if percent <= self.resume {
            state.blocked = false;
        }
    }

    pub fn status(&self) -> CpuStatus {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        CpuStatus {
            percent: state.percent,
            blocked: state.blocked,
            limit: self.limit,
            resume: self.resume,
        }
    }

    pub fn assert_available(&self) -> Result<(), DeployError> {
        if self.status().blocked {
            Err(DeployError::Capacity(Reason::Cpu))
        } else {
            Ok(())
        }
    }

    /// Samples the machine now and every two seconds.
    pub fn start_sampling(&self) {
        if let Some((idle, total)) = crate::system::cpu_times() {
            self.sample(idle, total);
        }
        let this = self.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let mut every = tokio::time::interval(Duration::from_secs(2));
                every.tick().await;
                loop {
                    every.tick().await;
                    if let Some((idle, total)) = crate::system::cpu_times() {
                        this.sample(idle, total);
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_sustained_load_and_resumes_only_below_the_low_watermark() {
        let cpu = CpuAdmission::new(80.0, 65.0).unwrap();
        cpu.sample(0.0, 0.0);
        cpu.sample(5.0, 100.0);
        assert!(cpu.assert_available().is_err());
        cpu.sample(30.0, 200.0);
        assert!(cpu.assert_available().is_err());
        cpu.sample(90.0, 300.0);
        cpu.assert_available().unwrap();
        assert!(!cpu.status().blocked);
    }

    #[test]
    fn counters_resetting_do_not_manufacture_overload() {
        let cpu = CpuAdmission::new(80.0, 65.0).unwrap();
        cpu.sample(100.0, 200.0);
        cpu.sample(0.0, 0.0);
        cpu.sample(80.0, 100.0);
        cpu.assert_available().unwrap();
        assert!(CpuAdmission::new(65.0, 80.0).is_err());
    }
}
