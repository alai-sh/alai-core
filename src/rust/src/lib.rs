// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

mod artifact;
mod collection;
mod extract;
mod index;
mod markup;
mod pep440;
mod plugins;
mod semver;
mod toml;
mod yaml;

use std::io;
use std::path::PathBuf;

use alai_core::ParseError;
use pyo3::exceptions::{PyOSError, PyValueError};
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
    m.add("ExtractError", py.get_type::<artifact::ExtractError>())?;
    m.add_function(wrap_pyfunction!(artifact::artifact_extract, m)?)?;
    m.add_function(wrap_pyfunction!(artifact::artifact_extract_sdist, m)?)?;
    m.add("CollectionMetadata", py.get_type::<collection::Metadata>())?;
    m.add("CollectionManifest", py.get_type::<collection::Manifest>())?;
    m.add("CollectionRuntime", py.get_type::<collection::Runtime>())?;
    m.add("CollectionEdaRuntime", py.get_type::<collection::EdaRuntime>())?;
    m.add("CollectionRoutingEntry", py.get_type::<collection::RoutingEntry>())?;
    m.add("CollectionRemoval", py.get_type::<collection::Removal>())?;
    m.add("CollectionMetadataError", py.get_type::<collection::MetadataError>())?;
    m.add_function(wrap_pyfunction!(collection::collection_check_name, m)?)?;
    m.add("ExtractedCollection", py.get_type::<extract::Collection>())?;
    m.add("ExtractedPlugin", py.get_type::<extract::Plugin>())?;
    m.add("ExtractedPluginDocs", py.get_type::<extract::PluginDocs>())?;
    m.add("ExtractedRouting", py.get_type::<extract::Routing>())?;
    m.add("ExtractedIssue", py.get_type::<extract::Issue>())?;
    m.add("ExtractedDocsError", py.get_type::<extract::DocsError>())?;
    m.add("ExtractedDocFragment", py.get_type::<extract::DocFragment>())?;
    m.add("ExtractedFragmentLookup", py.get_type::<extract::FragmentLookup>())?;
    m.add_function(wrap_pyfunction!(extract::extract_collection, m)?)?;
    m.add("EXTRACT_EDA_LEGACY_ROUTING", extract::eda_legacy_routing(py)?)?;
    m.add_function(wrap_pyfunction!(extract::extract_ansible_core, m)?)?;
    m.add_function(wrap_pyfunction!(extract::extract_ansible_rulebook, m)?)?;
    m.add_function(wrap_pyfunction!(extract::extract_fragment_lookup, m)?)?;
    m.add("MarkupPart", py.get_type::<markup::Part>())?;
    m.add_function(wrap_pyfunction!(markup::markup_parse, m)?)?;
    m.add("IndexConfig", py.get_type::<index::Config>())?;
    m.add("IndexCollections", py.get_type::<index::Collections>())?;
    m.add("IndexCollectionEntry", py.get_type::<index::CollectionEntry>())?;
    m.add("IndexCollectionIndex", py.get_type::<index::CollectionIndex>())?;
    m.add("IndexCollectionVersion", py.get_type::<index::CollectionVersion>())?;
    m.add("NewerFormatError", py.get_type::<index::NewerFormatError>())?;
    m.add("Plugins", py.get_type::<plugins::Plugins>())?;
    m.add("PluginEntry", py.get_type::<plugins::PluginEntry>())?;
    m.add("TomlDocument", py.get_type::<toml::TomlDocument>())?;
    m.add("TomlError", py.get_type::<toml::TomlError>())?;
    m.add("TomlEditError", py.get_type::<toml::TomlEditError>())?;
    m.add_function(wrap_pyfunction!(toml::toml_loads, m)?)?;
    m.add_function(wrap_pyfunction!(toml::toml_read, m)?)?;
    m.add_function(wrap_pyfunction!(toml::toml_dumps, m)?)?;
    m.add("YamlError", py.get_type::<yaml::YamlError>())?;
    m.add("YamlWarning", py.get_type::<yaml::YamlWarning>())?;
    m.add_function(wrap_pyfunction!(yaml::yaml_loads, m)?)?;
    m.add_function(wrap_pyfunction!(yaml::yaml_select, m)?)?;
    Ok(())
}

fn value_error(err: ParseError) -> PyErr {
    PyValueError::new_err(err.to_string())
}

/// `Name('text')`, quoted as Python's repr quotes it.
fn repr(py: Python<'_>, name: &str, text: &str) -> PyResult<String> {
    Ok(format!("{name}({})", PyString::new(py, text).repr()?))
}

/// The filesystem's error as Python's `OSError(errno, strerror, path)`, which picks the subclass, such as
/// `FileNotFoundError`, and names the file. On Windows the code is a Windows error, so it's given as `winerror`, from
/// which Python works out `errno` and the subclass.
fn os_error(err: io::Error, path: PathBuf) -> PyErr {
    let text = err.to_string();
    // io::Error's message ends in " (os error N)", which OSError's strerror doesn't have.
    let strerror = text.split(" (os error ").next().unwrap_or(&text).to_owned();
    match err.raw_os_error() {
        #[cfg(windows)]
        Some(code) => PyOSError::new_err((code, strerror, path, code)),
        #[cfg(not(windows))]
        Some(errno) => PyOSError::new_err((errno, strerror, path)),
        None => PyOSError::new_err(strerror),
    }
}
