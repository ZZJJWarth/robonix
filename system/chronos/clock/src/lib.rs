// SPDX-License-Identifier: MulanPSL-2.0

//! Chronos reference and steady clock readings for in-process consumers.
//!
//! Clock reads require no logging, discovery, code generation, or running
//! service. Linux reference readings preserve quality and discontinuities;
//! steady readings remain independent of reference-clock health.

mod config;
mod reference;
mod virtual_clock;

pub use config::{ChronosConfigError, ReferenceConfig, ReferenceSourceKind};
#[cfg(target_os = "linux")]
pub use reference::LinuxSystemReference;
pub use reference::{ReferenceError, ReferenceSample, ReferenceStatus};
#[cfg(target_os = "linux")]
pub use virtual_clock::VirtualClock;
pub use virtual_clock::{VirtualClockSnapshot, VirtualReferenceTime, VirtualSteadyTime};

#[cfg(target_os = "linux")]
static CLOCK: std::sync::Mutex<Option<VirtualClock>> = std::sync::Mutex::new(None);

/// Read a paired snapshot from the process-local clock with default reference limits.
///
/// Initialization and clock reads never log. Poisoned ownership, unsupported
/// platforms, and failed reads return errors; no timestamp is synthesized.
/// Generations belong to this process-local observer, not to a remote daemon.
pub fn snapshot() -> std::io::Result<VirtualClockSnapshot> {
    #[cfg(target_os = "linux")]
    {
        let mut clock = CLOCK
            .lock()
            .map_err(|_| std::io::Error::other("Chronos clock lock poisoned"))?;
        if clock.is_none() {
            *clock =
                Some(VirtualClock::new(ReferenceConfig::default()).map_err(std::io::Error::other)?);
        }
        clock
            .as_mut()
            .expect("initialized clock")
            .snapshot()
            .map_err(std::io::Error::other)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Chronos clock reads require Linux",
        ))
    }
}

/// Read reference time and quality without depending on a running service.
pub fn reference_time() -> std::io::Result<VirtualReferenceTime> {
    Ok(snapshot()?.reference)
}

/// Read local steady time independently of reference health or ownership.
pub fn steady_time() -> std::io::Result<VirtualSteadyTime> {
    #[cfg(target_os = "linux")]
    {
        Ok(VirtualSteadyTime {
            timestamp_ns: reference::read_linux_monotonic_ns().map_err(std::io::Error::other)?,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Chronos clock reads require Linux",
        ))
    }
}
