//! Callback roots stay in V8's owner state; Tokio only waits for a Rust deadline.

use std::time::Duration;

use crate::async_ops::SharedState;

pub(crate) struct Timer {
    pub callback: v8::Global<v8::Function>,
    pub arguments: Vec<v8::Global<v8::Value>>,
    pub deadline: tokio::time::Instant,
    pub interval: Option<Duration>,
    pub origin: String,
}

pub(crate) fn install(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> Option<()> {
    let timeout = v8::Function::new(scope, set_timeout)?;
    let interval = v8::Function::new(scope, set_interval)?;
    let clear = v8::Function::new(scope, clear_timer)?;
    for (name, function) in [
        ("setTimeout", timeout),
        ("setInterval", interval),
        ("clearTimeout", clear),
        ("clearInterval", clear),
    ] {
        let key = v8::String::new(scope, name)?;
        if context
            .global(scope)
            .set(scope, key.into(), function.into())
            != Some(true)
        {
            return None;
        }
    }
    Some(())
}

fn set_timeout(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    result: v8::ReturnValue,
) {
    register(scope, args, result, false);
}

fn set_interval(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    result: v8::ReturnValue,
) {
    register(scope, args, result, true);
}

fn register(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
    repeat: bool,
) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        throw_type_error(scope, "timer callback must be a function");
        return;
    };
    let delay = args.get(1);
    let milliseconds = if delay.is_undefined() {
        Some(0.0)
    } else {
        v8::Local::<v8::Number>::try_from(delay)
            .ok()
            .map(|value| value.value())
    };
    let Some(milliseconds) = milliseconds
        .filter(|ms| ms.is_finite() && ms.fract() == 0.0 && (0.0..=2_147_483_647.0).contains(ms))
    else {
        throw_type_error(
            scope,
            "timer delay must be an integer from 0 to 2147483647 milliseconds",
        );
        return;
    };
    let delay = Duration::from_millis(if repeat {
        milliseconds.max(1.0)
    } else {
        milliseconds
    } as u64);
    let callback = v8::Global::new(scope, callback);
    let arguments = (2..args.length())
        .map(|index| v8::Global::new(scope, args.get(index)))
        .collect();
    let Some(state) = scope.get_slot::<SharedState>().cloned() else {
        return;
    };
    let mut state = state.borrow_mut();
    let Some(id) = state.allocate_id() else {
        drop(state);
        if let Some(message) = v8::String::new(scope, "host operation IDs exhausted") {
            let error = v8::Exception::range_error(scope, message);
            scope.throw_exception(error);
        }
        return;
    };
    let origin = state.origin.clone();
    state.timers.insert(
        id,
        Timer {
            callback,
            arguments,
            deadline: tokio::time::Instant::now() + delay,
            interval: repeat.then_some(delay),
            origin,
        },
    );
    drop(state);
    result.set(v8::Number::new(scope, id as f64).into());
}

fn clear_timer(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let Ok(id) = v8::Local::<v8::Number>::try_from(args.get(0)) else {
        return;
    };
    let id = id.value();
    if id.fract() != 0.0 || !(1.0..=9_007_199_254_740_991.0).contains(&id) {
        return;
    }
    if let Some(state) = scope.get_slot::<SharedState>() {
        state.borrow_mut().timers.remove(&(id as u64));
    }
}

fn throw_type_error(scope: &mut v8::PinScope, text: &str) {
    if let Some(message) = v8::String::new(scope, text) {
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
    }
}
