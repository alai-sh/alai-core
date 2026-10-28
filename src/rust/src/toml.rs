// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use std::fs;
use std::path::PathBuf;

use alai_core::toml::toml_edit::{Datetime, Item, Offset, TableLike, Time, Value as TomlValue};
use alai_core::toml::{self, Value};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyMapping, PyString, PyType};

use crate::os_error;

create_exception!(
    alai_core.toml,
    TomlError,
    PyValueError,
    "The text isn't valid TOML. The message says why and where."
);
create_exception!(
    alai_core.toml,
    TomlEditError,
    PyException,
    "An edit that can't be made, or that wouldn't give exactly the intended document."
);

fn toml_error(err: alai_core::ParseError) -> PyErr {
    TomlError::new_err(err.to_string())
}

fn edit_error(err: toml::EditError) -> PyErr {
    TomlEditError::new_err(err.to_string())
}

fn read_text(py: Python<'_>, path: &PathBuf) -> PyResult<String> {
    let bytes = py
        .detach(|| fs::read(path))
        .map_err(|err| os_error(err, path.clone()))?;
    toml::decode(bytes).map_err(toml_error)
}

#[pyfunction]
pub(crate) fn toml_loads<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyAny>> {
    let doc = py.detach(|| toml::parse(text)).map_err(toml_error)?;
    table_to_python(py, doc.as_table())
}

#[pyfunction]
pub(crate) fn toml_read<'py>(py: Python<'py>, path: PathBuf) -> PyResult<Bound<'py, PyAny>> {
    let text = read_text(py, &path)?;
    toml_loads(py, &text)
}

#[pyfunction]
#[pyo3(signature = (table, *, width))]
pub(crate) fn toml_dumps(table: &Bound<'_, PyMapping>, width: usize) -> PyResult<String> {
    Ok(toml::dumps(&entries_from(table)?, width))
}

#[pyclass(name = "Document", module = "alai_core.toml")]
pub(crate) struct TomlDocument(toml::Document);

#[pymethods]
impl TomlDocument {
    #[new]
    fn new(py: Python<'_>, text: &str) -> PyResult<Self> {
        py.detach(|| toml::Document::new(text)).map(Self).map_err(toml_error)
    }

    #[classmethod]
    fn from_path(_cls: &Bound<'_, PyType>, py: Python<'_>, path: PathBuf) -> PyResult<Self> {
        let text = read_text(py, &path)?;
        Self::new(py, &text)
    }

    fn __str__(&self) -> &str {
        self.0.text()
    }

    fn data<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        table_to_python(py, self.0.document().as_table())
    }

    fn values(&self, path: Vec<String>) -> PyResult<Vec<String>> {
        self.0.values(&path).map_err(edit_error)
    }

    fn insert(&mut self, path: Vec<String>, index: usize, value: &str) -> PyResult<()> {
        self.0.insert(&path, index, value).map_err(edit_error)
    }

    fn replace(&mut self, path: Vec<String>, index: usize, value: &str) -> PyResult<()> {
        self.0.replace(&path, index, value).map_err(edit_error)
    }

    fn remove(&mut self, path: Vec<String>, index: usize) -> PyResult<()> {
        self.0.remove(&path, index).map_err(edit_error)
    }

    fn set_table(&mut self, path: Vec<String>, value: &Bound<'_, PyMapping>) -> PyResult<()> {
        let entries = entries_from(value)?;
        self.0.set_table(&path, &entries).map_err(edit_error)
    }

    fn remove_key(&mut self, path: Vec<String>) -> PyResult<()> {
        self.0.remove_key(&path).map_err(edit_error)
    }
}

fn entries_from(mapping: &Bound<'_, PyMapping>) -> PyResult<Vec<(String, Value)>> {
    mapping
        .items()?
        .iter()
        .map(|item| {
            let (key, value): (String, Bound<'_, PyAny>) = item.extract()?;
            Ok((key, value_from(&value)?))
        })
        .collect()
}

fn value_from(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    // bool before int, which it's a subclass of.
    if let Ok(flag) = value.cast::<PyBool>() {
        Ok(Value::Boolean(flag.is_true()))
    } else if value.is_instance_of::<PyInt>() {
        Ok(Value::Integer(value.extract()?))
    } else if value.is_instance_of::<PyFloat>() {
        Ok(Value::Float(value.extract()?))
    } else if let Ok(text) = value.cast::<PyString>() {
        Ok(Value::String(text.to_str()?.to_owned()))
    } else if let Ok(mapping) = value.cast::<PyMapping>() {
        Ok(Value::Table(entries_from(mapping)?))
    } else if let Ok(list) = value.cast::<PyList>() {
        Ok(Value::Array(
            list.iter().map(|v| value_from(&v)).collect::<PyResult<_>>()?,
        ))
    } else {
        Err(PyValueError::new_err(format!(
            "can't write a {} as TOML",
            value.get_type().name()?
        )))
    }
}

fn table_to_python<'py>(py: Python<'py>, table: &dyn TableLike) -> PyResult<Bound<'py, PyAny>> {
    let dict = PyDict::new(py);
    for (key, item) in table.iter() {
        dict.set_item(key, item_to_python(py, item)?)?;
    }
    Ok(dict.into_any())
}

fn item_to_python<'py>(py: Python<'py>, item: &Item) -> PyResult<Bound<'py, PyAny>> {
    match item {
        Item::None => Ok(PyDict::new(py).into_any()),
        Item::Value(value) => value_to_python(py, value),
        Item::Table(table) => table_to_python(py, table),
        Item::ArrayOfTables(tables) => {
            let items = tables
                .iter()
                .map(|t| table_to_python(py, t))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyList::new(py, items)?.into_any())
        }
    }
}

fn value_to_python<'py>(py: Python<'py>, value: &TomlValue) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        TomlValue::String(v) => PyString::new(py, v.value()).into_any(),
        TomlValue::Integer(v) => v.value().into_pyobject(py)?.into_any(),
        TomlValue::Float(v) => PyFloat::new(py, *v.value()).into_any(),
        TomlValue::Boolean(v) => PyBool::new(py, *v.value()).to_owned().into_any(),
        TomlValue::Datetime(v) => datetime_to_python(py, v.value())?,
        TomlValue::Array(items) => {
            let items = items
                .iter()
                .map(|v| value_to_python(py, v))
                .collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, items)?.into_any()
        }
        TomlValue::InlineTable(table) => table_to_python(py, table)?,
    })
}

/// As `tomllib` gives it, a `datetime` (aware if it has an offset), a `date` or a `time`, fractional seconds cut to
/// microseconds.
fn datetime_to_python<'py>(py: Python<'py>, value: &Datetime) -> PyResult<Bound<'py, PyAny>> {
    let module = py.import("datetime")?;
    let time = |t: &Time| {
        (
            t.hour,
            t.minute,
            t.second.unwrap_or(0),
            t.nanosecond.unwrap_or(0) / 1000,
        )
    };
    match (value.date, value.time) {
        (Some(d), Some(t)) => {
            let (hour, minute, second, micro) = time(&t);
            let tz = match value.offset {
                None => py.None().into_bound(py),
                Some(Offset::Z) => module.getattr("timezone")?.getattr("utc")?,
                Some(Offset::Custom { minutes }) => {
                    let delta = module.getattr("timedelta")?.call1((0, i32::from(minutes) * 60))?;
                    module.getattr("timezone")?.call1((delta,))?
                }
            };
            module
                .getattr("datetime")?
                .call1((d.year, d.month, d.day, hour, minute, second, micro, tz))
        }
        (Some(d), None) => module.getattr("date")?.call1((d.year, d.month, d.day)),
        (None, Some(t)) => module.getattr("time")?.call1(time(&t)),
        (None, None) => Err(PyValueError::new_err("a datetime with neither a date nor a time")),
    }
}
