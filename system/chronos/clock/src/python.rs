// SPDX-License-Identifier: MulanPSL-2.0

use pyo3::prelude::*;

use crate::{VirtualClockSnapshot, VirtualReferenceTime, VirtualSteadyTime};

#[pyfunction]
/// Read paired reference and local steady time, including reference quality.
fn snapshot(py: Python<'_>) -> PyResult<VirtualClockSnapshot> {
    Ok(py.detach(crate::snapshot)?)
}

#[pyfunction]
/// Read reference time with quality metadata, even when quality is unavailable.
fn reference_time(py: Python<'_>) -> PyResult<VirtualReferenceTime> {
    Ok(py.detach(crate::reference_time)?)
}

#[pyfunction]
/// Read available, nonnegative reference nanoseconds; failures raise OSError.
fn reference_timestamp_ns(py: Python<'_>) -> PyResult<u64> {
    Ok(py.detach(crate::reference_timestamp_ns)?)
}

#[pyfunction]
/// Read local steady nanoseconds independently of reference-clock health.
fn steady_time(py: Python<'_>) -> PyResult<VirtualSteadyTime> {
    Ok(py.detach(crate::steady_time)?)
}

#[pymodule]
fn robonix_chronos_clock(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<VirtualClockSnapshot>()?;
    module.add_class::<VirtualReferenceTime>()?;
    module.add_class::<VirtualSteadyTime>()?;
    module.add_function(wrap_pyfunction!(snapshot, module)?)?;
    module.add_function(wrap_pyfunction!(reference_time, module)?)?;
    module.add_function(wrap_pyfunction!(reference_timestamp_ns, module)?)?;
    module.add_function(wrap_pyfunction!(steady_time, module)?)?;
    Ok(())
}
