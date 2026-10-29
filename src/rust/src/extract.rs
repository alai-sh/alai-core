// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use std::path::PathBuf;

use alai_core::extract::{self, FragmentIndex};
use alai_core::yaml::{Document, Value};
use pyo3::exceptions::{PyFileNotFoundError, PyNotADirectoryError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyMapping, PyString, PyTuple};

use crate::collection::read_error;
use crate::os_error;
use crate::yaml::Converter;

/// A value from extraction as Python objects. It has no aliases, so it needs no document of its own.
fn py_value<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    let doc = Document {
        root: Value::Null,
        anchors: Vec::new(),
        warnings: Vec::new(),
    };
    Converter::new(&doc).value(py, value)
}

/// Whether two values from YAML are equal as Python compares what they give, a mapping's order aside.
fn same(py: Python<'_>, a: Option<&Value>, b: Option<&Value>) -> PyResult<bool> {
    match (a, b) {
        (Some(a), Some(b)) => py_value(py, a)?.eq(py_value(py, b)?),
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

fn same_docs(py: Python<'_>, a: &extract::PluginDocs, b: &extract::PluginDocs) -> PyResult<bool> {
    Ok(a.examples == b.examples
        && a.path == b.path
        && a.errors == b.errors
        && a.collection == b.collection
        && a.module == b.module
        && a.fragments == b.fragments
        && a.return_fragments == b.return_fragments
        && same(py, a.documentation.as_ref(), b.documentation.as_ref())?
        && same(py, a.returns.as_ref(), b.returns.as_ref())?
        && same(py, a.entry_points.as_ref(), b.entry_points.as_ref())?)
}

fn py_optional<'py>(py: Python<'py>, value: Option<&Value>) -> PyResult<Option<Bound<'py, PyAny>>> {
    value.map(|v| py_value(py, v)).transpose()
}

/// A Python value as one YAML could have held, a mapping of any type as a dict.
fn value_of(ob: &Bound<'_, PyAny>) -> PyResult<Value> {
    Ok(if ob.is_none() {
        Value::Null
    } else if let Ok(b) = ob.cast::<PyBool>() {
        Value::Bool(b.is_true())
    } else if let Ok(n) = ob.cast::<PyInt>() {
        n.extract::<i64>()
            .map_or_else(|_| Value::BigInt(n.to_string()), Value::Int)
    } else if let Ok(f) = ob.cast::<PyFloat>() {
        Value::Float(f.value())
    } else if let Ok(s) = ob.cast::<PyString>() {
        Value::String(s.to_str()?.to_owned())
    } else if let Ok(d) = ob.cast::<PyDict>() {
        Value::Mapping(
            d.iter()
                .map(|(k, v)| Ok((value_of(&k)?, value_of(&v)?)))
                .collect::<PyResult<_>>()?,
        )
    } else if let Ok(m) = ob.cast::<PyMapping>() {
        Value::Mapping(
            m.items()?
                .iter()
                .map(|item| {
                    let (k, v): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
                    Ok((value_of(&k)?, value_of(&v)?))
                })
                .collect::<PyResult<_>>()?,
        )
    } else if ob.is_instance_of::<PyList>() || ob.is_instance_of::<PyTuple>() {
        Value::Sequence(ob.try_iter()?.map(|item| value_of(&item?)).collect::<PyResult<_>>()?)
    } else {
        return Err(PyTypeError::new_err(format!(
            "can't use {} in docs",
            ob.get_type().name()?
        )));
    })
}

/// A mapping given from Python, or None.
fn mapping_of(ob: Option<&Bound<'_, PyAny>>, what: &str) -> PyResult<Option<Value>> {
    let Some(ob) = ob.filter(|ob| !ob.is_none()) else {
        return Ok(None);
    };
    if ob.cast::<PyMapping>().is_err() {
        return Err(PyTypeError::new_err(format!("{what} must be a mapping or None")));
    }
    value_of(ob).map(Some)
}

/// What `extract` gives for the directory `path`, with the GIL released. `what` names the source it must be.
fn run(
    py: Python<'_>,
    path: PathBuf,
    docs: bool,
    what: &str,
    extract: fn(&std::path::Path, bool) -> Result<extract::Collection, extract::ExtractError>,
) -> PyResult<Collection> {
    // A missing path is the filesystem's error, FileNotFoundError, from reading it.
    if path.exists() && !path.is_dir() {
        return Err(PyNotADirectoryError::new_err(format!(
            "{} isn't a directory: extract a tarball first",
            path.display()
        )));
    }
    py.detach(|| extract(&path, docs))
        .map(Collection)
        .map_err(|err| match err {
            extract::ExtractError::Io(err) => os_error(err, path),
            extract::ExtractError::Metadata(file, err) => {
                read_error(py, err, Some(&path.join(file).display().to_string()))
            }
            extract::ExtractError::NotSource(_) => {
                PyFileNotFoundError::new_err(format!("{} isn't {what}'s source: {err}", path.display()))
            }
        })
}

#[pyfunction]
#[pyo3(signature = (path, /, *, docs=false))]
pub(crate) fn extract_collection(py: Python<'_>, path: PathBuf, docs: bool) -> PyResult<Collection> {
    run(py, path, docs, "a collection", extract::extract)
}

#[pyfunction]
#[pyo3(signature = (path, /, *, docs=false))]
pub(crate) fn extract_ansible_core(py: Python<'_>, path: PathBuf, docs: bool) -> PyResult<Collection> {
    run(py, path, docs, "ansible-core", extract::extract_ansible_core)
}

#[pyfunction]
#[pyo3(signature = (path, /, *, docs=false))]
pub(crate) fn extract_ansible_rulebook(py: Python<'_>, path: PathBuf, docs: bool) -> PyResult<Collection> {
    run(py, path, docs, "ansible-rulebook", extract::extract_ansible_rulebook)
}

/// `EDA_LEGACY_ROUTING` by type, each old name to where it goes, as ansible-rulebook's own two dicts have it.
pub(crate) fn eda_legacy_routing(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let routing = PyDict::new(py);
    for (kind, name, target) in extract::EDA_LEGACY_ROUTING {
        let names = match routing.get_item(kind)? {
            Some(names) => names.cast_into::<PyDict>()?,
            None => {
                let names = PyDict::new(py);
                routing.set_item(kind, &names)?;
                names
            }
        };
        names.set_item(name, target)?;
    }
    Ok(routing)
}

#[pyfunction]
pub(crate) fn extract_fragment_lookup(collections: &Bound<'_, PyAny>) -> PyResult<FragmentLookup> {
    let mut index = FragmentIndex::default();
    for collection in collections.try_iter()? {
        let collection = collection?;
        let collection = collection.cast::<Collection>()?.get();
        let (Some(namespace), Some(name)) = (&collection.0.namespace, &collection.0.name) else {
            continue; // its fragments can't be named
        };
        index.add(
            &format!("{namespace}.{name}"),
            &collection.0.fragments,
            &collection.0.fragment_routing,
        );
    }
    Ok(FragmentLookup(index))
}

#[pyclass(frozen, name = "Collection", module = "alai_core.extract")]
pub(crate) struct Collection(extract::Collection);

#[pymethods]
impl Collection {
    #[getter]
    fn namespace(&self) -> Option<&str> {
        self.0.namespace.as_deref()
    }

    #[getter]
    fn name(&self) -> Option<&str> {
        self.0.name.as_deref()
    }

    #[getter]
    fn version(&self) -> Option<&str> {
        self.0.version.as_deref()
    }

    #[getter]
    fn dependencies<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (name, req) in &self.0.dependencies {
            dict.set_item(name, req)?;
        }
        Ok(dict)
    }

    #[getter]
    fn requires_ansible(&self) -> Option<&str> {
        self.0.requires_ansible.as_deref()
    }

    #[getter]
    fn plugins<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.plugins.iter().map(|p| Plugin(p.clone())))
    }

    #[getter]
    fn routing<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.routing.iter().map(|r| Routing(r.clone())))
    }

    #[getter]
    fn fragments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for fragment in &self.0.fragments {
            dict.set_item(&fragment.name, DocFragment(fragment.clone()))?;
        }
        Ok(dict)
    }

    #[getter]
    fn fragment_routing<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (name, entry) in &self.0.fragment_routing {
            dict.set_item(name, py_value(py, entry)?)?;
        }
        Ok(dict)
    }

    #[getter]
    fn issues<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.issues.iter().map(|i| Issue(i.clone())))
    }
}

#[pyclass(frozen, name = "Plugin", module = "alai_core.extract")]
pub(crate) struct Plugin(extract::Plugin);

#[pymethods]
impl Plugin {
    #[getter]
    fn r#type(&self) -> &str {
        &self.0.kind
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    #[getter]
    fn short_description(&self) -> Option<&str> {
        self.0.short_description.as_deref()
    }

    #[getter]
    fn docs(&self) -> Option<PluginDocs> {
        self.0.docs.clone().map(PluginDocs)
    }

    fn __repr__(&self) -> String {
        format!("Plugin(type={:?}, name={:?})", self.0.kind, self.0.name)
    }

    fn __eq__(&self, py: Python<'_>, other: &Self) -> PyResult<bool> {
        let (a, b) = (&self.0, &other.0);
        Ok(a.kind == b.kind
            && a.name == b.name
            && a.short_description == b.short_description
            && match (&a.docs, &b.docs) {
                (Some(x), Some(y)) => same_docs(py, x, y)?,
                (None, None) => true,
                _ => false,
            })
    }
}

#[pyclass(frozen, name = "PluginDocs", module = "alai_core.extract")]
pub(crate) struct PluginDocs(extract::PluginDocs);

#[pymethods]
impl PluginDocs {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (*, documentation=None, examples=None, returns=None, entry_points=None, fragments=Vec::new(), return_fragments=Vec::new(), path=None, collection=None, module=false))]
    fn new(
        documentation: Option<&Bound<'_, PyAny>>,
        examples: Option<String>,
        returns: Option<&Bound<'_, PyAny>>,
        entry_points: Option<&Bound<'_, PyAny>>,
        fragments: Vec<String>,
        return_fragments: Vec<String>,
        path: Option<String>,
        collection: Option<String>,
        module: bool,
    ) -> PyResult<Self> {
        Ok(PluginDocs(extract::PluginDocs {
            documentation: mapping_of(documentation, "documentation")?,
            examples,
            returns: mapping_of(returns, "returns")?,
            entry_points: mapping_of(entry_points, "entry_points")?,
            fragments,
            return_fragments,
            path,
            errors: Vec::new(),
            collection,
            module,
        }))
    }

    #[getter]
    fn collection(&self) -> Option<&str> {
        self.0.collection.as_deref()
    }

    #[getter]
    fn module(&self) -> bool {
        self.0.module
    }

    #[getter]
    fn errors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.errors.iter().map(|e| DocsError(e.clone())))
    }

    #[getter]
    fn path(&self) -> Option<&str> {
        self.0.path.as_deref()
    }

    #[getter]
    fn documentation<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        py_optional(py, self.0.documentation.as_ref())
    }

    #[getter]
    fn examples(&self) -> Option<&str> {
        self.0.examples.as_deref()
    }

    #[getter]
    fn returns<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        py_optional(py, self.0.returns.as_ref())
    }

    #[getter]
    fn entry_points<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        py_optional(py, self.0.entry_points.as_ref())
    }

    #[getter]
    fn fragments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.0.fragments)
    }

    #[getter]
    fn return_fragments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.0.return_fragments)
    }

    fn __eq__(&self, py: Python<'_>, other: &Self) -> PyResult<bool> {
        same_docs(py, &self.0, &other.0)
    }

    // It can't change, so a copy is itself, as a tuple's is.
    fn __copy__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __deepcopy__<'py>(slf: PyRef<'py, Self>, _memo: &Bound<'py, PyAny>) -> PyRef<'py, Self> {
        slf
    }

    fn apply_fragments(&self, lookup: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut find = |name: &str| -> PyResult<Option<extract::DocFragment>> {
            let found = lookup.call1((name,))?;
            if found.is_none() {
                return Ok(None);
            }
            Ok(Some(found.cast::<DocFragment>()?.get().0.clone()))
        };
        self.0.apply_fragments(&mut find).map(PluginDocs)
    }
}

#[pyclass(frozen, eq, name = "DocsError", module = "alai_core.extract")]
#[derive(PartialEq)]
pub(crate) struct DocsError(extract::DocsError);

#[pymethods]
impl DocsError {
    #[getter]
    fn section(&self) -> Option<&str> {
        self.0.section
    }

    #[getter]
    fn message(&self) -> &str {
        &self.0.message
    }

    #[getter]
    fn text(&self) -> Option<&str> {
        self.0.text.as_deref()
    }

    fn __repr__(&self) -> String {
        format!("DocsError(section={:?}, message={:?})", self.0.section, self.0.message)
    }
}

#[pyclass(frozen, name = "Routing", module = "alai_core.extract")]
pub(crate) struct Routing(extract::Routing);

#[pymethods]
impl Routing {
    #[getter]
    fn r#type(&self) -> &str {
        &self.0.kind
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    #[getter]
    fn entry<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        py_value(py, &self.0.entry)
    }

    fn __eq__(&self, py: Python<'_>, other: &Self) -> PyResult<bool> {
        let (a, b) = (&self.0, &other.0);
        Ok(a.kind == b.kind && a.name == b.name && same(py, Some(&a.entry), Some(&b.entry))?)
    }
}

#[pyclass(frozen, eq, name = "Issue", module = "alai_core.extract")]
#[derive(PartialEq)]
pub(crate) struct Issue(extract::Issue);

#[pymethods]
impl Issue {
    #[getter]
    fn kind(&self) -> &str {
        self.0.kind
    }

    #[getter]
    fn path(&self) -> &str {
        &self.0.path
    }

    #[getter]
    fn detail(&self) -> &str {
        &self.0.detail
    }

    fn __repr__(&self) -> String {
        format!(
            "Issue(kind={:?}, path={:?}, detail={:?})",
            self.0.kind, self.0.path, self.0.detail
        )
    }
}

#[pyclass(frozen, name = "DocFragment", module = "alai_core.extract")]
pub(crate) struct DocFragment(extract::DocFragment);

#[pymethods]
impl DocFragment {
    #[new]
    #[pyo3(signature = (*, collection, name, attributes))]
    fn new(collection: Option<String>, name: String, attributes: &Bound<'_, PyMapping>) -> PyResult<Self> {
        let attributes = attributes
            .items()?
            .iter()
            .map(|item| {
                let (k, v): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
                let value = mapping_of(Some(&v), "each attribute")?.unwrap_or(Value::Mapping(Vec::new()));
                Ok((k.extract::<String>()?, value))
            })
            .collect::<PyResult<_>>()?;
        Ok(DocFragment(extract::DocFragment {
            collection,
            name,
            attributes,
        }))
    }

    #[getter]
    fn collection(&self) -> Option<&str> {
        self.0.collection.as_deref()
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    #[getter]
    fn attributes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (name, value) in &self.0.attributes {
            dict.set_item(name, py_value(py, value)?)?;
        }
        Ok(dict)
    }

    fn __eq__(&self, py: Python<'_>, other: &Self) -> PyResult<bool> {
        let (a, b) = (&self.0, &other.0);
        Ok(a.collection == b.collection && a.name == b.name && self.attributes(py)?.eq(other.attributes(py)?)?)
    }

    // It can't change, so a copy is itself, as a tuple's is.
    fn __copy__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __deepcopy__<'py>(slf: PyRef<'py, Self>, _memo: &Bound<'py, PyAny>) -> PyRef<'py, Self> {
        slf
    }
}

#[pyclass(frozen, name = "FragmentLookup", module = "alai_core.extract")]
pub(crate) struct FragmentLookup(FragmentIndex);

#[pymethods]
impl FragmentLookup {
    fn __call__(&self, name: &str) -> Option<DocFragment> {
        self.0.get(name).cloned().map(DocFragment)
    }
}
