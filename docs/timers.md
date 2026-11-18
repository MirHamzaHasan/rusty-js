# Callback timers

`setTimeout`, `clearTimeout`, `setInterval`, and `clearInterval` are implemented
as globals. This is a small explicit API, not full browser or Node compatibility.
See [supported API](api.md) for the signatures and input rules.

```sh
cargo run --locked -- examples/timers.js
```

## Walkthrough

1. `src/timers.rs` installs the four native functions in the existing context.
   Registration requires a function callback and checks a primitive Number delay
   without executing user-defined conversion hooks.
2. The isolate-local `HostState` allocates a monotonically increasing ID. IDs stay
   within JavaScript's safe integer range and are shared with internal operation
   IDs, so a stale identifier cannot be recycled into new work.
3. The registry retains `Global<Function>` and argument roots, the script origin,
   a Tokio deadline, and an optional interval period. No V8 handle enters a Rust
   worker task. Registration records a deadline; it does not block JS.
4. `run_event_loop` finds the earliest live timer using `(deadline, ID)` order.
   The initial implementation scans the registry in O(n); a scheduling heap can
   replace this later if profiling justifies it.
5. With only timers pending, the loop awaits Tokio's earliest deadline. With host
   Promise tasks also pending, `timeout_at` races that deadline against a
   cancellation-safe `JoinSet::join_next_with_id`. A timer winning this race does
   not discard a host task result.
6. Before calling JS, the host enters a fresh V8 scope and creates local callback
   and argument handles. It ends every registry borrow before invoking the
   callback, allowing callbacks to register or clear timers safely.
7. A timeout is removed from the registry before invocation. An interval stays
   registered during its callback so it can clear itself. Arguments retain their
   original JS identity; the explicit receiver is `globalThis`.
8. The callback's Promise reactions run at the following microtask checkpoint.
   An interval is rearmed only if it still exists after that checkpoint. Clearing
   it from a Promise reaction therefore prevents the next tick too.
9. The loop yields to Tokio after a callback and lets a ready host completion
   run before selecting another due timer. A recursive zero-delay timeout chain
   cannot indefinitely prevent an already-ready `Rusty.sleep` from settling.
10. The loop exits when no callback timers or host operations remain. An active
    interval keeps it alive until cleared or cancelled by the host.

## Interval policy

Intervals use fixed delay: the next deadline is a full period after the callback
and its microtask checkpoint finish. Synchronous invocations do not overlap, and
there is no fixed-rate catch-up or burst of callbacks for ticks missed while JS
was busy. Returned Promises are not awaited before rearming: work started by async
interval callbacks can overlap. Their registered host operations still keep the
event loop alive after the interval itself is cleared.

An interval's effective period is at least 1 ms, including an omitted or zero
delay. Timeouts may use zero delay and still execute asynchronously. Neither kind
of timer guarantees precise wall-clock execution or interrupts running JS.

This policy favors a simple predictable lifecycle over matching another runtime's
timer internals. It is part of the RustyJS API contract and must stay documented
if it changes.

## Clearing, errors, and teardown

Both clear functions remove either kind of callback timer. Unknown, already-fired,
or invalid IDs are no-ops; they do not trigger JS number conversion. Clearing a
timer drops its registry roots immediately. It also removes the timer from the
next-deadline calculation, so a cleared long timer does not delay idle exit.

The host selects callbacks from the live registry one at a time. If one callback
clears another timer whose deadline has already elapsed, that other callback is
not invoked. Clearing a currently running callback cannot undo its side effects.

A synchronous callback exception becomes a `JsError`. A rejected async callback
is detected by the existing checkpoint rejection policy. Either failure stops the
run and clears the remaining timers and host operations. As with the Promise
bridge, the context remains reusable and existing guest side effects remain.

`cancel_pending`, dropping a running event-loop future, and runtime teardown all
release callback and argument roots. Callback timers do not have detached tasks
that could later re-enter a disposed isolate.

## Verification

The CLI tests cover argument identity, receiver, callback/microtask order, default
delays, strict validation, clearing queued and already-due timers, cross-clearing,
self-clearing intervals, clearing from a microtask, and synchronous/async failure.
Child processes have a test timeout, so a broken interval cannot hang CI forever.

The library tests use a paused Tokio clock to advance past an interval deadline,
verify one invocation rather than replayed ticks, cancel the loop, and check the
timer registry is empty. Other tests check timer callback definition-file
diagnostics and progress for a host Promise alongside recursive zero-delay timers.
Another paused-clock test verifies that returned async callback Promises are not
awaited before rearming, while their pending host work still drains before exit.

This does not establish a scheduling latency bound or a general fairness budget.
Synchronous JS and endless Promise microtasks can still monopolize the isolate
thread. Resource limits and interruptible sandbox execution remain future work.
