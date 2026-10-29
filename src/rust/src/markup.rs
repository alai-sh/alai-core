// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use alai_core::markup;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

#[pyfunction]
#[pyo3(signature = (text, /, *, role = false))]
pub(crate) fn markup_parse<'py>(py: Python<'py>, text: &str, role: bool) -> PyResult<Bound<'py, PyTuple>> {
    PyTuple::new(py, markup::parse(text, role).into_iter().map(Part))
}

#[pyclass(frozen, eq, name = "Part", module = "alai_core.markup")]
#[derive(PartialEq)]
pub(crate) struct Part(markup::Part);

#[pymethods]
impl Part {
    #[getter]
    fn kind(&self) -> &'static str {
        self.0.kind.name()
    }

    #[getter]
    fn text(&self) -> Option<&str> {
        self.0.text.as_deref()
    }

    #[getter]
    fn url(&self) -> Option<&str> {
        self.0.url.as_deref()
    }

    #[getter]
    fn anchor(&self) -> Option<&str> {
        self.0.anchor.as_deref()
    }

    #[getter]
    fn fqcn(&self) -> Option<&str> {
        self.0.fqcn.as_deref()
    }

    #[getter]
    fn plugin_type(&self) -> Option<&str> {
        self.0.plugin_type.as_deref()
    }

    #[getter]
    fn entrypoint(&self) -> Option<&str> {
        self.0.entrypoint.as_deref()
    }

    #[getter]
    fn name(&self) -> Option<&str> {
        self.0.name.as_deref()
    }

    #[getter]
    fn link<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.0.link)
    }

    #[getter]
    fn value(&self) -> Option<&str> {
        self.0.value.as_deref()
    }

    #[getter]
    fn ignore(&self) -> bool {
        self.0.ignore
    }

    #[getter]
    fn message(&self) -> Option<&str> {
        self.0.message.as_deref()
    }

    fn __repr__(&self) -> String {
        let p = &self.0;
        let mut fields = vec![format!("kind={:?}", p.kind.name())];
        let named = [
            ("text", &p.text),
            ("url", &p.url),
            ("anchor", &p.anchor),
            ("fqcn", &p.fqcn),
            ("plugin_type", &p.plugin_type),
            ("entrypoint", &p.entrypoint),
            ("name", &p.name),
            ("value", &p.value),
            ("message", &p.message),
        ];
        fields.extend(
            named
                .iter()
                .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={v:?}"))),
        );
        if p.ignore {
            fields.push("ignore=True".to_owned());
        }
        format!("Part({})", fields.join(", "))
    }
}
