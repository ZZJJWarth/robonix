# Chronos clock

`robonix-chronos-clock` is Chronos's in-process reference and steady clock
library. It has no dependency on Scribe, Atlas, Codegen, or gRPC and needs no
running service. Linux is currently the only implemented clock source.

For the process-local clock with default reference limits, call
`robonix_chronos_clock::reference_time()`, `steady_time()`, or `snapshot()`.
These return `std::io::Result` without logging. Reference reads share one
serialized observer so generation changes remain visible within the process;
steady reads do not acquire that observer or depend on reference availability.
Unsupported platforms, poisoned reference ownership, and read failures return
errors without substituting a timestamp. Quality metadata remains separate
from read errors: callers decide whether an unavailable reading is usable.

`reference_timestamp_ns()` returns an available, nonnegative epoch timestamp
for wire fields. It rejects read failures, unavailable quality and negative
values. Unverified Linux reference time remains usable.

Add `robonix-chronos-clock.workspace = true` to a workspace member's
dependencies and construct a `VirtualClock` with an explicit `ReferenceConfig`:

```rust
use robonix_chronos_clock::{ReferenceConfig, ReferenceSourceKind, VirtualClock};

let mut clock = VirtualClock::new(ReferenceConfig {
    kind: ReferenceSourceKind::LinuxSystem,
    max_uncertainty_ns: 1_000_000,
    max_status_age_ns: 2_000_000_000,
    max_clock_step_ns: 100_000_000,
})?;
let reference = clock.reference_time()?;
let steady = clock.steady_time()?;
```

Use reference time for business timestamps and local steady time for durations
and deadlines. `snapshot()` pairs a reference reading with its monotonic
bracket midpoint. Reference readings include uncertainty, availability,
verification, and generation changes; Linux system time remains unverified.
Read failures return errors without logging or fabricating timestamps. Steady
values are local to the host and cannot be compared across machines.

Step detection adds both readings' sampling uncertainty to `max_clock_step_ns`,
so scheduling delays within the measured brackets do not reset clock generations.
Realtime moving backward still starts a new generation.

The shared `ChronosConfigError` retains the existing configuration error
variants for compatibility with `robonix-chronos`; domain validation and YAML
configuration parsing remain in that package. `robonix-chronos` re-exports the
clock types, so existing consumers can keep their imports.

Run `cargo test -p robonix-chronos-clock` from the repository root.

## Python

The independent Python package `robonix-chronos-clock` exposes the same clock
readings through `robonix_chronos_clock`. It uses the Rust implementation inside
the Python process and has no Python runtime dependencies. The optional Cargo
feature `python` enables the bindings; Rust consumers need not enable it.

```python
import robonix_chronos_clock as clock

reading = clock.reference_time()
snapshot = clock.snapshot()
steady_ns = clock.steady_time().timestamp_ns
timestamp_ns = clock.reference_timestamp_ns()
received_at_unix = timestamp_ns / 1_000_000_000
```

`VirtualReferenceTime`, `VirtualSteadyTime`, and `VirtualClockSnapshot` expose
read-only fields matching their Rust types. Times remain integer nanoseconds.
`reference_time()` and `snapshot()` expose unavailable quality as metadata;
`reference_timestamp_ns()` rejects unavailable or negative reference time.
Read failures raise `OSError`. There is no fallback to another clock. Reference
generations belong to this extension in the current process, and steady time
must not be compared across hosts. Linux reference time remains unverified.

Build a wheel without modifying an existing virtual environment:

```bash
uvx maturin build --release --manifest-path system/chronos/clock/Cargo.toml \
  --out /tmp/chronos-wheels
```

Install the resulting wheel into the Python environment that will read time.
Wheels use the CPython 3.10+ stable ABI (`abi3`) on standard GIL-enabled CPython.
Build separately for x86_64 and aarch64; the wheel's platform tag also determines
which Linux distributions can install it. Source builds require Rust and
maturin. Scene containers and Soma's ROS helper need the package installed in
their own Python environments before migrating their calls.

With the wheel installed, run
`python -m unittest discover -s system/chronos/clock/tests -p test_python.py`.
The failure tests require Linux and a C compiler and never change host time.
