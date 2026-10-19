# Chronos

`Chronos::new(ChronosConfig)` composes the clock library and device models. Capture ingress snapshots, submit observations and query conversions and coherent quality/model snapshots. Reference and device generations invalidate stale observation tokens.

Outlier checks include the configured drift allowance over steady time since the last accepted observation, so permitted rate changes and observations within holdover uncertainty can update the model. Larger residuals remain rejected.

`QualityRuntime::start(chronos)` runs periodic quality evaluation on an owned worker. Read failures retain historical diagnostics and mark data unavailable; recovery clears the failure. Call `stop()` to join the worker. The library does not require Atlas, gRPC or ROS.

Run `cargo test -p robonix-chronos`.
