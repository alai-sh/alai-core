// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use alai_core::semver;
use pyo3::prelude::*;
use pyo3::types::{PyInt, PyString, PyTuple, PyType};

use crate::{repr, value_error};

#[pyclass(frozen, eq, ord, hash, name = "Version", module = "alai_core.semver")]
#[derive(Clone, PartialEq, PartialOrd, Hash)]
pub(crate) struct Version(pub(crate) semver::Version);

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
    fn precedence<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let key = self.0.precedence();
        let (major, minor, patch) = key.release();
        let mut parts: Vec<Bound<'py, PyAny>> = vec![
            major.into_pyobject(py)?.into_any(),
            minor.into_pyobject(py)?.into_any(),
            patch.into_pyobject(py)?.into_any(),
        ];
        let pre: Vec<_> = key.prerelease().collect();
        parts.push(i64::from(pre.is_empty()).into_pyobject(py)?.into_any());
        for id in pre {
            let pair = match id {
                semver::Identifier::Number(digits) => (0, py.get_type::<PyInt>().call1((digits,))?),
                semver::Identifier::Word(word) => (1, PyString::new(py, word).into_any()),
            };
            parts.push(pair.into_pyobject(py)?.into_any());
        }
        PyTuple::new(py, parts)
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
pub(crate) struct VersionReq(pub(crate) semver::VersionReq);

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
