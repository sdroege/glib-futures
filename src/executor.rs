// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use futures_core;
use futures_core::executor::{Executor, SpawnError};
use futures_core::task::{LocalMap, UnsafeWake, Waker};
use futures_core::{Async, Future, Never};
use futures_executor;
use futures_util::future::FutureExt;

use glib;
use glib::translate::{from_glib_none, mut_override, ToGlibPtr};
use glib_ffi;

const INIT: usize = 0;
const NOT_READY: usize = 1;
const READY: usize = 2;
const DONE: usize = 3;

#[repr(C)]
struct TaskSource {
    source: glib_ffi::GSource,
    future: Option<(Box<Future<Item = (), Error = Never>>, Box<LocalMap>)>,
    thread: Option<thread::ThreadId>,
    state: AtomicUsize,
}

unsafe impl UnsafeWake for TaskSource {
    unsafe fn clone_raw(&self) -> Waker {
        Waker::new(glib_ffi::g_source_ref(mut_override(&self.source)) as *const TaskSource)
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
    let source = &mut *(source as *mut TaskSource);

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
    let source = &mut *(source as *mut TaskSource);

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
    let source = &mut *(source as *mut TaskSource);
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
    let source = source as *mut TaskSource;
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

impl TaskSource {
    fn new(
        future: Box<Future<Item = (), Error = Never> + 'static + Send>,
    ) -> *mut glib_ffi::GSource {
        unsafe { Self::new_unsafe(future) }
    }

    // NOTE: This does not have the Send bound and requires to be called from the same
    // thread where the main context is running
    unsafe fn new_unsafe(
        future: Box<Future<Item = (), Error = Never> + 'static>,
    ) -> *mut glib_ffi::GSource {
        let source = glib_ffi::g_source_new(
            mut_override(&SOURCE_FUNCS),
            mem::size_of::<TaskSource>() as u32,
        );
        {
            let source = &mut *(source as *mut TaskSource);
            source.future = Some((future, Box::new(LocalMap::new())));
            source.thread = None;
            source.state = AtomicUsize::new(INIT);
        }
        source
    }

    fn poll(&mut self) -> Async<()> {
        // Make sure that the first time we're polled that the current thread is remembered
        // and from there one we ensure that we're always polled from exactly the same thread.
        //
        // In theory a GMainContext can be first run from one thread and later from another
        // thread, but we allow spawning non-Send futures and must not ever use them from
        // any other thread.
        match &mut self.thread {
            thread @ &mut None => {
                *thread = Some(thread::current().id());
            }
            &mut Some(thread_id) => {
                assert_eq!(thread::current().id(), thread_id);
            }
        }

        let waker = unsafe { self.clone_raw() };
        let source = &self.source as *const _;
        if let Some(ref mut future) = self.future {
            let (ref mut future, ref mut local_map) = *future;

            let mut executor = unsafe {
                let ctx: glib::MainContext =
                    from_glib_none(glib_ffi::g_source_get_context(mut_override(source)));
                MainContext(ctx)
            };

            // Clone that we store in the task local data so that
            // it can be retrieved as needed
            executor.0.push_thread_default();

            let res = {
                let enter = futures_executor::enter().unwrap();
                let mut context =
                    futures_core::task::Context::new(local_map, &waker, &mut executor);

                let res = future.poll(&mut context).unwrap_or(Async::Ready(()));

                drop(enter);

                res
            };

            executor.0.pop_thread_default();
            res
        } else {
            Async::Ready(())
        }
    }
}

pub struct MainContext(glib::MainContext);

impl MainContext {
    pub fn new() -> Self {
        glib::MainContext::new().into()
    }

    pub fn default() -> Option<Self> {
        glib::MainContext::default().map(MainContext)
    }

    pub fn thread_default() -> Option<Self> {
        glib::MainContext::ref_thread_default().map(MainContext)
    }

    pub fn spawn<F: Future<Item = (), Error = Never> + Send + 'static>(&mut self, f: F) {
        <Self as Executor>::spawn(self, Box::new(f)).unwrap()
    }

    pub fn spawn_local<F: Future<Item = (), Error = Never> + 'static>(&mut self, f: F) {
        assert!(self.0.is_owner());
        unsafe {
            let source = TaskSource::new_unsafe(Box::new(f));

            // Ensure that this task is never polled on another thread
            // than this one where it was spawned now.
            {
                let source = &mut *(source as *mut TaskSource);
                source.thread = Some(thread::current().id());
            }

            glib_ffi::g_source_attach(source, self.0.to_glib_none().0);
        }
    }

    pub fn block_on<F: Future>(&mut self, f: F) -> Result<F::Item, F::Error> {
        let mut res = None;
        let l = glib::MainLoop::new(Some(&self.0), false);
        let l_clone = l.clone();

        unsafe {
            let future = f.then(|r| {
                res = Some(r);
                l_clone.quit();
                Ok::<(), Never>(())
            });

            let future: *mut Future<Item = (), Error = Never> = Box::into_raw(Box::new(future));
            // XXX: Transmute to get a 'static lifetime here, super unsafe
            let future: *mut (Future<Item = (), Error = Never> + 'static) = mem::transmute(future);
            let future: Box<Future<Item = (), Error = Never> + 'static> = Box::from_raw(future);

            let source = TaskSource::new_unsafe(future);

            // Ensure that this task is never polled on another thread
            // than this one where it was spawned now.
            {
                let source = &mut *(source as *mut TaskSource);
                source.thread = Some(thread::current().id());
            }

            glib_ffi::g_source_attach(source, self.0.to_glib_none().0);
        }

        l.run();

        res.unwrap()
    }
}

impl Executor for MainContext {
    fn spawn(&mut self, f: Box<Future<Item = (), Error = Never> + Send>) -> Result<(), SpawnError> {
        let source = TaskSource::new(f);
        unsafe {
            glib_ffi::g_source_attach(source, self.0.to_glib_none().0);
        }
        Ok(())
    }
}

impl ::std::ops::Deref for MainContext {
    type Target = glib::MainContext;

    fn deref(&self) -> &glib::MainContext {
        &self.0
    }
}

impl From<glib::MainContext> for MainContext {
    fn from(ctx: glib::MainContext) -> MainContext {
        MainContext(ctx)
    }
}
