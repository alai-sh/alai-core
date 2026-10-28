// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use std::collections::HashSet;

use alai_core::plugins;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyList, PyString, PyType};
use serde_json::{Map, Value};

use crate::value_error;

/// The Python objects a search's caller reads are made once, with the entry.
#[pyclass(frozen, eq, name = "PluginEntry", module = "alai_core.plugins")]
pub(crate) struct PluginEntry {
    entry: plugins::PluginEntry,
    routing: Option<Py<PyAny>>,
    line: Option<Py<PyBytes>>,
}

impl PartialEq for PluginEntry {
    fn eq(&self, other: &Self) -> bool {
        self.entry == other.entry
    }
}

impl PluginEntry {
    fn wrap(py: Python<'_>, entry: plugins::PluginEntry, line: Option<&[u8]>) -> PyResult<Self> {
        let routing = match entry.routing() {
            Some(routing) => Some(object_to_python(py, routing)?.into_any().unbind()),
            None => None,
        };
        Ok(PluginEntry {
            entry,
            routing,
            line: line.map(|line| PyBytes::new(py, line).unbind()),
        })
    }
}

#[pymethods]
impl PluginEntry {
    #[new]
    #[pyo3(signature = (r#type, name, collection, *, since=None, until=None, short_description=None, routing=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        r#type: &str,
        name: &str,
        collection: &str,
        since: Option<&str>,
        until: Option<&str>,
        short_description: Option<&str>,
        routing: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        // Through Python's json, so anything it can't write is refused with its own message.
        let routing = match routing {
            Some(routing) => {
                let routing = py.get_type::<PyDict>().call1((routing,))?;
                let text: String = py.import("json")?.call_method1("dumps", (routing,))?.extract()?;
                Some(serde_json::from_str::<Map<String, Value>>(&text).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("'routing' must be a JSON object: {e}"))
                })?)
            }
            None => None,
        };
        let entry = plugins::PluginEntry::new(r#type, name, collection, since, until, short_description, routing)
            .map_err(value_error)?;
        Self::wrap(py, entry, None)
    }

    #[classmethod]
    fn parse(_cls: &Bound<'_, PyType>, py: Python<'_>, line: &[u8]) -> PyResult<Option<Self>> {
        plugins::PluginEntry::parse(line)
            .map(|entry| Self::wrap(py, entry, Some(line)))
            .transpose()
    }

    #[getter(r#type)]
    fn kind(&self) -> &str {
        self.entry.kind()
    }

    #[getter]
    fn name(&self) -> &str {
        self.entry.name()
    }

    #[getter]
    fn collection(&self) -> &str {
        self.entry.collection()
    }

    #[getter]
    fn fqcn(&self) -> String {
        format!("{}.{}", self.entry.collection(), self.entry.name())
    }

    #[getter]
    fn since(&self) -> Option<&str> {
        self.entry.since()
    }

    #[getter]
    fn until(&self) -> Option<&str> {
        self.entry.until()
    }

    #[getter]
    fn short_description(&self) -> Option<&str> {
        self.entry.short_description()
    }

    #[getter]
    fn routing(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.routing.as_ref().map(|routing| routing.clone_ref(py))
    }

    #[getter]
    fn line(&self, py: Python<'_>) -> Option<Py<PyBytes>> {
        self.line.as_ref().map(|line| line.clone_ref(py))
    }

    fn to_json(&self) -> PyResult<String> {
        self.entry.to_json().map_err(value_error)
    }
}

#[pyclass(frozen, name = "Plugins", module = "alai_core.plugins")]
pub(crate) struct Plugins {
    data: Py<PyBytes>,
}

impl Plugins {
    fn wrap_all<'py>(py: Python<'py>, found: Vec<(plugins::PluginEntry, &[u8])>) -> PyResult<Bound<'py, PyList>> {
        let entries = found
            .into_iter()
            .map(|(entry, line)| PluginEntry::wrap(py, entry, Some(line)))
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, entries)
    }
}

#[pymethods]
impl Plugins {
    #[new]
    fn new(data: Py<PyBytes>) -> Self {
        Plugins { data }
    }

    #[pyo3(signature = (tokens=Vec::new()))]
    fn search<'py>(&self, py: Python<'py>, tokens: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyList>> {
        let data = self.data.bind(py).as_bytes();
        let found = py.detach(|| plugins::search(data, &tokens));
        Self::wrap_all(py, found)
    }

    fn named<'py>(&self, py: Python<'py>, names: HashSet<String>) -> PyResult<Bound<'py, PyList>> {
        let data = self.data.bind(py).as_bytes();
        let found = py.detach(|| plugins::named(data, &names));
        Self::wrap_all(py, found)
    }

    fn names(&self, py: Python<'_>) -> HashSet<String> {
        let data = self.data.bind(py).as_bytes();
        py.detach(|| plugins::names(data))
    }
}

/// `value` as `json.loads` would give it, but for an object's keys, which are in sorted order.
fn to_python<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Null => py.None().into_bound(py),
        Value::Bool(flag) => PyBool::new(py, *flag).to_owned().into_any(),
        Value::Number(number) => {
            if let Some(n) = number.as_i64() {
                n.into_pyobject(py)?.into_any()
            } else if let Some(n) = number.as_u64() {
                n.into_pyobject(py)?.into_any()
            } else {
                number.as_f64().unwrap_or(f64::NAN).into_pyobject(py)?.into_any()
            }
        }
        Value::String(text) => PyString::new(py, text).into_any(),
        Value::Array(items) => {
            let items = items.iter().map(|v| to_python(py, v)).collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, items)?.into_any()
        }
        Value::Object(obj) => object_to_python(py, obj)?.into_any(),
    })
}

fn object_to_python<'py>(py: Python<'py>, obj: &Map<String, Value>) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, v) in obj {
        dict.set_item(key, to_python(py, v)?)?;
    }
    Ok(dict)
}
