// SPDX-License-Identifier: MulanPSL-2.0

#![cfg(target_os = "linux")]

use robonix_chronos_clock::{
    ChronosConfigError, ReferenceConfig, ReferenceSourceKind, VirtualClock,
};

/// Select the Linux reference without service configuration or initialization.
fn config() -> ReferenceConfig {
    ReferenceConfig {
        kind: ReferenceSourceKind::LinuxSystem,
        max_uncertainty_ns: 1_000_000,
        max_status_age_ns: 2_000_000_000,
        max_clock_step_ns: 100_000_000,
    }
}

#[test]
/// Standalone consumers receive a reference reading paired with local steady time.
fn standalone_clock_pairs_reference_and_steady() {
    let mut clock = VirtualClock::new(config()).unwrap();
    let before = clock.steady_time().unwrap();
    let snapshot = clock.snapshot().unwrap();
    let after = clock.steady_time().unwrap();
    assert!(snapshot.reference.timestamp_ns > 0);
    assert!(snapshot.reference.uncertainty_ns > 0);
    assert!(!snapshot.reference.verified);
    assert_eq!(snapshot.reference.generation, 1);
    assert!(snapshot.steady.timestamp_ns >= before.timestamp_ns);
    assert!(snapshot.steady.timestamp_ns <= after.timestamp_ns);
}

#[test]
/// Invalid reference limits retain the configuration error used by Chronos.
fn invalid_limits_are_rejected_before_clock_reading() {
    let mut config = config();
    config.max_status_age_ns = 0;
    assert!(matches!(
        VirtualClock::new(config),
        Err(ChronosConfigError::InvalidReference(_))
    ));
}
