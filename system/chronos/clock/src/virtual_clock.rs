// SPDX-License-Identifier: MulanPSL-2.0

#[cfg(target_os = "linux")]
use crate::config::{ChronosConfigError, ReferenceConfig, ReferenceSourceKind};
#[cfg(target_os = "linux")]
use crate::reference::{LinuxSystemReference, ReferenceError, read_linux_monotonic_ns};

/// One best-effort reading of the configured reference-time scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualReferenceTime {
    pub timestamp_ns: i64,
    pub generation: u64,
    pub generation_changed: bool,
    pub uncertainty_ns: u64,
    pub available: bool,
    pub verified: bool,
    pub reason: String,
}

/// One local monotonic reading for durations, deadlines, and status age.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualSteadyTime {
    pub timestamp_ns: u64,
}

/// A reference reading paired with the midpoint of its monotonic bracket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualClockSnapshot {
    pub reference: VirtualReferenceTime,
    pub steady: VirtualSteadyTime,
}

/// Read-only virtual clock backed by the explicitly configured reference.
#[derive(Debug)]
#[cfg(target_os = "linux")]
pub struct VirtualClock {
    reference: LinuxSystemReference,
}

#[cfg(target_os = "linux")]
impl VirtualClock {
    /// Validate and construct the selected reference-clock implementation.
    pub fn new(config: ReferenceConfig) -> Result<Self, ChronosConfigError> {
        match config.kind {
            ReferenceSourceKind::LinuxSystem => Ok(Self {
                reference: LinuxSystemReference::new(config)?,
            }),
        }
    }

    /// Sample reference time and report its paired monotonic midpoint.
    ///
    /// Reference discontinuities remain visible and advance generation; they
    /// are never hidden by synthesizing a replacement timeline from steady
    /// time. Quality failure is returned as metadata unless the clock read
    /// itself cannot be completed.
    pub fn snapshot(&mut self) -> Result<VirtualClockSnapshot, ReferenceError> {
        let (sample, status) = self.reference.sample_with_status()?;
        Ok(VirtualClockSnapshot {
            steady: VirtualSteadyTime {
                timestamp_ns: sample.steady_received_ns,
            },
            reference: VirtualReferenceTime {
                timestamp_ns: sample.reference_timestamp_ns,
                generation: sample.generation,
                generation_changed: sample.generation_changed,
                uncertainty_ns: sample.uncertainty_ns,
                available: status.available,
                verified: status.verified,
                reason: status.reason,
            },
        })
    }

    /// Read reference time with quality metadata, discarding only its pair.
    pub fn reference_time(&mut self) -> Result<VirtualReferenceTime, ReferenceError> {
        Ok(self.snapshot()?.reference)
    }

    /// Read local steady time without depending on reference-clock health.
    pub fn steady_time(&self) -> Result<VirtualSteadyTime, ReferenceError> {
        Ok(VirtualSteadyTime {
            timestamp_ns: read_linux_monotonic_ns()?,
        })
    }
}
