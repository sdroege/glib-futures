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

use std::str;

use futures_util::FutureExt;
use futures_util::future;

fn main() {
    let mut c = glib_futures::MainContext::default().unwrap();
    let l = glib::MainLoop::new(Some(&*c), false);

    c.push_thread_default();

    let file = gio::File::new_for_path("Cargo.toml");

    let l_clone = l.clone();
    c.spawn_local(
        // Try to open the file
        file.read_async_future()
            .map_err(|(_file, err)| {
                eprintln!("Failed to open file: {}", err);

                ()
            })
            .and_then(|(_file, strm)| {
                // If opening the file succeeds, we asynchronously loop and
                // read the file in up to 64 byte chunks and re-use the same
                // vec for each read
                let buf = vec![0; 64];
                future::loop_fn((strm, buf), |(strm, buf)| {
                    strm.read_async_future(buf)
                        .map_err(|(_strm, (_b, err))| {
                            eprintln!("Failed to read from stream: {}", err);

                            ()
                        })
                        .and_then(move |(_obj, (buf, len))| {
                            println!("meh {:?}", str::from_utf8(&buf[0..len]).unwrap());
                            // Once 0 is returned, we know that we're done with reading
                            // and asynchronously close the stream, otherwise we loop again
                            // with the same stream/buffer
                            if len == 0 {
                                Ok(future::Loop::Break(strm.close_async_future()))
                            } else {
                                Ok(future::Loop::Continue((strm, buf)))
                            }
                        })
                })
            })
            // Once all is done, i.e. the stream was closed above, we quit the main loop
            .and_then(move |_| {
                l_clone.quit();
                Ok(())
            })
            .map_err(|_| unreachable!()),
    );

    l.run();

    c.pop_thread_default();
}
