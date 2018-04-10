// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

use futures_core::Future;

use self::gio::prelude::*;
use gio_ as gio;
use glib;

use super::future::GioFuture;

// TODO: Send bound on the buffer not needed anymore after this is fixed
// https://github.com/gtk-rs/gir/issues/578

pub trait InputStreamFuture: Sized + Clone {
    // FIXME: impl trait in traits not possible yet :(
    fn read_async_future<'a, B: AsMut<[u8]> + Send + 'static>(
        &self,
        buffer: B,
    ) -> Box<Future<Item = (Self, (B, usize)), Error = (Self, (B, glib::Error))>>;
    fn close_async_future(&self) -> Box<Future<Item = (Self, ()), Error = (Self, glib::Error)>>;
}

impl<O: glib::IsA<glib::Object> + glib::IsA<gio::InputStream> + Clone + Sized + 'static>
    InputStreamFuture for O
{
    fn read_async_future<'a, B: AsMut<[u8]> + Send + 'static>(
        &self,
        buffer: B,
    ) -> Box<Future<Item = (Self, (B, usize)), Error = (Self, (B, glib::Error))>> {
        let f = GioFuture::new(self, move |obj, send| {
            let cancellable = gio::Cancellable::new();
            let mut send = Some(send);
            obj.read_async(
                buffer,
                glib::PRIORITY_DEFAULT,
                Some(&cancellable),
                move |res| {
                    let res = match res {
                        Ok((b, l)) => Ok((b, l)),
                        Err((b, e)) => Err((b, e)),
                    };

                    let _ = send.take().unwrap().send(res);
                },
            );

            cancellable
        });

        Box::new(f)
    }

    fn close_async_future(&self) -> Box<Future<Item = (Self, ()), Error = (Self, glib::Error)>> {
        let f = GioFuture::new(self, move |obj, send| {
            let cancellable = gio::Cancellable::new();
            let mut send = Some(send);
            obj.close_async(glib::PRIORITY_DEFAULT, Some(&cancellable), move |res| {
                let res = match res {
                    Ok(()) => Ok(()),
                    Err(e) => Err(e),
                };

                let _ = send.take().unwrap().send(res);
            });

            cancellable
        });

        Box::new(f)
    }
}
