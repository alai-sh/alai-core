// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use alai_core::pep440;
use pyo3::prelude::*;

use crate::{repr, value_error};

#[pyclass(frozen, eq, ord, hash, name = "Version", module = "alai_core.pep440")]
#[derive(Clone, PartialEq, PartialOrd, Hash)]
pub(crate) struct Version(pub(crate) pep440::Version);

#[pymethods]
impl Version {
    #[new]
    fn new(text: &str) -> PyResult<Self> {
        pep440::Version::parse(text).map(Self).map_err(value_error)
    }

    #[getter]
    fn text(&self) -> &str {
        self.0.text()
    }

    #[getter]
    fn is_prerelease(&self) -> bool {
        self.0.is_prerelease()
    }

    #[getter]
    fn is_postrelease(&self) -> bool {
        self.0.is_postrelease()
    }

    #[getter]
    fn base(&self) -> Self {
        Self(self.0.base())
    }

    fn __str__(&self) -> &str {
        self.0.text()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        repr(py, "Version", self.0.text())
    }
}

#[pyclass(frozen, eq, hash, name = "SpecifierSet", module = "alai_core.pep440")]
#[derive(PartialEq, Hash)]
pub(crate) struct SpecifierSet(pub(crate) pep440::SpecifierSet);

#[pymethods]
impl SpecifierSet {
    #[new]
    fn new(text: &str) -> PyResult<Self> {
        pep440::SpecifierSet::parse(text).map(Self).map_err(value_error)
    }

    #[getter]
    fn text(&self) -> &str {
        self.0.text()
    }

    fn contains(&self, version: &pyo3::Bound<'_, Version>) -> bool {
        self.0.contains(&version.get().0)
    }

    fn admits_lower_bound(&self, other: &pyo3::Bound<'_, SpecifierSet>) -> bool {
        self.0.admits_lower_bound(&other.get().0)
    }

    fn narrower_than(&self, other: &pyo3::Bound<'_, SpecifierSet>) -> bool {
        self.0.narrower_than(&other.get().0)
    }

    fn __str__(&self) -> &str {
        self.0.text()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        repr(py, "SpecifierSet", self.0.text())
    }
}
