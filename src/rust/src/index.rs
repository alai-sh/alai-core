// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use std::collections::HashMap;

use alai_core::index::{self, FormatError, VersionFields};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple, PyType};

use crate::{pep440, semver, value_error};

pyo3::create_exception!(
    alai_core.index,
    NewerFormatError,
    PyValueError,
    "A file needs a newer reader. `found` lists the formats or versions it offers, and `supported` is the newest read."
);

fn format_error(py: Python<'_>, err: FormatError) -> PyErr {
    match err {
        FormatError::Invalid(message) => PyValueError::new_err(message),
        FormatError::Unsupported { ref found, supported } => {
            let raised = NewerFormatError::new_err(err.to_string());
            let value = raised.value(py);
            let set = PyTuple::new(py, found)
                .and_then(|found| value.setattr("found", found))
                .and_then(|()| value.setattr("supported", supported));
            set.err().unwrap_or(raised)
        }
    }
}

#[pyclass(frozen, eq, name = "Config", module = "alai_core.index")]
#[derive(PartialEq)]
pub(crate) struct Config(index::Config);

#[pymethods]
impl Config {
    #[new]
    #[pyo3(signature = (*, index, dl, collections=None, plugins=None, moved_to=None))]
    fn new(
        index: &str,
        dl: &str,
        collections: Option<&str>,
        plugins: Option<&str>,
        moved_to: Option<&str>,
    ) -> PyResult<Self> {
        index::Config::new(index, collections, plugins, dl, moved_to)
            .map(Self)
            .map_err(value_error)
    }

    #[classmethod]
    fn parse(_cls: &Bound<'_, PyType>, py: Python<'_>, data: &[u8]) -> PyResult<Self> {
        index::Config::parse(data).map(Self).map_err(|e| format_error(py, e))
    }

    #[getter]
    fn format(&self) -> u64 {
        self.0.format()
    }

    #[getter]
    fn index(&self) -> &str {
        self.0.index()
    }

    #[getter]
    fn collections(&self) -> Option<&str> {
        self.0.collections()
    }

    #[getter]
    fn plugins(&self) -> Option<&str> {
        self.0.plugins()
    }

    #[getter]
    fn dl(&self) -> &str {
        self.0.dl()
    }

    #[getter]
    fn moved_to(&self) -> Option<&str> {
        self.0.moved_to()
    }

    #[getter]
    fn hashed(&self) -> bool {
        self.0.is_hashed()
    }

    #[pyo3(signature = (*, name, sha256=None))]
    fn index_path(&self, name: &str, sha256: Option<&str>) -> PyResult<String> {
        self.0.index_path(name, sha256).map_err(value_error)
    }

    #[pyo3(signature = (*, name, version, sha256))]
    fn artifact_path(&self, name: &str, version: &Bound<'_, semver::Version>, sha256: &str) -> PyResult<String> {
        self.0
            .artifact_path(name, &version.get().0, sha256)
            .map_err(value_error)
    }

    fn to_json(&self) -> String {
        self.0.to_json()
    }
}

#[pyclass(frozen, eq, name = "CollectionEntry", module = "alai_core.index")]
#[derive(Clone, PartialEq)]
pub(crate) struct CollectionEntry(index::CollectionEntry);

#[pymethods]
impl CollectionEntry {
    #[new]
    #[pyo3(signature = (updated, sha256=None))]
    fn new(updated: &str, sha256: Option<&str>) -> PyResult<Self> {
        index::CollectionEntry::new(updated, sha256)
            .map(Self)
            .map_err(value_error)
    }

    #[getter]
    fn updated(&self) -> &str {
        self.0.updated()
    }

    #[getter]
    fn sha256(&self) -> Option<&str> {
        self.0.sha256()
    }
}

#[pyclass(frozen, eq, name = "Collections", module = "alai_core.index")]
#[derive(PartialEq)]
pub(crate) struct Collections(index::Collections);

#[pymethods]
impl Collections {
    #[new]
    fn new(collections: HashMap<String, Bound<'_, CollectionEntry>>) -> PyResult<Self> {
        index::Collections::new(
            collections
                .into_iter()
                .map(|(name, listed)| (name, listed.get().0.clone())),
        )
        .map(Self)
        .map_err(value_error)
    }

    #[classmethod]
    fn parse(_cls: &Bound<'_, PyType>, py: Python<'_>, data: &[u8]) -> PyResult<Self> {
        index::Collections::parse(data)
            .map(Self)
            .map_err(|e| format_error(py, e))
    }

    fn get(&self, name: &str) -> Option<CollectionEntry> {
        self.0.get(name).cloned().map(CollectionEntry)
    }

    fn __contains__(&self, name: &str) -> bool {
        self.0.get(name).is_some()
    }

    fn __len__(&self) -> usize {
        self.0.len()
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let names = PyList::new(py, self.0.iter().map(|(name, _)| name))?;
        Ok(names.as_any().try_iter()?.into_any())
    }

    fn to_json(&self) -> PyResult<String> {
        self.0.to_json().map_err(value_error)
    }
}

/// The Python objects the resolver reads over and over are made once, with the line.
#[pyclass(frozen, eq, name = "CollectionVersion", module = "alai_core.index")]
pub(crate) struct CollectionVersion {
    line: index::CollectionVersion,
    version: Py<semver::Version>,
    /// Why the line can't be used, in place of what can't be read.
    deps: Result<Py<PyTuple>, String>,
    requires_ansible: Result<Option<Py<pep440::SpecifierSet>>, String>,
}

impl PartialEq for CollectionVersion {
    fn eq(&self, other: &Self) -> bool {
        self.line == other.line
    }
}

impl CollectionVersion {
    fn wrap(py: Python<'_>, line: index::CollectionVersion, shared: &mut Shared) -> PyResult<Self> {
        let deps = match line.deps() {
            Ok(deps) => {
                let mut made = Vec::with_capacity(deps.len());
                for (name, req) in deps {
                    made.push((name, shared.req(py, req)?));
                }
                Ok(PyTuple::new(py, made)?.unbind())
            }
            Err(why) => Err(why),
        };
        let requires_ansible = match line.requires_ansible() {
            Ok(spec) => Ok(spec.map(|spec| shared.spec(py, spec)).transpose()?),
            Err(why) => Err(why),
        };
        Ok(CollectionVersion {
            version: Py::new(py, semver::Version(line.version().clone()))?,
            deps,
            requires_ansible,
            line,
        })
    }
}

/// One Python object per distinct requirement or specifier text, shared by every line that has it, as an index
/// repeats a few hundred across thousands of lines.
#[derive(Default)]
struct Shared {
    reqs: HashMap<String, Py<semver::VersionReq>>,
    specs: HashMap<String, Py<pep440::SpecifierSet>>,
}

impl Shared {
    fn req(&mut self, py: Python<'_>, req: &alai_core::semver::VersionReq) -> PyResult<Py<semver::VersionReq>> {
        if let Some(found) = self.reqs.get(req.text()) {
            return Ok(found.clone_ref(py));
        }
        let made = Py::new(py, semver::VersionReq(req.clone()))?;
        self.reqs.insert(req.text().to_owned(), made.clone_ref(py));
        Ok(made)
    }

    fn spec(&mut self, py: Python<'_>, spec: &alai_core::pep440::SpecifierSet) -> PyResult<Py<pep440::SpecifierSet>> {
        if let Some(found) = self.specs.get(spec.text()) {
            return Ok(found.clone_ref(py));
        }
        let made = Py::new(py, pep440::SpecifierSet(spec.clone()))?;
        self.specs.insert(spec.text().to_owned(), made.clone_ref(py));
        Ok(made)
    }
}

#[pymethods]
impl CollectionVersion {
    #[new]
    #[pyo3(signature = (
        *, name, version, sha256, size, published, deps=Vec::new(), requires_ansible=None, yanked=false,
        deprecated=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        name: String,
        version: &Bound<'_, semver::Version>,
        sha256: String,
        size: u64,
        published: String,
        deps: Vec<(String, String)>,
        requires_ansible: Option<String>,
        yanked: bool,
        deprecated: bool,
    ) -> PyResult<Self> {
        let line = index::CollectionVersion::new(VersionFields {
            name,
            version: version.get().0.clone(),
            deps,
            sha256,
            size,
            requires_ansible,
            yanked,
            published,
            deprecated,
        })
        .map_err(value_error)?;
        Self::wrap(py, line, &mut Shared::default())
    }

    #[getter]
    fn name(&self) -> &str {
        self.line.name()
    }

    #[getter]
    fn version(&self, py: Python<'_>) -> Py<semver::Version> {
        self.version.clone_ref(py)
    }

    #[getter]
    fn deps(&self, py: Python<'_>) -> PyResult<Py<PyTuple>> {
        self.deps
            .as_ref()
            .map(|deps| deps.clone_ref(py))
            .map_err(|why| PyValueError::new_err(why.clone()))
    }

    #[getter]
    fn sha256(&self) -> &str {
        self.line.sha256()
    }

    #[getter]
    fn size(&self) -> u64 {
        self.line.size()
    }

    #[getter]
    fn requires_ansible(&self, py: Python<'_>) -> PyResult<Option<Py<pep440::SpecifierSet>>> {
        match &self.requires_ansible {
            Ok(spec) => Ok(spec.as_ref().map(|spec| spec.clone_ref(py))),
            Err(why) => Err(PyValueError::new_err(why.clone())),
        }
    }

    #[getter]
    fn yanked(&self) -> bool {
        self.line.yanked()
    }

    #[getter]
    fn published(&self) -> &str {
        self.line.published()
    }

    #[getter]
    fn deprecated(&self) -> bool {
        self.line.deprecated()
    }

    #[getter]
    fn problem(&self) -> Option<String> {
        self.line.problem()
    }

    #[pyo3(signature = (*, allow_unreadable=false))]
    fn to_json(&self, allow_unreadable: bool) -> PyResult<String> {
        self.line.to_json(allow_unreadable).map_err(value_error)
    }
}

#[pyclass(frozen, name = "CollectionIndex", module = "alai_core.index")]
pub(crate) struct CollectionIndex {
    name: String,
    lines: Py<PyTuple>,
    newer: usize,
    invalid: Py<PyTuple>,
}

#[pymethods]
impl CollectionIndex {
    #[new]
    fn new(py: Python<'_>, name: &str, lines: Vec<Bound<'_, CollectionVersion>>) -> PyResult<Self> {
        let core = lines.iter().map(|line| line.get().line.clone()).collect();
        index::CollectionIndex::new(name, core).map_err(value_error)?;
        Ok(CollectionIndex {
            name: name.to_owned(),
            lines: PyTuple::new(py, lines)?.unbind(),
            newer: 0,
            invalid: PyTuple::empty(py).unbind(),
        })
    }

    #[classmethod]
    fn parse(_cls: &Bound<'_, PyType>, py: Python<'_>, data: &[u8], name: &str) -> PyResult<Self> {
        let (name, lines, newer, invalid) = py.detach(|| index::CollectionIndex::parse(data, name)).into_parts();
        let mut shared = Shared::default();
        let lines = lines
            .into_iter()
            .map(|line| CollectionVersion::wrap(py, line, &mut shared))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(CollectionIndex {
            name,
            lines: PyTuple::new(py, lines)?.unbind(),
            newer,
            invalid: PyTuple::new(py, invalid)?.unbind(),
        })
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn lines(&self, py: Python<'_>) -> Py<PyTuple> {
        self.lines.clone_ref(py)
    }

    #[getter]
    fn newer(&self) -> usize {
        self.newer
    }

    #[getter]
    fn invalid(&self, py: Python<'_>) -> Py<PyTuple> {
        self.invalid.clone_ref(py)
    }

    #[pyo3(signature = (*, allow_unreadable=false))]
    fn to_jsonl(&self, py: Python<'_>, allow_unreadable: bool) -> PyResult<String> {
        let mut out = String::new();
        for line in self.lines.bind(py).iter() {
            let line = line.cast_into::<CollectionVersion>()?;
            out.push_str(&line.get().line.to_json(allow_unreadable).map_err(value_error)?);
            out.push('\n');
        }
        Ok(out)
    }
}
