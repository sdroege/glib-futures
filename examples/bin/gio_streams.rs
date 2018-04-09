// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

extern crate futures_channel;
extern crate futures_core;
extern crate futures_executor;
extern crate futures_util;

extern crate gio;
extern crate glib;

extern crate glib_futures;
use glib_futures::gio::*;

use futures_util::FutureExt;

fn main() {
    let mut c = glib_futures::MainContext::default().unwrap();
    let l = glib::MainLoop::new(Some(&*c), false);

    c.push_thread_default();

    let b = glib::Bytes::from_owned(vec![1, 2, 3]);
    let strm = gio::MemoryInputStream::new_from_bytes(&b);
    let buf = vec![0; 10];

    let l_clone = l.clone();
    c.spawn_local(
        strm.read_async_future(buf)
            .and_then(move |(_obj, (buf, len))| {
                println!("meh {:?}", &buf[0..len]);
                l_clone.quit();
                Ok(())
            })
            .map_err(|_| unreachable!()),
    );

    l.run();

    c.pop_thread_default();
}
