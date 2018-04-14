// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

use futures_channel::oneshot;
use futures_core::task::Context;
use futures_core::{Async, Future};

use gio_ as gio;
use glib;

pub struct GioFuture<F, O, T, E> {
    obj: O,
    schedule_operation: Option<F>,
    cancellable: Option<(gio::Cancellable, oneshot::Receiver<Result<T, E>>)>,
}

impl<F, O, T, E> GioFuture<F, O, T, E>
where
    O: glib::IsA<glib::Object> + Clone + 'static,
    F: FnOnce(&O, oneshot::Sender<Result<T, E>>) -> gio::Cancellable + 'static,
{
    pub fn new(obj: &O, schedule_operation: F) -> impl Future<Item = (O, T), Error = (O, E)> {
        GioFuture {
            obj: obj.clone(),
            schedule_operation: Some(schedule_operation),
            cancellable: None,
        }
    }
}

impl<F, O, T, E> Future for GioFuture<F, O, T, E>
where
    O: glib::IsA<glib::Object> + Clone + 'static,
    F: FnOnce(&O, oneshot::Sender<Result<T, E>>) -> gio::Cancellable + 'static,
{
    type Item = (O, T);
    type Error = (O, E);

    fn poll(&mut self, ctx: &mut Context) -> Result<Async<(O, T)>, (O, E)> {
        let GioFuture {
            ref obj,
            ref mut schedule_operation,
            ref mut cancellable,
            ..
        } = *self;

        if let Some(schedule_operation) = schedule_operation.take() {
            let main_context = glib::MainContext::ref_thread_default();
            match main_context {
                None => unreachable!(),
                Some(ref main_context) => {
                    assert!(main_context.is_owner());

                    // Channel for sending back the GIO async operation
                    // result to our future here.
                    //
                    // In theory we could directly continue polling the
                    // corresponding task from the GIO async operation
                    // callback, however this would break at the very
                    // least the g_main_current_source() API.
                    let (send, recv) = oneshot::channel();

                    let c = schedule_operation(obj, send);

                    *cancellable = Some((c, recv));
                }
            }
        }

        // At this point we must have a receiver
        let res = {
            let (_, receiver) = cancellable.as_mut().unwrap();
            receiver.poll(ctx)
        };
        match res {
            Err(_) => unreachable!(),
            Ok(Async::Ready(v)) => {
                // Get rid of the reference to the cancellable
                let _ = cancellable.take();
                match v {
                    Ok(v) => Ok(Async::Ready((obj.clone(), v))),
                    Err(e) => Err((obj.clone(), e)),
                }
            }
            Ok(Async::Pending) => Ok(Async::Pending),
        }
    }
}

impl<F, O, T, E> Drop for GioFuture<F, O, T, E> {
    fn drop(&mut self) {
        if let Some((cancellable, _)) = self.cancellable.take() {
            cancellable.cancel();
        }
    }
}
