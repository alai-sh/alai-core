// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use pyo3::prelude::*;

// Without gil_used = false, a free-threaded Python turns the GIL back on for the whole process when importing this.
#[pymodule(gil_used = false)]
fn _native(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
