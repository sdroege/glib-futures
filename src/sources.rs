use futures_channel::oneshot;
use futures_core::task::Context;
use futures_core::{Async, Future, Never};

use glib;

pub struct SourceFuture<F, T> {
    create_source: Option<F>,
    source: Option<(glib::Source, oneshot::Receiver<T>)>,
}

impl<F, T> SourceFuture<F, T>
where
    F: FnOnce(oneshot::Sender<T>) -> glib::Source + Send + 'static,
{
    pub fn new(create_source: F) -> impl Future<Item = T, Error = Never> {
        SourceFuture {
            create_source: Some(create_source),
            source: None,
        }
    }
}

impl<F, T> Future for SourceFuture<F, T>
where
    F: FnOnce(oneshot::Sender<T>) -> glib::Source + Send + 'static,
{
    type Item = T;
    type Error = Never;

    fn poll(&mut self, ctx: &mut Context) -> Result<Async<T>, Never> {
        let SourceFuture {
            ref mut create_source,
            ref mut source,
            ..
        } = *self;

        if let Some(create_source) = create_source.take() {
            let main_context = glib::MainContext::ref_thread_default();
            match main_context {
                None => unreachable!(),
                Some(ref main_context) => {
                    assert!(main_context.is_owner());

                    let (send, recv) = oneshot::channel();

                    let s = create_source(send);

                    s.attach(Some(main_context));
                    *source = Some((s, recv));
                }
            }
        }

        // At this point we must have a receiver
        let res = {
            let (_, receiver) = source.as_mut().unwrap();
            receiver.poll(ctx)
        };
        match res {
            Err(_) => unreachable!(),
            Ok(Async::Ready(v)) => {
                // Get rid of the reference to the timeout source, it triggered
                let _ = source.take();
                Ok(Async::Ready(v))
            }
            Ok(Async::Pending) => Ok(Async::Pending),
        }
    }
}

impl<T, F> Drop for SourceFuture<T, F> {
    fn drop(&mut self) {
        // Get rid of the source, we don't care anymore if it still triggers
        if let Some((source, _)) = self.source.take() {
            source.destroy();
        }
    }
}

pub fn timeout(value: u32) -> impl Future<Item = (), Error = Never> {
    SourceFuture::new(move |send| {
        let mut send = Some(send);
        glib::timeout_source_new(value, None, glib::PRIORITY_DEFAULT, move || {
            let _ = send.take().unwrap().send(());
            glib::Continue(false)
        })
    })
}

pub fn timeout_seconds(value: u32) -> impl Future<Item = (), Error = Never> {
    SourceFuture::new(move |send| {
        let mut send = Some(send);
        glib::timeout_source_new_seconds(value, None, glib::PRIORITY_DEFAULT, move || {
            let _ = send.take().unwrap().send(());
            glib::Continue(false)
        })
    })
}

pub fn child_watch(pid: glib::Pid) -> impl Future<Item = (glib::Pid, i32), Error = Never> {
    SourceFuture::new(move |send| {
        let mut send = Some(send);
        glib::child_watch_source_new(pid, None, glib::PRIORITY_DEFAULT, move |pid, code| {
            let _ = send.take().unwrap().send((pid, code));
        })
    })
}

#[cfg(any(unix, feature = "dox"))]
pub fn unix_signal(signum: i32) -> impl Future<Item = (), Error = Never> {
    SourceFuture::new(move |send| {
        let mut send = Some(send);
        glib::unix_signal_source_new(signum, None, glib::PRIORITY_DEFAULT, move || {
            let _ = send.take().unwrap().send(());
            glib::Continue(false)
        })
    })
}
