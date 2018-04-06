#[macro_use]
extern crate futures_core;
extern crate futures_executor;
extern crate futures_util;

extern crate gio;
extern crate glib;
extern crate glib_sys as glib_ffi;

use std::mem;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_core::executor::{Executor, SpawnError};
use futures_core::task::{Context, LocalMap, UnsafeWake, Waker};
use futures_core::{Async, Future, Never};
use futures_util::FutureExt;
use futures_util::future;

use glib::translate::{from_glib_none, mut_override, ToGlibPtr};

const INIT: usize = 0;
const NOT_READY: usize = 1;
const READY: usize = 2;
const DONE: usize = 3;

task_local! {
    static MAIN_CONTEXT: Option<glib::MainContext> = None
}

#[repr(C)]
struct FutureSource {
    source: glib_ffi::GSource,
    future: Option<(
        Box<Future<Item = (), Error = Never> + 'static + Send>,
        Box<LocalMap>,
    )>,
    state: AtomicUsize,
}

unsafe impl UnsafeWake for FutureSource {
    unsafe fn clone_raw(&self) -> Waker {
        Waker::new(glib_ffi::g_source_ref(mut_override(&self.source)) as *const FutureSource)
    }

    unsafe fn drop_raw(&self) {
        glib_ffi::g_source_unref(mut_override(&self.source));
    }

    unsafe fn wake(&self) {
        if self.state
            .compare_and_swap(NOT_READY, READY, Ordering::SeqCst) == READY
        {
            glib_ffi::g_source_set_ready_time(mut_override(&self.source), 0);
        }
    }
}

unsafe extern "C" fn prepare(
    source: *mut glib_ffi::GSource,
    timeout: *mut i32,
) -> glib_ffi::gboolean {
    let source = &mut *(source as *mut FutureSource);

    *timeout = -1;

    let mut cur = source
        .state
        .compare_and_swap(INIT, NOT_READY, Ordering::SeqCst);
    if cur == INIT {
        // XXX: This is not actually correct, we should not dispatch the
        // GSource here already but we need to know its current status so
        // that if it is not ready yet something can register to the waker
        if let Async::Ready(_) = source.poll() {
            source.state.store(DONE, Ordering::SeqCst);
            cur = DONE;
        } else {
            cur = NOT_READY;
        }
    }

    if cur == READY || cur == DONE {
        glib_ffi::GTRUE
    } else {
        glib_ffi::GFALSE
    }
}

unsafe extern "C" fn check(source: *mut glib_ffi::GSource) -> glib_ffi::gboolean {
    let source = &mut *(source as *mut FutureSource);

    let cur = source.state.load(Ordering::SeqCst);
    if cur == READY || cur == DONE {
        glib_ffi::GTRUE
    } else {
        glib_ffi::GFALSE
    }
}

unsafe extern "C" fn dispatch(
    source: *mut glib_ffi::GSource,
    callback: glib_ffi::GSourceFunc,
    _user_data: glib_ffi::gpointer,
) -> glib_ffi::gboolean {
    let source = &mut *(source as *mut FutureSource);
    assert!(callback.is_none());

    glib_ffi::g_source_set_ready_time(mut_override(&source.source), -1);
    let mut cur = source
        .state
        .compare_and_swap(READY, NOT_READY, Ordering::SeqCst);
    if cur == READY {
        if let Async::Ready(_) = source.poll() {
            source.state.store(DONE, Ordering::SeqCst);
            cur = DONE;
        } else {
            cur = NOT_READY;
        }
    }

    if cur == DONE {
        glib_ffi::G_SOURCE_REMOVE
    } else {
        glib_ffi::G_SOURCE_CONTINUE
    }
}

unsafe extern "C" fn finalize(source: *mut glib_ffi::GSource) {
    let source = source as *mut FutureSource;
    let _ = (*source).future.take();
}

static SOURCE_FUNCS: glib_ffi::GSourceFuncs = glib_ffi::GSourceFuncs {
    check: Some(check),
    prepare: Some(prepare),
    dispatch: Some(dispatch),
    finalize: Some(finalize),
    closure_callback: None,
    closure_marshal: None,
};

impl FutureSource {
    fn new<F: Future<Item = (), Error = Never> + 'static + Send>(
        future: F,
    ) -> *mut glib_ffi::GSource {
        let future: Box<Future<Item = (), Error = Never> + 'static + Send> = Box::new(future);
        unsafe {
            let source = glib_ffi::g_source_new(
                mut_override(&SOURCE_FUNCS),
                mem::size_of::<FutureSource>() as u32,
            );
            {
                let source = &mut *(source as *mut FutureSource);
                source.future = Some((future, Box::new(LocalMap::new())));
                source.state = AtomicUsize::new(INIT);
            }
            source
        }
    }

    fn poll(&mut self) -> Async<()> {
        let waker = unsafe { self.clone_raw() };
        let source = &self.source as *const _;
        if let Some(ref mut future) = self.future {
            let (ref mut future, ref mut local_map) = *future;

            let mut executor = unsafe {
                let ctx: glib::MainContext =
                    from_glib_none(glib_ffi::g_source_get_context(mut_override(source)));
                GMainContext(ctx)
            };

            let main_context = executor.0.clone();

            let enter = futures_executor::enter().unwrap();
            let mut context = futures_core::task::Context::new(local_map, &waker, &mut executor);
            *MAIN_CONTEXT.get_mut(&mut context) = Some(main_context);

            let res = future.poll(&mut context).unwrap_or(Async::Ready(()));

            drop(enter);
            res
        } else {
            Async::Ready(())
        }
    }
}

struct GMainContext(glib::MainContext);

impl Executor for GMainContext {
    fn spawn(&mut self, f: Box<Future<Item = (), Error = Never> + Send>) -> Result<(), SpawnError> {
        let source = FutureSource::new(f);
        unsafe {
            glib_ffi::g_source_attach(source, self.0.to_glib_none().0);
        }
        Ok(())
    }
}

const TIMEOUT_INIT: usize = 0;
const TIMEOUT_SCHEDULED: usize = 1;
const TIMEOUT_TRIGGERED: usize = 2;
struct TimeoutFuture {
    value: u32,
    state: Arc<AtomicUsize>,
}

impl TimeoutFuture {
    fn new(value: u32) -> TimeoutFuture {
        TimeoutFuture {
            value,
            state: Arc::new(AtomicUsize::new(TIMEOUT_INIT)),
        }
    }
}

impl Future for TimeoutFuture {
    type Item = ();
    type Error = Never;

    fn poll(&mut self, ctx: &mut Context) -> Result<Async<()>, Never> {
        let mut cur =
            self.state
                .compare_and_swap(TIMEOUT_SCHEDULED, TIMEOUT_INIT, Ordering::SeqCst);
        if cur == TIMEOUT_INIT {
            let waker = ctx.waker().clone();
            let main_context = MAIN_CONTEXT.get_mut(ctx);
            match *main_context {
                None => unreachable!(),
                Some(ref main_context) => {
                    let state = self.state.clone();
                    let t = glib::timeout_source_new(
                        self.value,
                        None,
                        glib::PRIORITY_DEFAULT,
                        move || {
                            state.store(TIMEOUT_TRIGGERED, Ordering::SeqCst);
                            waker.wake();
                            glib::Continue(false)
                        },
                    );
                    t.attach(Some(main_context));
                }
            }
            cur = TIMEOUT_SCHEDULED;
        }

        if cur == TIMEOUT_SCHEDULED {
            Ok(Async::Pending)
        } else {
            Ok(Async::Ready(()))
        }
    }
}

extern crate futures_channel;
use futures_channel::oneshot;

fn main() {
    let mut c = GMainContext(glib::MainContext::default().unwrap());
    let l = glib::MainLoop::new(Some(&c.0), false);

    let (sender, receiver) = oneshot::channel::<()>();

    let l_clone = l.clone();
    c.spawn(Box::new(future::lazy(move |ctx| {
        println!("meh");

        let l = l_clone.clone();
        ctx.spawn(receiver.then(move |_| {
            println!("meh2");
            l.quit();
            Ok(())
        }));

        Ok(())
    }))).unwrap();

    let mut sender = Some(sender);
    let t = TimeoutFuture::new(2000).and_then(move |_| {
        println!("meh3");
        // Get rid of sender to let the receiver trigger
        let _ = sender.take();

        Ok(())
    });
    c.spawn(Box::new(t)).unwrap();

    l.run();
}
