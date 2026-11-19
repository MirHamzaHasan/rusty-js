//! Built-in exports captured before guest code can replace global properties.

use crate::modules::SharedModules;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Builtin {
    Timers,
    Module,
    Events,
    Buffer,
    Stream,
    StreamPromises,
    Path,
    PathPosix,
    PathWin32,
}

impl Builtin {
    pub fn resolve(specifier: &str) -> Option<Self> {
        match specifier {
            "rustyjs:timers" => Some(Self::Timers),
            "rustyjs:module" => Some(Self::Module),
            "rustyjs:events" | "node:events" | "events" => Some(Self::Events),
            "rustyjs:buffer" | "node:buffer" | "buffer" => Some(Self::Buffer),
            "rustyjs:stream" | "node:stream" | "stream" => Some(Self::Stream),
            "rustyjs:stream/promises" | "node:stream/promises" | "stream/promises" => {
                Some(Self::StreamPromises)
            }
            "rustyjs:path" | "node:path" | "path" => Some(Self::Path),
            "rustyjs:path/posix" | "node:path/posix" | "path/posix" => Some(Self::PathPosix),
            "rustyjs:path/win32" | "node:path/win32" | "path/win32" => Some(Self::PathWin32),
            _ => None,
        }
    }

    pub fn specifier(self) -> &'static str {
        match self {
            Self::Timers => "rustyjs:timers",
            Self::Module => "rustyjs:module",
            Self::Events => "rustyjs:events",
            Self::Buffer => "rustyjs:buffer",
            Self::Stream => "rustyjs:stream",
            Self::StreamPromises => "rustyjs:stream/promises",
            Self::Path => "rustyjs:path",
            Self::PathPosix => "rustyjs:path/posix",
            Self::PathWin32 => "rustyjs:path/win32",
        }
    }

    fn export_names(self) -> &'static [&'static str] {
        match self {
            Self::Timers => &TIMER_NAMES,
            Self::Module => &["createRequire"],
            Self::Events => &["default", "EventEmitter"],
            Self::Buffer => &["Buffer"],
            Self::Stream => &["Writable", "Readable", "finished", "pipeline", "promises"],
            Self::StreamPromises => &["finished", "pipeline"],
            Self::Path | Self::PathPosix | Self::PathWin32 => &PATH_NAMES,
        }
    }
}

const TIMER_NAMES: [&str; 5] = [
    "sleep",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
];

const PATH_NAMES: [&str; 16] = [
    "default",
    "resolve",
    "normalize",
    "isAbsolute",
    "join",
    "relative",
    "toNamespacedPath",
    "dirname",
    "basename",
    "extname",
    "format",
    "parse",
    "sep",
    "delimiter",
    "posix",
    "win32",
];

pub(crate) struct NativeExports(HashMap<Builtin, Vec<v8::Global<v8::Value>>>);

impl NativeExports {
    /// Called only during fresh-context setup, before any user JS executes.
    pub fn capture(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> Option<Self> {
        let global = context.global(scope);
        let rusty_key = v8::String::new(scope, "Rusty")?;
        let rusty = v8::Local::<v8::Object>::try_from(global.get(scope, rusty_key.into())?).ok()?;
        let mut functions = Vec::with_capacity(TIMER_NAMES.len());
        for name in TIMER_NAMES {
            let object = if name == "sleep" { rusty } else { global };
            let key = v8::String::new(scope, name)?;
            let function =
                v8::Local::<v8::Function>::try_from(object.get(scope, key.into())?).ok()?;
            let function: v8::Local<v8::Value> = function.into();
            functions.push(v8::Global::new(scope, function));
        }
        let create_require = v8::Function::new(scope, crate::commonjs::create_require)?;
        let create_require: v8::Local<v8::Value> = create_require.into();
        let events = crate::events::create(scope)?;
        let buffer = crate::buffer::create(scope)?;
        let streams = crate::streams::create(scope, events, buffer)?;
        let key = v8::String::new(scope, "Buffer")?;
        if global.create_data_property(scope, key.into(), buffer) != Some(true) {
            return None;
        }
        let mut exports = HashMap::from([
            (Builtin::Buffer, vec![v8::Global::new(scope, buffer)]),
            (
                Builtin::Stream,
                streams
                    .stream
                    .into_iter()
                    .map(|value| v8::Global::new(scope, value))
                    .collect(),
            ),
            (
                Builtin::StreamPromises,
                streams
                    .promises
                    .into_iter()
                    .map(|value| v8::Global::new(scope, value))
                    .collect(),
            ),
            (Builtin::Timers, functions),
            (
                Builtin::Module,
                vec![v8::Global::new(scope, create_require)],
            ),
            (
                Builtin::Events,
                vec![
                    v8::Global::new(scope, events),
                    v8::Global::new(scope, events),
                ],
            ),
        ]);
        let path = crate::paths::create(scope)?;
        for (builtin, property) in [
            (Builtin::Path, None),
            (Builtin::PathPosix, Some("posix")),
            (Builtin::PathWin32, Some("win32")),
        ] {
            let object = if let Some(property) = property {
                let key = v8::String::new(scope, property)?;
                v8::Local::<v8::Object>::try_from(path.get(scope, key.into())?).ok()?
            } else {
                path
            };
            let mut values = vec![v8::Global::new(scope, v8::Local::<v8::Value>::from(object))];
            for name in &PATH_NAMES[1..] {
                let key = v8::String::new(scope, name)?;
                let value = object.get(scope, key.into())?;
                values.push(v8::Global::new(scope, value));
            }
            exports.insert(builtin, values);
        }
        Some(Self(exports))
    }
}

pub(crate) fn commonjs_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    builtin: Builtin,
) -> Option<v8::Local<'s, v8::Object>> {
    let modules = scope.get_slot::<SharedModules>()?.clone();
    if builtin == Builtin::StreamPromises {
        // CommonJS stream/promises is the same object exported as stream.promises.
        let object = {
            let modules = modules.borrow();
            v8::Local::new(
                scope,
                modules
                    .native_exports
                    .as_ref()?
                    .0
                    .get(&Builtin::Stream)?
                    .get(4)?,
            )
        };
        return v8::Local::<v8::Object>::try_from(object).ok();
    }
    let functions = {
        let modules = modules.borrow();
        modules
            .native_exports
            .as_ref()?
            .0
            .get(&builtin)?
            .iter()
            .map(|function| v8::Local::new(scope, function))
            .collect::<Vec<_>>()
    };
    if matches!(
        builtin,
        Builtin::Events | Builtin::Path | Builtin::PathPosix | Builtin::PathWin32
    ) {
        return v8::Local::<v8::Object>::try_from(functions[0]).ok();
    }
    let object = v8::Object::new(scope);
    for (name, function) in builtin.export_names().iter().zip(functions) {
        let key = v8::String::new(scope, name)?;
        if object.create_data_property(scope, key.into(), function) != Some(true) {
            return None;
        }
    }
    Some(object)
}

pub(crate) fn create<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    builtin: Builtin,
) -> Option<v8::Local<'s, v8::Module>> {
    let name = v8::String::new(scope, builtin.specifier())?;
    let exports = builtin
        .export_names()
        .iter()
        .map(|name| v8::String::new(scope, name))
        .collect::<Option<Vec<_>>>()?;
    Some(v8::Module::create_synthetic_module(
        scope, name, &exports, evaluate,
    ))
}

fn evaluate<'s>(
    context: v8::Local<'s, v8::Context>,
    module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
    // SAFETY: V8 invokes these evaluation steps in the live context on its owner thread.
    v8::callback_scope!(unsafe scope, context);
    let modules = scope.get_slot::<SharedModules>()?.clone();
    let builtin = modules.borrow().builtin_for_module(module)?;
    let functions = {
        let modules = modules.borrow();
        modules
            .native_exports
            .as_ref()?
            .0
            .get(&builtin)?
            .iter()
            .map(|function| v8::Local::new(scope, function))
            .collect::<Vec<_>>()
    };
    for (name, function) in builtin.export_names().iter().zip(functions) {
        let name = v8::String::new(scope, name)?;
        if module.set_synthetic_module_export(scope, name, function) != Some(true) {
            return None;
        }
    }
    let resolver = v8::PromiseResolver::new(scope)?;
    let value = v8::undefined(scope);
    resolver.resolve(scope, value.into())?;
    Some(resolver.get_promise(scope).into())
}
