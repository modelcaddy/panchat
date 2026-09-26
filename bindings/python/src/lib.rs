//! The native half of the Python package.
//!
//! Deliberately thin: every function here takes plain values and returns the
//! document as JSON text, and `python/panchat/__init__.py` turns that into
//! dicts. The JSON Schema is therefore the whole Python API — there is no
//! second type system to keep in step with the Rust one, and a document read in
//! Python is byte-for-byte the document the CLI prints.

use panchat::export::{self, Branches};
use panchat::{Document, Error, ExportFile};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;

create_exception!(
    _panchat,
    PanchatError,
    PyException,
    "Base class for everything panchat raises."
);
create_exception!(
    _panchat,
    NotRecognized,
    PanchatError,
    "Nothing in the input looked like an export this version can read."
);
create_exception!(
    _panchat,
    MalformedExport,
    PanchatError,
    "The export was recognised but is structurally unreadable."
);

fn to_py(e: Error) -> PyErr {
    match e {
        Error::NotRecognized(m) => NotRecognized::new_err(m),
        Error::Malformed(m) => MalformedExport::new_err(m),
        Error::Json(e) => MalformedExport::new_err(e.to_string()),
        Error::Io(e) => PyErr::from(e),
    }
}

fn to_json(doc: &Document) -> PyResult<String> {
    serde_json::to_string(doc).map_err(|e| PanchatError::new_err(e.to_string()))
}

/// Read a path — an export's folder, its zip, or a single file — and return
/// the document as JSON.
#[pyfunction]
fn load(py: Python<'_>, path: std::path::PathBuf) -> PyResult<String> {
    // Parsing a large export takes seconds; nothing here touches Python
    // objects, so other threads may run meanwhile.
    let doc = py
        .detach(|| panchat::read_path(&path).and_then(|f| panchat::normalize(&f)))
        .map_err(to_py)?;
    to_json(&doc)
}

/// Normalize files already in memory, as `(relative_path, bytes)` pairs — an
/// upload, an object store, a database row. A zip passed this way is expanded
/// exactly as one read from disk would be.
#[pyfunction]
fn normalize_files(py: Python<'_>, files: Vec<(String, Vec<u8>)>) -> PyResult<String> {
    let files: Vec<ExportFile> = files
        .into_iter()
        .map(|(path, bytes)| ExportFile::new(path, bytes))
        .collect();
    let doc = py.detach(|| panchat::normalize(&files)).map_err(to_py)?;
    to_json(&doc)
}

/// What an export is, without parsing all of it, as JSON — or `None`.
#[pyfunction]
fn detect(py: Python<'_>, path: std::path::PathBuf) -> PyResult<Option<String>> {
    let found = py
        .detach(|| panchat::read_path(&path).map(|f| panchat::detect(&f)))
        .map_err(to_py)?;
    Ok(found.map(|d| {
        serde_json::json!({
            "platform": d.platform,
            "variant": d.variant,
            "variant_version": d.variant_version,
            "confidence": d.confidence,
            "notes": d.notes,
        })
        .to_string()
    }))
}

/// Render a document (as JSON text) to `markdown`, `jsonl` or `turns`.
#[pyfunction]
#[pyo3(signature = (document, format, all_branches = false))]
fn render(document: &str, format: &str, all_branches: bool) -> PyResult<String> {
    let doc: Document =
        serde_json::from_str(document).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let branches = match all_branches {
        true => Branches::All,
        false => Branches::ActiveOnly,
    };
    let json_err = |e: serde_json::Error| PanchatError::new_err(e.to_string());
    match format {
        "markdown" => Ok(doc
            .conversations
            .iter()
            .map(|c| export::to_markdown(c, branches))
            .collect::<Vec<_>>()
            .join("\n---\n\n")),
        "jsonl" => export::to_jsonl(&doc).map_err(json_err),
        "turns" => export::to_turns_jsonl(&doc, branches).map_err(json_err),
        other => Err(PyValueError::new_err(format!(
            "unknown format {other:?}; expected markdown, jsonl or turns"
        ))),
    }
}

#[pymodule]
fn _panchat(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(normalize_files, m)?)?;
    m.add_function(wrap_pyfunction!(detect, m)?)?;
    m.add_function(wrap_pyfunction!(render, m)?)?;
    m.add("PanchatError", m.py().get_type::<PanchatError>())?;
    m.add("NotRecognized", m.py().get_type::<NotRecognized>())?;
    m.add("MalformedExport", m.py().get_type::<MalformedExport>())?;
    m.add("FORMAT_VERSION", panchat::FORMAT_VERSION)?;
    m.add("SCHEMA_URL", panchat::SCHEMA_URL)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
