// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

mod index;
mod pep440;
mod semver;

use alai_core::ParseError;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyString;

// Without gil_used = false, a free-threaded Python turns the GIL back on for the whole process when importing this.
#[pymodule(gil_used = false)]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Prefixed here, as both modules have a Version. alai_core.semver and alai_core.pep440 give them their names.
    let py = m.py();
    m.add("SemverVersion", py.get_type::<semver::Version>())?;
    m.add("SemverVersionReq", py.get_type::<semver::VersionReq>())?;
    m.add("Pep440Version", py.get_type::<pep440::Version>())?;
    m.add("Pep440SpecifierSet", py.get_type::<pep440::SpecifierSet>())?;
    m.add("IndexConfig", py.get_type::<index::Config>())?;
    m.add("IndexCollections", py.get_type::<index::Collections>())?;
    m.add("IndexCollectionEntry", py.get_type::<index::CollectionEntry>())?;
    m.add("IndexCollectionIndex", py.get_type::<index::CollectionIndex>())?;
    m.add("IndexCollectionVersion", py.get_type::<index::CollectionVersion>())?;
    m.add("NewerFormatError", py.get_type::<index::NewerFormatError>())?;
    Ok(())
}

fn value_error(err: ParseError) -> PyErr {
    PyValueError::new_err(err.to_string())
}

/// `Name('text')`, quoted as Python's repr quotes it.
fn repr(py: Python<'_>, name: &str, text: &str) -> PyResult<String> {
    Ok(format!("{name}({})", PyString::new(py, text).repr()?))
}
