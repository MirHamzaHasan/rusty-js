//! A deliberately bounded ESM package resolver. No V8 handles or dependency downloads.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde_json::{Map, Value};
use url::Url;

use crate::JsError;
use crate::builtins::Builtin;
use crate::modules::{ModuleId, canonical_file};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum LoadMode {
    Import,
    Require,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Format {
    Esm,
    CommonJs,
    Json,
}

pub(crate) fn format(file: &Path, origin: &str) -> Result<Format, JsError> {
    match file.extension().and_then(|extension| extension.to_str()) {
        Some("mjs") => Ok(Format::Esm),
        Some("cjs") => Ok(Format::CommonJs),
        Some("json") => Ok(Format::Json),
        Some("js") => {
            let is_module = nearest_scope(file, origin)?
                .as_ref()
                .and_then(|scope| scope.value.get("type"))
                .is_some_and(|kind| kind == "module");
            Ok(if is_module {
                Format::Esm
            } else {
                Format::CommonJs
            })
        }
        _ => Err(JsError::host(
            &format!("unsupported imported module format: {}", file.display()),
            origin,
        )),
    }
}

fn validate_mode(file: &Path, origin: &str, mode: LoadMode) -> Result<(), JsError> {
    match (mode, format(file, origin)?) {
        (LoadMode::Require, Format::Esm) => Err(JsError::host(
            "require() of ES modules is not supported; use import()",
            origin,
        )),
        (LoadMode::Import, Format::Json) => Err(JsError::host(
            "JSON imports are not supported; JSON can be loaded with require()",
            origin,
        )),
        _ => Ok(()),
    }
}

struct Manifest {
    directory: PathBuf,
    value: Value,
}

impl Manifest {
    fn error(&self, message: &str, origin: &str) -> JsError {
        JsError::host(
            &format!(
                "{}: {message}",
                self.directory.join("package.json").display()
            ),
            origin,
        )
    }
}

fn read_manifest(directory: &Path, origin: &str) -> Result<Option<Manifest>, JsError> {
    let path = directory.join("package.json");
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(JsError::host(
                &format!("cannot read {}: {error}", path.display()),
                origin,
            ));
        }
    };
    let value: Value = serde_json::from_str(&source)
        .map_err(|error| JsError::host(&format!("invalid {}: {error}", path.display()), origin))?;
    let manifest = Manifest {
        directory: directory.to_path_buf(),
        value,
    };
    if !manifest.value.is_object() {
        return Err(manifest.error("package metadata must be a JSON object", origin));
    }
    if let Some(kind) = manifest.value.get("type")
        && kind != "module"
        && kind != "commonjs"
    {
        return Err(manifest.error("package type must be module or commonjs", origin));
    }
    Ok(Some(manifest))
}

fn nearest_scope(file: &Path, origin: &str) -> Result<Option<Manifest>, JsError> {
    for directory in file.parent().into_iter().flat_map(Path::ancestors) {
        if directory
            .file_name()
            .is_some_and(|name| name == "node_modules")
        {
            break;
        }
        if let Some(manifest) = read_manifest(directory, origin)? {
            return Ok(Some(manifest));
        }
    }
    Ok(None)
}

pub(crate) fn validate_format(
    file: &Path,
    origin: &str,
    explicit_entry: bool,
) -> Result<(), JsError> {
    if explicit_entry {
        if file.extension().is_some_and(|extension| extension == "cjs") {
            return Err(JsError::host(
                "a .cjs file cannot be forced into ES module mode",
                origin,
            ));
        }
        return Ok(());
    }
    validate_mode(file, origin, LoadMode::Import)
}

fn parse_specifier(specifier: &str, origin: &str) -> Result<(String, String), JsError> {
    let invalid = || JsError::host(&format!("invalid package specifier {specifier:?}"), origin);
    if specifier.starts_with('#') {
        return Err(JsError::host(
            "imports targets cannot alias another #import",
            origin,
        ));
    }
    if specifier.is_empty() || specifier.contains(['\\', ':', '?', '#', '*', '\0']) {
        return Err(invalid());
    }
    let parts = specifier.split('/').collect::<Vec<_>>();
    let name_parts = if specifier.starts_with('@') { 2 } else { 1 };
    if parts.len() < name_parts || (name_parts == 2 && parts[0].len() == 1) {
        return Err(invalid());
    }
    if parts.iter().enumerate().any(|(index, part)| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || (index < name_parts && (part.starts_with('.') || part.contains('%')))
            || part.eq_ignore_ascii_case("node_modules")
    }) {
        return Err(invalid());
    }
    let name = parts[..name_parts].join("/");
    let subpath = if parts.len() == name_parts {
        ".".to_owned()
    } else {
        format!("./{}", parts[name_parts..].join("/"))
    };
    if subpath != "." {
        validate_target(&subpath).map_err(|_| invalid())?;
    }
    Ok((name, subpath))
}

pub(crate) fn resolve(specifier: &str, referrer: &str) -> Result<ModuleId, JsError> {
    resolve_mode(specifier, referrer, LoadMode::Import)
}

pub(crate) fn resolve_mode(
    specifier: &str,
    referrer: &str,
    mode: LoadMode,
) -> Result<ModuleId, JsError> {
    if specifier.starts_with('#') {
        return resolve_imports(specifier, referrer, mode);
    }
    resolve_package(specifier, referrer, mode).map(ModuleId::File)
}

fn resolve_package(specifier: &str, referrer: &str, mode: LoadMode) -> Result<PathBuf, JsError> {
    let (name, subpath) = parse_specifier(specifier, referrer)?;
    if referrer == "<eval>" {
        return Err(JsError::host(
            "package lookup requires a file referrer",
            referrer,
        ));
    }
    let base = referrer_path(referrer, mode)?;
    if let Some(scope) = nearest_scope(&base, referrer)?
        && scope.value.get("name").and_then(Value::as_str) == Some(&name)
        && scope
            .value
            .get("exports")
            .is_some_and(|exports| !exports.is_null())
    {
        return resolve_in_package(&scope.directory, Some(&scope), &subpath, referrer, mode);
    }
    for directory in base.parent().into_iter().flat_map(Path::ancestors) {
        if directory
            .file_name()
            .is_some_and(|part| part == "node_modules")
        {
            continue;
        }
        let candidate = directory.join("node_modules").join(&name);
        match std::fs::metadata(&candidate) {
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                if mode == LoadMode::Require
                    && let Some(file) = require_file(&candidate, referrer)?
                {
                    return Ok(file);
                }
                continue;
            }
            Err(error) => {
                return Err(JsError::host(
                    &format!("cannot inspect package {}: {error}", candidate.display()),
                    referrer,
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                if mode == LoadMode::Require
                    && let Some(file) = require_file(&candidate, referrer)?
                {
                    return Ok(file);
                }
                return Err(JsError::host(
                    &format!("package is not a directory: {}", candidate.display()),
                    referrer,
                ));
            }
            Ok(_) => {}
        }
        let root = std::fs::canonicalize(&candidate).map_err(|error| {
            JsError::host(
                &format!("cannot resolve package {}: {error}", candidate.display()),
                referrer,
            )
        })?;
        let manifest = read_manifest(&root, referrer)?;
        return resolve_in_package(&root, manifest.as_ref(), &subpath, referrer, mode);
    }
    Err(JsError::host(
        &format!("package {name:?} was not found in node_modules"),
        referrer,
    ))
}

fn resolve_in_package(
    root: &Path,
    manifest: Option<&Manifest>,
    subpath: &str,
    origin: &str,
    mode: LoadMode,
) -> Result<PathBuf, JsError> {
    let fail = |message: &str| match manifest {
        Some(manifest) => manifest.error(message, origin),
        None => JsError::host(message, origin),
    };
    let target = if let Some(exports) = manifest
        .and_then(|manifest| manifest.value.get("exports"))
        .filter(|value| !value.is_null())
    {
        let selected = if let Some(map) = exports.as_object() {
            let has_subpaths = map.keys().any(|key| key.starts_with('.'));
            if has_subpaths {
                if map.keys().any(|key| !key.starts_with('.')) {
                    return Err(fail("exports cannot mix subpath and condition keys"));
                }
                match_mapping(map, subpath, false).map_err(|error| fail(&error.message()))?
            } else if subpath == "." {
                Some((exports, None))
            } else {
                None
            }
        } else if subpath == "." {
            Some((exports, None))
        } else {
            None
        };
        match selected
            .map(|(value, capture)| select_target(value, capture, 0, false, mode))
            .transpose()
            .map_err(|error| fail(&error.message()))?
        {
            Some(Target::Selected(target)) => target,
            _ => {
                return Err(fail(&format!(
                    "package subpath {subpath:?} is not exported"
                )));
            }
        }
    } else if subpath != "." {
        subpath.to_owned()
    } else if let Some(main) = manifest.and_then(|manifest| manifest.value.get("main")) {
        let main = main.as_str().ok_or_else(|| fail("main must be a string"))?;
        if main.starts_with("./") {
            main.to_owned()
        } else {
            format!("./{main}")
        }
    } else {
        "./index.js".to_owned()
    };
    if mode == LoadMode::Require
        && manifest
            .and_then(|manifest| manifest.value.get("exports"))
            .is_none_or(Value::is_null)
    {
        let path = if subpath == "." {
            root.to_path_buf()
        } else {
            validate_target(&target).map_err(|message| fail(&message))?;
            target_path(root, &target, origin)?
        };
        let file = legacy_require(&path, origin)?;
        if !file.starts_with(root) {
            return Err(fail(
                "package target escapes its canonical package directory",
            ));
        }
        return Ok(file);
    }
    resolve_file_target(root, &target, origin, mode)
}

fn target_path(root: &Path, target: &str, origin: &str) -> Result<PathBuf, JsError> {
    let fail = |message: &str| {
        JsError::host(
            &format!("{}: {message}", root.join("package.json").display()),
            origin,
        )
    };
    validate_target(target).map_err(|message| fail(&message))?;
    let url = Url::from_directory_path(root)
        .map_err(|_| fail("invalid package directory"))?
        .join(target)
        .map_err(|_| fail("invalid package target URL"))?;
    url.to_file_path()
        .map_err(|_| fail("package target is not a local path"))
}

fn resolve_file_target(
    root: &Path,
    target: &str,
    origin: &str,
    mode: LoadMode,
) -> Result<PathBuf, JsError> {
    let file = target_path(root, target, origin)?;
    let file = canonical_file(&file, origin)?;
    if !file.starts_with(root) {
        return Err(JsError::host(
            "package target escapes its canonical package directory",
            origin,
        ));
    }
    validate_mode(&file, origin, mode)?;
    Ok(file)
}

fn probe_require_file(path: &Path, origin: &str) -> Result<Option<PathBuf>, JsError> {
    match std::fs::metadata(path) {
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            Ok(None)
        }
        Err(error) => Err(JsError::host(
            &format!("cannot inspect {}: {error}", path.display()),
            origin,
        )),
        Ok(metadata) if metadata.is_dir() => Ok(None),
        Ok(_) => {
            let file = canonical_file(path, origin)?;
            validate_mode(&file, origin, LoadMode::Require)?;
            Ok(Some(file))
        }
    }
}

fn require_file(path: &Path, origin: &str) -> Result<Option<PathBuf>, JsError> {
    if let Some(file) = probe_require_file(path, origin)? {
        return Ok(Some(file));
    }
    for extension in [".js", ".json", ".node"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(extension);
        if let Some(file) = probe_require_file(Path::new(&candidate), origin)? {
            return Ok(Some(file));
        }
    }
    Ok(None)
}

fn require_index(path: &Path, origin: &str) -> Result<Option<PathBuf>, JsError> {
    for name in ["index.js", "index.json", "index.node"] {
        if let Some(file) = probe_require_file(&path.join(name), origin)? {
            return Ok(Some(file));
        }
    }
    Ok(None)
}

pub(crate) fn legacy_require(path: &Path, origin: &str) -> Result<PathBuf, JsError> {
    if let Some(file) = require_file(path, origin)? {
        return Ok(file);
    }
    if path.is_dir() {
        if let Some(manifest) = read_manifest(path, origin)?
            && let Some(main) = manifest.value.get("main")
        {
            let main = main
                .as_str()
                .ok_or_else(|| manifest.error("main must be a string", origin))?;
            let target = if main == "." || main == "./" {
                path.to_path_buf()
            } else {
                let main = if main.starts_with("./") {
                    main.to_owned()
                } else {
                    format!("./{main}")
                };
                target_path(path, &main, origin)?
            };
            if let Some(file) = require_file(&target, origin)? {
                return Ok(file);
            }
            if let Some(file) = require_index(&target, origin)? {
                return Ok(file);
            }
        }
        if let Some(file) = require_index(path, origin)? {
            return Ok(file);
        }
    }
    Err(JsError::host(
        &format!("cannot find CommonJS module {}", path.display()),
        origin,
    ))
}

enum Target {
    Unmatched,
    Blocked,
    Selected(String),
}

enum TargetError {
    InvalidTarget(String),
    InvalidConfig(String),
}

impl TargetError {
    fn message(self) -> String {
        match self {
            Self::InvalidTarget(message) | Self::InvalidConfig(message) => message,
        }
    }
}

/// Exact keys win; otherwise prefer the longest prefix, then the longest trailer.
fn match_mapping<'a, 'b>(
    map: &'a Map<String, Value>,
    request: &'b str,
    internal: bool,
) -> Result<Option<(&'a Value, Option<&'b str>)>, TargetError> {
    for key in map.keys() {
        let path = if internal {
            if !key.starts_with('#') || key == "#" || key.starts_with("#/") {
                return Err(TargetError::InvalidConfig("invalid imports key".into()));
            }
            format!("./{}", &key[1..])
        } else {
            if key == "." {
                continue;
            }
            key.clone()
        };
        if key.matches('*').count() > 1 || validate_target(&path.replace('*', "pattern")).is_err() {
            return Err(TargetError::InvalidConfig(
                "invalid package map key; use exact paths or one wildcard".into(),
            ));
        }
    }
    if let Some(value) = map.get(request) {
        return Ok(Some((value, None)));
    }
    let mut best = None;
    let mut rank = (0, 0);
    for (key, value) in map {
        let Some((prefix, trailer)) = key.split_once('*') else {
            continue;
        };
        if request.starts_with(prefix)
            && request.ends_with(trailer)
            && request.len() > prefix.len() + trailer.len()
            && (prefix.len(), key.len()) > rank
        {
            rank = (prefix.len(), key.len());
            best = Some((
                value,
                Some(&request[prefix.len()..request.len() - trailer.len()]),
            ));
        }
    }
    Ok(best)
}

fn select_target(
    value: &Value,
    capture: Option<&str>,
    depth: usize,
    internal: bool,
    mode: LoadMode,
) -> Result<Target, TargetError> {
    if depth > 32 {
        return Err(TargetError::InvalidConfig(
            "package target nesting exceeds 32 levels".into(),
        ));
    }
    match value {
        Value::String(target) => {
            // Validate the template before substitution can hide invalid segments.
            let path = if let Some(capture) = capture {
                validate_mapped_target(&target.replace('*', "pattern"), internal)
                    .map_err(TargetError::InvalidTarget)?;
                target.replace('*', capture)
            } else {
                target.clone()
            };
            validate_mapped_target(&path, internal).map_err(TargetError::InvalidTarget)?;
            Ok(Target::Selected(path))
        }
        Value::Null => Ok(Target::Blocked),
        Value::Array(items) => {
            if items.is_empty() {
                return Ok(Target::Blocked);
            }
            let mut last = Ok(Target::Unmatched);
            for item in items {
                match select_target(item, capture, depth + 1, internal, mode) {
                    Ok(Target::Unmatched) => {}
                    Ok(Target::Blocked) => last = Ok(Target::Blocked),
                    Ok(path) => return Ok(path),
                    Err(error @ TargetError::InvalidTarget(_)) => last = Err(error),
                    Err(error) => return Err(error),
                }
            }
            last
        }
        Value::Object(conditions) => {
            if conditions.keys().any(|key| {
                key.starts_with('.')
                    || key
                        .parse::<u32>()
                        .is_ok_and(|index| index < u32::MAX && index.to_string() == *key)
            }) {
                return Err(TargetError::InvalidConfig(
                    "invalid package condition key".into(),
                ));
            }
            for (condition, target) in conditions {
                if condition == "default"
                    || condition
                        == if mode == LoadMode::Import {
                            "import"
                        } else {
                            "require"
                        }
                {
                    match select_target(target, capture, depth + 1, internal, mode)? {
                        Target::Unmatched => {}
                        target => return Ok(target),
                    }
                }
            }
            Ok(Target::Unmatched)
        }
        _ => Err(TargetError::InvalidTarget(
            "invalid package target; expected a string, array, condition object, or null".into(),
        )),
    }
}

fn validate_mapped_target(target: &str, internal: bool) -> Result<(), String> {
    if !internal || target.starts_with("./") {
        return validate_target(target);
    }
    if target.starts_with("rustyjs:") && !target.contains(['*', '?', '#', '\\', '\0']) {
        return Ok(());
    }
    parse_specifier(target, "<package target>")
        .map(|_| ())
        .map_err(|_| "imports target must be a ./ path, a package name, or a rustyjs: built-in; # aliases and other URLs are unsupported".into())
}

fn resolve_imports(specifier: &str, referrer: &str, mode: LoadMode) -> Result<ModuleId, JsError> {
    let fail = |message: &str| JsError::host(message, referrer);
    if referrer == "<eval>" {
        return Err(fail("package imports require a file referrer"));
    }
    if specifier == "#"
        || specifier.starts_with("#/")
        || validate_target(&format!("./{}", &specifier[1..])).is_err()
    {
        return Err(fail("invalid package import specifier"));
    }
    let base = referrer_path(referrer, mode)?;
    let manifest = nearest_scope(&base, referrer)?.ok_or_else(|| {
        fail(&format!(
            "package import {specifier:?} is not defined; no package scope"
        ))
    })?;
    let fail = |message: &str| manifest.error(message, referrer);
    let map = manifest
        .value
        .get("imports")
        .ok_or_else(|| {
            fail(&format!(
                "package import {specifier:?} is not defined in the nearest scope"
            ))
        })?
        .as_object()
        .ok_or_else(|| fail("package imports requires an object map in the nearest scope"))?;
    let selected = match_mapping(map, specifier, true).map_err(|error| fail(&error.message()))?;
    let target = match selected
        .map(|(value, capture)| select_target(value, capture, 0, true, mode))
        .transpose()
        .map_err(|error| fail(&error.message()))?
    {
        Some(Target::Selected(target)) => target,
        _ => {
            return Err(fail(&format!(
                "package import {specifier:?} is not defined"
            )));
        }
    };
    if target.starts_with("./") {
        return resolve_file_target(&manifest.directory, &target, referrer, mode)
            .map(ModuleId::File);
    }
    if target.starts_with("rustyjs:") {
        return Builtin::resolve(&target)
            .map(ModuleId::Builtin)
            .ok_or_else(|| fail(&format!("unknown built-in module {target:?}")));
    }
    // External aliases resolve from the manifest, not a nested importing file.
    resolve_package(
        &target,
        &manifest.directory.join("package.json").to_string_lossy(),
        mode,
    )
    .map(ModuleId::File)
    .map_err(|mut error| {
        error.origin = referrer.into();
        error
    })
}

fn referrer_path(referrer: &str, mode: LoadMode) -> Result<PathBuf, JsError> {
    if mode == LoadMode::Require && Path::new(referrer).is_absolute() {
        // Bound require data comes from a loaded canonical module or createRequire's
        // explicit absolute anchor; a virtual anchor is not a source file to read.
        Ok(PathBuf::from(referrer))
    } else {
        canonical_file(Path::new(referrer), referrer)
    }
}

fn validate_target(target: &str) -> Result<(), String> {
    let Some(relative) = target.strip_prefix("./") else {
        return Err("package target must start with ./".into());
    };
    if target.contains(['\\', '?', '#', '*']) {
        return Err("unsupported package target characters".into());
    }
    for segment in relative.split('/') {
        let decoded = percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| "package target must use UTF-8 paths")?;
        if decoded.is_empty()
            || decoded == "."
            || decoded == ".."
            || decoded.eq_ignore_ascii_case("node_modules")
            || decoded.contains(['/', '\\', '\0', ':'])
        {
            return Err("invalid package target segment or path traversal".into());
        }
    }
    Ok(())
}
