# Chronos

`robonix-chronos` provides `ChronosCore`: a deterministic device-to-reference clock model library. Register a clock domain and policy, submit paired `ClockObservation` values, then query status, model coefficients or conversions. It does not read host clocks or run a process.

The core bounds its observation window, checks integer conversion overflow, tracks reference/device generations, and handles wrap, outliers, holdover and reset. Model updates support step and bounded slew.

Outlier checks include the configured drift allowance over steady time since the last accepted observation, so permitted rate changes and observations within holdover uncertainty can update the model. Larger residuals remain rejected.

Run `cargo test -p robonix-chronos --test core`.


`ChronosConfig::from_yaml()` parses and validates explicit direct/affine domain configuration, timestamp units, quality limits and observation policy. Configuration describes collection but does not start it. Linux clock types are re-exported from `robonix-chronos-clock`.

Run `cargo test -p robonix-chronos`.
