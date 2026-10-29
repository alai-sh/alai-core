// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use alai_core::collection::{self, PluginRouting, ReadError};
use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple, PyType};

use crate::value_error;
use crate::yaml::{Converter, OnWarning, named, warn, yaml_error};

create_exception!(
    alai_core.collection,
    MetadataError,
    PyValueError,
    "A collection's metadata breaks ansible-galaxy's rules. `errors` lists each field's problem."
);

pub(crate) fn read_error(py: Python<'_>, err: ReadError, name: Option<&str>) -> PyErr {
    match err {
        ReadError::Yaml(err) => yaml_error(py, err, name),
        ReadError::Json(message) => PyValueError::new_err(named(name, message)),
        ReadError::Invalid(err) => metadata_error(py, err, name),
    }
}

fn metadata_error(py: Python<'_>, err: collection::MetadataError, name: Option<&str>) -> PyErr {
    let lines: Vec<String> = err
        .to_string()
        .lines()
        .map(|line| named(name, line.to_owned()))
        .collect();
    let raised = MetadataError::new_err(lines.join("\n"));
    let value = raised.value(py);
    let set = PyTuple::new(py, err.errors).and_then(|errors| value.setattr("errors", errors));
    set.err().unwrap_or(raised)
}

#[pyfunction]
pub(crate) fn collection_check_name(name: &str) -> PyResult<()> {
    collection::check_collection_name(name).map_err(value_error)
}

#[pyclass(frozen, name = "Metadata", module = "alai_core.collection")]
pub(crate) struct Metadata {
    read: collection::Metadata,
    name: Option<String>,
}

impl Metadata {
    fn raw<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let doc = &self.read.document;
        self.read.get(key).map(|v| Converter::new(doc).value(py, v)).transpose()
    }

    fn checked<T>(&self, py: Python<'_>, got: Result<T, collection::MetadataError>) -> PyResult<T> {
        got.map_err(|err| metadata_error(py, err, self.name.as_deref()))
    }
}

#[pymethods]
impl Metadata {
    #[classmethod]
    #[pyo3(signature = (data, *, name=None, on_warning=OnWarning::Default))]
    fn from_galaxy_yml<'py>(
        _cls: &Bound<'py, PyType>,
        py: Python<'py>,
        data: &[u8],
        name: Option<&str>,
        on_warning: OnWarning<'py>,
    ) -> PyResult<Self> {
        let read = py
            .detach(|| collection::Metadata::from_galaxy_yml(data))
            .map_err(|err| read_error(py, err, name))?;
        warn(py, &read.document.warnings, name, on_warning)?;
        Ok(Metadata {
            read,
            name: name.map(str::to_owned),
        })
    }

    /// The top-level value `key` names, as written, or `default` if it isn't set.
    #[pyo3(signature = (key, /, default=None))]
    fn get<'py>(
        &self,
        py: Python<'py>,
        key: &str,
        default: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        if !self.read.has(key) {
            return Ok(default);
        }
        self.raw(py, key)
    }

    fn __contains__(&self, key: &str) -> bool {
        self.read.has(key)
    }

    fn get_fqcn(&self, py: Python<'_>) -> PyResult<String> {
        self.checked(py, self.read.get_fqcn())
    }

    fn get_version(&self, py: Python<'_>) -> PyResult<crate::semver::Version> {
        self.checked(py, self.read.get_version()).map(crate::semver::Version)
    }

    fn get_dependencies<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (name, req) in self.checked(py, self.read.get_dependencies())? {
            dict.set_item(name, crate::semver::VersionReq(req))?;
        }
        Ok(dict)
    }
}

#[pyclass(frozen, name = "Manifest", module = "alai_core.collection")]
pub(crate) struct Manifest {
    read: collection::Manifest,
    name: Option<String>,
}

#[pymethods]
impl Manifest {
    #[classmethod]
    #[pyo3(signature = (data, *, name=None))]
    fn parse(_cls: &Bound<'_, PyType>, py: Python<'_>, data: &[u8], name: Option<&str>) -> PyResult<Self> {
        let read = collection::Manifest::parse(data).map_err(|err| read_error(py, err, name))?;
        Ok(Manifest {
            read,
            name: name.map(str::to_owned),
        })
    }

    #[getter]
    fn collection_info(&self) -> Metadata {
        Metadata {
            read: self.read.collection_info.clone(),
            name: self.name.clone(),
        }
    }

    #[getter]
    fn files_name(&self) -> &str {
        &self.read.files_name
    }

    #[getter]
    fn files_sha256(&self) -> &str {
        &self.read.files_sha256
    }
}

fn routing<'py>(py: Python<'py>, routing: &PluginRouting) -> PyResult<Bound<'py, PyDict>> {
    let mut converter = Converter::new(&routing.document);
    let by_type = PyDict::new(py);
    for (kind, entries) in &routing.types {
        let named = PyDict::new(py);
        for (name, entry, raw) in entries {
            let raw = converter.value(py, raw)?.unbind();
            named.set_item(
                name,
                RoutingEntry {
                    entry: entry.clone(),
                    raw,
                },
            )?;
        }
        by_type.set_item(kind, named)?;
    }
    Ok(by_type)
}

#[pyclass(frozen, name = "RoutingEntry", module = "alai_core.collection")]
pub(crate) struct RoutingEntry {
    pub(crate) entry: collection::RoutingEntry,
    pub(crate) raw: Py<PyAny>,
}

#[pymethods]
impl RoutingEntry {
    #[getter]
    fn redirect(&self) -> Option<&str> {
        self.entry.redirect.as_deref()
    }

    #[getter]
    fn action_plugin(&self) -> Option<&str> {
        self.entry.action_plugin.as_deref()
    }

    #[getter]
    fn deprecation(&self) -> Option<Removal> {
        self.entry.deprecation.clone().map(Removal)
    }

    #[getter]
    fn tombstone(&self) -> Option<Removal> {
        self.entry.tombstone.clone().map(Removal)
    }

    #[getter]
    fn raw(&self, py: Python<'_>) -> Py<PyAny> {
        self.raw.clone_ref(py)
    }
}

#[pyclass(frozen, name = "Removal", module = "alai_core.collection")]
pub(crate) struct Removal(collection::Removal);

#[pymethods]
impl Removal {
    #[getter]
    fn removal_version(&self) -> Option<&str> {
        self.0.removal_version.as_deref()
    }

    #[getter]
    fn removal_date(&self) -> Option<&str> {
        self.0.removal_date.as_deref()
    }

    #[getter]
    fn warning_text(&self) -> Option<&str> {
        self.0.warning_text.as_deref()
    }
}

#[pyclass(frozen, name = "Runtime", module = "alai_core.collection")]
pub(crate) struct Runtime {
    read: collection::Runtime,
    name: Option<String>,
}

#[pymethods]
impl Runtime {
    #[classmethod]
    #[pyo3(signature = (data, *, name=None, on_warning=OnWarning::Default))]
    fn parse<'py>(
        _cls: &Bound<'py, PyType>,
        py: Python<'py>,
        data: &[u8],
        name: Option<&str>,
        on_warning: OnWarning<'py>,
    ) -> PyResult<Self> {
        let read = py
            .detach(|| collection::Runtime::parse(data))
            .map_err(|err| read_error(py, err, name))?;
        warn(py, &read.warnings, name, on_warning)?;
        Ok(Runtime {
            read,
            name: name.map(str::to_owned),
        })
    }

    #[getter(requires_ansible)]
    fn raw_requires_ansible<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let doc = &self.read.plugin_routing.document;
        self.read
            .requires_ansible
            .as_ref()
            .map(|v| Converter::new(doc).value(py, v))
            .transpose()
    }

    fn get_requires_ansible(&self, py: Python<'_>) -> PyResult<Option<crate::pep440::SpecifierSet>> {
        let got = self.read.get_requires_ansible();
        let got = got.map_err(|err| metadata_error(py, err, self.name.as_deref()))?;
        Ok(got.map(crate::pep440::SpecifierSet))
    }

    #[getter]
    fn plugin_routing<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        routing(py, &self.read.plugin_routing)
    }
}

#[pyclass(frozen, name = "EdaRuntime", module = "alai_core.collection")]
pub(crate) struct EdaRuntime(collection::EdaRuntime);

#[pymethods]
impl EdaRuntime {
    #[classmethod]
    #[pyo3(signature = (data, *, name=None, on_warning=OnWarning::Default))]
    fn parse<'py>(
        _cls: &Bound<'py, PyType>,
        py: Python<'py>,
        data: &[u8],
        name: Option<&str>,
        on_warning: OnWarning<'py>,
    ) -> PyResult<Self> {
        let read = py
            .detach(|| collection::EdaRuntime::parse(data))
            .map_err(|err| read_error(py, err, name))?;
        warn(py, &read.warnings, name, on_warning)?;
        Ok(EdaRuntime(read))
    }

    #[getter]
    fn plugin_routing<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        routing(py, &self.0.plugin_routing)
    }
}
