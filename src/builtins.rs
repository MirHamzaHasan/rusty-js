//! Native ESM exports captured before guest code can replace global properties.

use crate::modules::SharedModules;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Builtin {
    Timers,
}

impl Builtin {
    pub fn resolve(specifier: &str) -> Option<Self> {
        match specifier {
            "rustyjs:timers" => Some(Self::Timers),
            _ => None,
        }
    }

    pub fn specifier(self) -> &'static str {
        match self {
            Self::Timers => "rustyjs:timers",
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

pub(crate) struct TimerExports(Vec<v8::Global<v8::Function>>);

impl TimerExports {
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
            functions.push(v8::Global::new(scope, function));
        }
        Some(Self(functions))
    }
}

pub(crate) fn commonjs_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::Object>> {
    let modules = scope.get_slot::<SharedModules>()?.clone();
    let functions = {
        let modules = modules.borrow();
        modules
            .timer_exports
            .as_ref()?
            .0
            .iter()
            .map(|function| v8::Local::new(scope, function))
            .collect::<Vec<_>>()
    };
    let object = v8::Object::new(scope);
    for (name, function) in TIMER_NAMES.into_iter().zip(functions) {
        let key = v8::String::new(scope, name)?;
        if object.create_data_property(scope, key.into(), function.into()) != Some(true) {
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
    let exports = TIMER_NAMES
        .iter()
        .map(|name| v8::String::new(scope, name))
        .collect::<Option<Vec<_>>>()?;
    Some(v8::Module::create_synthetic_module(
        scope,
        name,
        &exports,
        evaluate_timers,
    ))
}

fn evaluate_timers<'s>(
    context: v8::Local<'s, v8::Context>,
    module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
    // SAFETY: V8 invokes these evaluation steps in the live context on its owner thread.
    v8::callback_scope!(unsafe scope, context);
    let modules = scope.get_slot::<SharedModules>()?.clone();
    let functions = {
        let modules = modules.borrow();
        modules
            .timer_exports
            .as_ref()?
            .0
            .iter()
            .map(|function| v8::Local::new(scope, function))
            .collect::<Vec<_>>()
    };
    for (name, function) in TIMER_NAMES.into_iter().zip(functions) {
        let name = v8::String::new(scope, name)?;
        if module.set_synthetic_module_export(scope, name, function.into()) != Some(true) {
            return None;
        }
    }
    let resolver = v8::PromiseResolver::new(scope)?;
    let value = v8::undefined(scope);
    resolver.resolve(scope, value.into())?;
    Some(resolver.get_promise(scope).into())
}
