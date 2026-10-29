// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use std::path::PathBuf;

use alai_core::artifact::{self, ExtractError as Error};
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

use crate::os_error;

create_exception!(
    alai_core.artifact,
    ExtractError,
    PyException,
    "A tarball can't be read, or its collection can't be extracted safely."
);

fn extract_error(py: Python<'_>, err: Error) -> PyErr {
    let message = err.to_string();
    match err {
        Error::Os(err, path) => os_error(err, path),
        Error::Artifact(kind, entry, detail) => {
            let raised = ExtractError::new_err(message);
            let value = raised.value(py);
            let set = value
                .setattr("kind", kind)
                .and_then(|()| value.setattr("entry", entry))
                .and_then(|()| value.setattr("detail", detail));
            set.err().unwrap_or(raised)
        }
    }
}

#[pyfunction]
#[pyo3(signature = (tarball, dest, *, limit=0, expected=None))]
pub(crate) fn artifact_extract(
    py: Python<'_>,
    tarball: PathBuf,
    dest: PathBuf,
    limit: u64,
    expected: Option<(String, String, String)>,
) -> PyResult<(String, String, String)> {
    py.detach(|| artifact::extract(&tarball, &dest, limit, expected.as_ref()))
        .map_err(|err| extract_error(py, err))
}

#[pyfunction]
#[pyo3(signature = (tarball, dest, *, include, limit=0))]
pub(crate) fn artifact_extract_sdist(
    py: Python<'_>,
    tarball: PathBuf,
    dest: PathBuf,
    include: Vec<String>,
    limit: u64,
) -> PyResult<()> {
    py.detach(|| artifact::extract_sdist(&tarball, &dest, limit, &include))
        .map_err(|err| extract_error(py, err))
}
