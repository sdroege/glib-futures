// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

use futures_core::Future;

use self::gio::prelude::*;
use gio_ as gio;
use glib;

use super::future::GioFuture;

pub trait FileFuture: Sized + Clone {
    // FIXME: impl trait in traits not possible yet :(
    fn read_async_future(
        &self,
    ) -> Box<Future<Item = (Self, gio::FileInputStream), Error = (Self, glib::Error)>>;
}

impl<O: glib::IsA<glib::Object> + glib::IsA<gio::File> + Clone + Sized + 'static> FileFuture for O {
    fn read_async_future(
        &self,
    ) -> Box<Future<Item = (Self, gio::FileInputStream), Error = (Self, glib::Error)>> {
        let f = GioFuture::new(self, move |obj, send| {
            // TODO: SendCell not needed for the sender anymore once this is fixed
            // https://github.com/gtk-rs/gir/issues/578
            use send_cell::SendCell;
            let send = SendCell::new(send);

            let cancellable = gio::Cancellable::new();
            let mut send = Some(send);
            obj.read_async(glib::PRIORITY_DEFAULT, Some(&cancellable), move |res| {
                let res = match res {
                    Ok(s) => Ok(s),
                    Err(e) => Err(e),
                };

                let _ = send.take().unwrap().into_inner().send(res);
            });

            cancellable
        });

        Box::new(f)
    }
}
