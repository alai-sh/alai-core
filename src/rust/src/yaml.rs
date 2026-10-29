// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use alai_core::yaml::{self, Document, Value, Warning, YamlError as Error};
use pyo3::create_exception;
use pyo3::exceptions::{PyTypeError, PyUserWarning, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};

create_exception!(
    alai_core.yaml,
    YamlError,
    PyValueError,
    "The text isn't YAML Ansible could read. The message says why and where."
);
create_exception!(
    alai_core.yaml,
    YamlWarning,
    PyUserWarning,
    "Something in a document worth knowing about that didn't stop it being read."
);

/// `on_warning` as given: left out, which is `warnings.warn`, None to ignore warnings, or a callable.
pub(crate) enum OnWarning<'py> {
    Default,
    Silent,
    Call(Bound<'py, PyAny>),
}

impl<'py> FromPyObject<'_, 'py> for OnWarning<'py> {
    type Error = PyErr;

    fn extract(ob: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        Ok(if ob.is_none() {
            OnWarning::Silent
        } else {
            OnWarning::Call(ob.to_owned())
        })
    }
}

/// `message` after `name`, as `name: message`, when there is one.
pub(crate) fn named(name: Option<&str>, message: String) -> String {
    match name {
        Some(name) => format!("{name}: {message}"),
        None => message,
    }
}

/// The text as UTF-8, or a `YamlError` for one, such as a string decoded with `surrogateescape`, that has a lone
/// surrogate, which no YAML parser can read.
fn utf8<'a>(py: Python<'_>, text: &'a Bound<'_, PyString>, name: Option<&str>) -> PyResult<&'a str> {
    text.to_str().map_err(|err| {
        let message = format!("the text isn't valid Unicode: {}", err.value(py));
        yaml_error(
            py,
            Error {
                message,
                line: None,
                column: None,
            },
            name,
        )
    })
}

pub(crate) fn yaml_error(py: Python<'_>, err: Error, name: Option<&str>) -> PyErr {
    let raised = YamlError::new_err(named(name, err.to_string()));
    let value = raised.value(py);
    let set = value
        .setattr("name", name)
        .and_then(|()| value.setattr("line", err.line))
        .and_then(|()| value.setattr("column", err.column));
    set.err().unwrap_or(raised)
}

#[pyfunction]
#[pyo3(signature = (text, /, *, name=None, on_warning=OnWarning::Default))]
pub(crate) fn yaml_loads<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    name: Option<&str>,
    on_warning: OnWarning<'py>,
) -> PyResult<Bound<'py, PyAny>> {
    let text = utf8(py, text, name)?;
    let doc = py
        .detach(|| yaml::loads(text))
        .map_err(|err| yaml_error(py, err, name))?;
    let value = Converter::new(&doc).value(py, &doc.root)?;
    warn(py, &doc.warnings, name, on_warning)?;
    Ok(value)
}

#[pyfunction]
#[pyo3(signature = (text, keys, /, *, name=None, on_warning=OnWarning::Default))]
pub(crate) fn yaml_select<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    keys: &Bound<'py, PyAny>,
    name: Option<&str>,
    on_warning: OnWarning<'py>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    // A str is an iterable of strings too, but never what's meant.
    if keys.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err("keys must be an iterable of strings, not a str"));
    }
    let keys: Vec<String> = keys.try_iter()?.map(|key| key?.extract()).collect::<PyResult<_>>()?;
    let text = utf8(py, text, name)?;
    let wanted: Vec<&str> = keys.iter().map(String::as_str).collect();
    let doc = py
        .detach(|| yaml::select(text, &wanted))
        .map_err(|err| yaml_error(py, err, name))?;
    let Some(doc) = doc else {
        return Ok(None);
    };
    let value = Converter::new(&doc).value(py, &doc.root)?;
    warn(py, &doc.warnings, name, on_warning)?;
    Ok(Some(value))
}

/// Hand each warning to `on_warning`, once the document is built, so the parse never waits on Python.
pub(crate) fn warn(
    py: Python<'_>,
    warnings: &[Warning],
    name: Option<&str>,
    on_warning: OnWarning<'_>,
) -> PyResult<()> {
    let on_warning = match on_warning {
        OnWarning::Silent => return Ok(()),
        OnWarning::Call(f) => f,
        OnWarning::Default if warnings.is_empty() => return Ok(()),
        OnWarning::Default => py.import("warnings")?.getattr("warn")?,
    };
    for warning in warnings {
        let message = named(
            name,
            format!(
                "{} (at line {}, column {})",
                warning.message, warning.line, warning.column
            ),
        );
        let instance = YamlWarning::new_err(message).into_value(py).into_bound(py);
        instance.setattr("kind", warning.kind)?;
        instance.setattr("name", name)?;
        instance.setattr("line", warning.line)?;
        instance.setattr("column", warning.column)?;
        on_warning.call1((instance,))?;
    }
    Ok(())
}

/// Builds Python objects from a document, each anchored node once, so every alias to it is the same object, as
/// PyYAML gives it.
pub(crate) struct Converter<'d> {
    doc: &'d Document,
    built: Vec<Option<Py<PyAny>>>,
}

impl<'d> Converter<'d> {
    pub(crate) fn new(doc: &'d Document) -> Self {
        Converter {
            doc,
            built: (0..doc.anchors.len()).map(|_| None).collect(),
        }
    }

    pub(crate) fn value<'py>(&mut self, py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
        Ok(match value {
            Value::Null => py.None().into_bound(py),
            Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
            Value::Int(n) => n.into_pyobject(py)?.into_any(),
            Value::BigInt(digits) => py.get_type::<PyInt>().call1((digits.as_str(),))?,
            Value::Float(f) => PyFloat::new(py, *f).into_any(),
            Value::String(s) => PyString::new(py, s).into_any(),
            Value::Sequence(items) => {
                let list = PyList::empty(py);
                for item in items {
                    list.append(self.value(py, item)?)?;
                }
                list.into_any()
            }
            Value::Mapping(entries) => {
                let dict = PyDict::new(py);
                for (key, item) in entries {
                    dict.set_item(self.value(py, key)?, self.value(py, item)?)?;
                }
                dict.into_any()
            }
            Value::Alias(id) => {
                if let Some(built) = &self.built[*id] {
                    return Ok(built.bind(py).clone());
                }
                let built = self.value(py, &self.doc.anchors[*id])?;
                self.built[*id] = Some(built.clone().unbind());
                built
            }
        })
    }
}
