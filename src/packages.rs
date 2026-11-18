//! A deliberately bounded ESM package resolver. No V8 handles or dependency downloads.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde_json::Value;
use url::Url;

use crate::JsError;
use crate::modules::canonical_file;

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
    match file.extension().and_then(|extension| extension.to_str()) {
        Some("cjs") => Err(JsError::host("CommonJS (.cjs) is not supported", origin)),
        Some("mjs") => Ok(()),
        _ if explicit_entry => Ok(()),
        Some("js") => {
            if nearest_scope(file, origin)?
                .as_ref()
                .and_then(|scope| scope.value.get("type"))
                .is_some_and(|kind| kind == "module")
            {
                Ok(())
            } else {
                Err(JsError::host(
                    &format!(
                        "CommonJS or untyped .js import is not supported: {}; use .mjs or package.json type module",
                        file.display()
                    ),
                    origin,
                ))
            }
        }
        _ => Err(JsError::host(
            &format!("unsupported imported module format: {}", file.display()),
            origin,
        )),
    }
}

fn parse_specifier(specifier: &str, origin: &str) -> Result<(String, String), JsError> {
    let invalid = || JsError::host(&format!("invalid package specifier {specifier:?}"), origin);
    if specifier.starts_with('#') {
        return Err(JsError::host(
            "package imports (#imports) are not supported",
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

pub(crate) fn resolve(specifier: &str, referrer: &str) -> Result<PathBuf, JsError> {
    let (name, subpath) = parse_specifier(specifier, referrer)?;
    if referrer == "<eval>" {
        return Err(JsError::host(
            "package lookup requires a file referrer",
            referrer,
        ));
    }
    let base = canonical_file(Path::new(referrer), referrer)?;
    if let Some(scope) = nearest_scope(&base, referrer)?
        && scope.value.get("name").and_then(Value::as_str) == Some(&name)
        && scope
            .value
            .get("exports")
            .is_some_and(|exports| !exports.is_null())
    {
        return resolve_in_package(&scope.directory, Some(&scope), &subpath, referrer);
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
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(JsError::host(
                    &format!("cannot inspect package {}: {error}", candidate.display()),
                    referrer,
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
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
        return resolve_in_package(&root, manifest.as_ref(), &subpath, referrer);
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
                if map.keys().any(|key| key.contains('*')) {
                    return Err(fail("exports patterns are not supported"));
                }
                if map.keys().any(|key| key != "." && !key.starts_with("./")) {
                    return Err(fail("invalid exports subpath key"));
                }
                map.get(subpath)
            } else if subpath == "." {
                Some(exports)
            } else {
                None
            }
        } else if subpath == "." {
            Some(exports)
        } else {
            None
        };
        match selected
            .map(|value| select_target(value, 0))
            .transpose()
            .map_err(|message| fail(&message))?
        {
            Some(Target::Path(target)) => target.to_owned(),
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
    validate_target(&target).map_err(|message| fail(&message))?;
    let url = Url::from_directory_path(root)
        .map_err(|_| fail("invalid package directory"))?
        .join(&target)
        .map_err(|_| fail("invalid package target URL"))?;
    let file = url
        .to_file_path()
        .map_err(|_| fail("package target is not a local path"))?;
    let file = canonical_file(&file, origin)?;
    if !file.starts_with(root) {
        return Err(fail(
            "package target escapes its canonical package directory",
        ));
    }
    validate_format(&file, origin, false)?;
    Ok(file)
}

enum Target<'a> {
    Unmatched,
    Blocked,
    Path(&'a str),
}

fn select_target(value: &Value, depth: usize) -> Result<Target<'_>, String> {
    if depth > 32 {
        return Err("exports condition nesting exceeds 32 levels".into());
    }
    match value {
        Value::String(target) => Ok(Target::Path(target)),
        Value::Null => Ok(Target::Blocked),
        Value::Array(_) => Err("exports fallback arrays are not supported".into()),
        Value::Object(conditions) => {
            if conditions
                .keys()
                .any(|key| key.starts_with('.') || key.parse::<u32>().is_ok())
            {
                return Err("invalid exports condition key".into());
            }
            for (condition, target) in conditions {
                if condition == "import" || condition == "default" {
                    match select_target(target, depth + 1)? {
                        Target::Unmatched => {}
                        target => return Ok(target),
                    }
                }
            }
            Ok(Target::Unmatched)
        }
        _ => Err("invalid exports target; expected a string, condition object, or null".into()),
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
