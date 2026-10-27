// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use alai_core::semver;
use pyo3::prelude::*;
use pyo3::types::{PyTuple, PyType};

use crate::{repr, value_error};

#[pyclass(frozen, eq, ord, hash, name = "Version", module = "alai_core.semver")]
#[derive(Clone, PartialEq, PartialOrd, Hash)]
pub(crate) struct Version(semver::Version);

#[pymethods]
impl Version {
    #[new]
    fn new(text: &str) -> PyResult<Self> {
        semver::Version::parse(text).map(Self).map_err(value_error)
    }

    #[getter]
    fn text(&self) -> &str {
        self.0.text()
    }

    #[getter]
    fn is_prerelease(&self) -> bool {
        self.0.is_prerelease()
    }

    fn __str__(&self) -> &str {
        self.0.text()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        repr(py, "Version", self.0.text())
    }
}

#[pyclass(frozen, eq, hash, name = "VersionReq", module = "alai_core.semver")]
#[derive(PartialEq, Hash)]
pub(crate) struct VersionReq(semver::VersionReq);

#[pymethods]
impl VersionReq {
    #[new]
    fn new(text: &str) -> PyResult<Self> {
        semver::VersionReq::parse(text).map(Self).map_err(value_error)
    }

    #[classmethod]
    fn from_galaxy(_cls: &Bound<'_, PyType>, text: &str) -> PyResult<Self> {
        semver::VersionReq::parse_galaxy(text).map(Self).map_err(value_error)
    }

    #[getter]
    fn text(&self) -> &str {
        self.0.text()
    }

    #[getter]
    fn clauses<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(
            py,
            self.0
                .clauses()
                .iter()
                .map(|(op, version)| (op.as_str(), Version(version.clone()))),
        )
    }

    fn contains(&self, version: &Bound<'_, Version>) -> bool {
        self.0.contains(&version.get().0)
    }

    fn __str__(&self) -> &str {
        self.0.text()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        repr(py, "VersionReq", self.0.text())
    }
}
