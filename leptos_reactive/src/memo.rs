#![forbid(unsafe_code)]
use crate::{
    create_effect, diagnostics::AccessDiagnostics, node::NodeId, on_cleanup,
    with_runtime, AnyComputation, RuntimeId, Scope, SignalDispose, SignalGet,
    SignalGetUntracked, SignalStream, SignalWith, SignalWithUntracked,
};
use cfg_if::cfg_if;
use std::{any::Any, cell::RefCell, fmt::Debug, marker::PhantomData, rc::Rc};

/// Creates an efficient derived reactive value based on other reactive values.
///
/// Unlike a "derived signal," a memo comes with two guarantees:
/// 1. The memo will only run *once* per change, no matter how many times you
/// access its value.
/// 2. The memo will only notify its dependents if the value of the computation changes.
///
/// This makes a memo the perfect tool for expensive computations.
///
/// Memos have a certain overhead compared to derived signals. In most cases, you should
/// create a derived signal. But if the derivation calculation is expensive, you should
/// create a memo.
///
/// As with [create_effect](crate::create_effect), the argument to the memo function is the previous value,
/// i.e., the current value of the memo, which will be `None` for the initial calculation.
///
/// ```
/// # use leptos_reactive::*;
/// # fn really_expensive_computation(value: i32) -> i32 { value };
/// # create_scope(create_runtime(), |cx| {
/// let (value, set_value) = create_signal(cx, 0);
///
/// // 🆗 we could create a derived signal with a simple function
/// let double_value = move || value() * 2;
/// set_value(2);
/// assert_eq!(double_value(), 4);
///
/// // but imagine the computation is really expensive
/// let expensive = move || really_expensive_computation(value()); // lazy: doesn't run until called
/// create_effect(cx, move |_| {
///   // 🆗 run #1: calls `really_expensive_computation` the first time
///   log::debug!("expensive = {}", expensive());
/// });
/// create_effect(cx, move |_| {
///   // ❌ run #2: this calls `really_expensive_computation` a second time!
///   let value = expensive();
///   // do something else...
/// });
///
/// // instead, we create a memo
/// // 🆗 run #1: the calculation runs once immediately
/// let memoized = create_memo(cx, move |_| really_expensive_computation(value()));
/// create_effect(cx, move |_| {
///  // 🆗 reads the current value of the memo
///   log::debug!("memoized = {}", memoized());
/// });
/// create_effect(cx, move |_| {
///   // ✅ reads the current value **without re-running the calculation**
///   let value = memoized();
///   // do something else...
/// });
/// # }).dispose();
/// ```
#[cfg_attr(
    debug_assertions,
    instrument(
        level = "trace",
        skip_all,
        fields(
            scope = ?cx.id,
            ty = %std::any::type_name::<T>()
        )
    )
)]
#[track_caller]
pub fn create_memo<T>(
    cx: Scope,
    f: impl Fn(Option<&T>) -> T + 'static,
) -> Memo<T>
where
    T: PartialEq + 'static,
{
    cx.runtime.create_memo(f)
}

/// An efficient derived reactive value based on other reactive values.
///
/// Unlike a "derived signal," a memo comes with two guarantees:
/// 1. The memo will only run *once* per change, no matter how many times you
/// access its value.
/// 2. The memo will only notify its dependents if the value of the computation changes.
///
/// This makes a memo the perfect tool for expensive computations.
///
/// Memos have a certain overhead compared to derived signals. In most cases, you should
/// create a derived signal. But if the derivation calculation is expensive, you should
/// create a memo.
///
/// As with [create_effect](crate::create_effect), the argument to the memo function is the previous value,
/// i.e., the current value of the memo, which will be `None` for the initial calculation.
///
/// ## Core Trait Implementations
/// - [`.get()`](#impl-SignalGet<T>-for-Memo<T>) (or calling the signal as a function) clones the current
///   value of the signal. If you call it within an effect, it will cause that effect
///   to subscribe to the signal, and to re-run whenever the value of the signal changes.
///   - [`.get_untracked()`](#impl-SignalGetUntracked<T>-for-Memo<T>) clones the value of the signal
///   without reactively tracking it.
/// - [`.with()`](#impl-SignalWith<T>-for-Memo<T>) allows you to reactively access the signal’s value without
///   cloning by applying a callback function.
///   - [`.with_untracked()`](#impl-SignalWithUntracked<T>-for-Memo<T>) allows you to access the signal’s
///   value without reactively tracking it.
/// - [`.to_stream()`](#impl-SignalStream<T>-for-Memo<T>) converts the signal to an `async` stream of values.
///
/// ## Examples
/// ```
/// # use leptos_reactive::*;
/// # fn really_expensive_computation(value: i32) -> i32 { value };
/// # create_scope(create_runtime(), |cx| {
/// let (value, set_value) = create_signal(cx, 0);
///
/// // 🆗 we could create a derived signal with a simple function
/// let double_value = move || value() * 2;
/// set_value(2);
/// assert_eq!(double_value(), 4);
///
/// // but imagine the computation is really expensive
/// let expensive = move || really_expensive_computation(value()); // lazy: doesn't run until called
/// create_effect(cx, move |_| {
///   // 🆗 run #1: calls `really_expensive_computation` the first time
///   log::debug!("expensive = {}", expensive());
/// });
/// create_effect(cx, move |_| {
///   // ❌ run #2: this calls `really_expensive_computation` a second time!
///   let value = expensive();
///   // do something else...
/// });
///
/// // instead, we create a memo
/// // 🆗 run #1: the calculation runs once immediately
/// let memoized = create_memo(cx, move |_| really_expensive_computation(value()));
/// create_effect(cx, move |_| {
///  // 🆗 reads the current value of the memo
///   log::debug!("memoized = {}", memoized());
/// });
/// create_effect(cx, move |_| {
///   // ✅ reads the current value **without re-running the calculation**
///   let value = memoized();
///   // do something else...
/// });
/// # }).dispose();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct Memo<T>
where
    T: 'static,
{
    pub(crate) runtime: RuntimeId,
    pub(crate) id: NodeId,
    pub(crate) ty: PhantomData<T>,
    #[cfg(debug_assertions)]
    pub(crate) defined_at: &'static std::panic::Location<'static>,
}

impl<T> Clone for Memo<T>
where
    T: 'static,
{
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime,
            id: self.id,
            ty: PhantomData,
            #[cfg(debug_assertions)]
            defined_at: self.defined_at,
        }
    }
}

impl<T> Copy for Memo<T> {}

impl<T: Clone> SignalGetUntracked<T> for Memo<T> {
    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::get_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn get_untracked(&self) -> T {
        with_runtime(self.runtime, move |runtime| {
            let f = move |maybe_value: &Option<T>| maybe_value.clone().unwrap();
            match self.id.try_with_no_subscription(runtime, f) {
                Ok(t) => t,
                Err(_) => panic_getting_dead_memo(
                    #[cfg(debug_assertions)]
                    self.defined_at,
                ),
            }
        })
        .expect("runtime to be alive")
    }

    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::try_get_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn try_get_untracked(&self) -> Option<T> {
        with_runtime(self.runtime, move |runtime| {
            self.id.try_with_no_subscription(runtime, T::clone).ok()
        })
        .ok()
        .flatten()
    }
}

impl<T> SignalWithUntracked<T> for Memo<T> {
    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::with_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn with_untracked<O>(&self, f: impl FnOnce(&T) -> O) -> O {
        // Unwrapping here is fine for the same reasons as <Memo as
        // UntrackedSignal>::get_untracked
        with_runtime(self.runtime, |runtime| {
            match self.id.try_with_no_subscription(runtime, |v: &T| f(v)) {
                Ok(t) => t,
                Err(_) => panic_getting_dead_memo(
                    #[cfg(debug_assertions)]
                    self.defined_at,
                ),
            }
        })
        .expect("runtime to be alive")
    }

    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::try_with_untracked()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn try_with_untracked<O>(&self, f: impl FnOnce(&T) -> O) -> Option<O> {
        with_runtime(self.runtime, |runtime| {
            self.id.try_with_no_subscription(runtime, |v: &T| f(v)).ok()
        })
        .ok()
        .flatten()
    }
}

/// # Examples
///
/// ```
/// # use leptos_reactive::*;
/// # create_scope(create_runtime(), |cx| {
/// let (count, set_count) = create_signal(cx, 0);
/// let double_count = create_memo(cx, move |_| count() * 2);
///
/// assert_eq!(double_count.get(), 0);
/// set_count(1);
///
/// // double_count() is shorthand for double_count.get()
/// assert_eq!(double_count(), 2);
/// # }).dispose();
/// #
/// ```
impl<T: Clone> SignalGet<T> for Memo<T> {
    #[cfg_attr(
        debug_assertions,
        instrument(
            name = "Memo::get()",
            level = "trace",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    fn get(&self) -> T {
        self.with(T::clone)
    }

    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::try_get()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    fn try_get(&self) -> Option<T> {
        self.try_with(T::clone)
    }
}

impl<T> SignalWith<T> for Memo<T> {
    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::with()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    fn with<O>(&self, f: impl FnOnce(&T) -> O) -> O {
        match self.try_with(f) {
            Some(t) => t,
            None => panic_getting_dead_memo(
                #[cfg(debug_assertions)]
                self.defined_at,
            ),
        }
    }

    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::try_with()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    #[track_caller]
    fn try_with<O>(&self, f: impl FnOnce(&T) -> O) -> Option<O> {
        // memo is stored as Option<T>, but will always have T available
        // after latest_value() called, so we can unwrap safely
        let f = move |maybe_value: &Option<T>| f(maybe_value.as_ref().unwrap());

        let diagnostics = diagnostics!(self);

        with_runtime(self.runtime, |runtime| {
            self.id.subscribe(runtime, diagnostics);
            self.id.try_with_no_subscription(runtime, f).ok()
        })
        .ok()
        .flatten()
    }
}

impl<T: Clone> SignalStream<T> for Memo<T> {
    #[cfg_attr(
        debug_assertions,
        instrument(
            level = "trace",
            name = "Memo::to_stream()",
            skip_all,
            fields(
                id = ?self.id,
                defined_at = %self.defined_at,
                ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn to_stream(
        &self,
        cx: Scope,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = T>>> {
        let (tx, rx) = futures::channel::mpsc::unbounded();

        let close_channel = tx.clone();

        on_cleanup(cx, move || close_channel.close_channel());

        let this = *self;

        create_effect(cx, move |_| {
            let _ = tx.unbounded_send(this.get());
        });

        Box::pin(rx)
    }
}

impl<T> SignalDispose for Memo<T> {
    fn dispose(self) {
        _ = with_runtime(self.runtime, |runtime| runtime.dispose_node(self.id));
    }
}

impl_get_fn_traits![Memo];

pub(crate) struct MemoState<T, F>
where
    T: PartialEq + 'static,
    F: Fn(Option<&T>) -> T,
{
    pub f: F,
    pub t: PhantomData<T>,
    #[cfg(debug_assertions)]
    pub(crate) defined_at: &'static std::panic::Location<'static>,
}

impl<T, F> AnyComputation for MemoState<T, F>
where
    T: PartialEq + 'static,
    F: Fn(Option<&T>) -> T,
{
    #[cfg_attr(
        debug_assertions,
        instrument(
            name = "Memo::run()",
            level = "debug",
            skip_all,
            fields(
              defined_at = %self.defined_at,
              ty = %std::any::type_name::<T>()
            )
        )
    )]
    fn run(&self, value: Rc<RefCell<dyn Any>>) -> bool {
        let (new_value, is_different) = {
            let value = value.borrow();
            let curr_value = value
                .downcast_ref::<Option<T>>()
                .expect("to downcast memo value");

            // run the effect
            let new_value = (self.f)(curr_value.as_ref());
            let is_different = curr_value.as_ref() != Some(&new_value);
            (new_value, is_different)
        };
        if is_different {
            let mut value = value.borrow_mut();
            let curr_value = value
                .downcast_mut::<Option<T>>()
                .expect("to downcast memo value");
            *curr_value = Some(new_value);
        }

        is_different
    }
}

#[track_caller]
fn format_memo_warning(
    msg: &str,
    #[cfg(debug_assertions)] defined_at: &'static std::panic::Location<'static>,
) -> String {
    let location = std::panic::Location::caller();

    let defined_at_msg = {
        #[cfg(debug_assertions)]
        {
            format!("signal created here: {defined_at}\n")
        }

        #[cfg(not(debug_assertions))]
        {
            String::default()
        }
    };

    format!("{msg}\n{defined_at_msg}warning happened here: {location}",)
}

#[track_caller]
pub(crate) fn panic_getting_dead_memo(
    #[cfg(debug_assertions)] defined_at: &'static std::panic::Location<'static>,
) -> ! {
    panic!(
        "{}",
        format_memo_warning(
            "Attempted to get a memo after it was disposed.",
            #[cfg(debug_assertions)]
            defined_at,
        )
    )
}
