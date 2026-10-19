# Chronos

`robonix-chronos` provides `ChronosCore`: a deterministic device-to-reference clock model library. Register a clock domain and policy, submit paired `ClockObservation` values, then query status, model coefficients or conversions. It does not read host clocks or run a process.

The core bounds its observation window, checks integer conversion overflow, tracks reference/device generations, and handles wrap, outliers, holdover and reset. Model updates support step and bounded slew.

Run `cargo test -p robonix-chronos --test core`.
