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

use futures_channel::oneshot;
use futures_util::future;
use futures_util::{FutureExt, StreamExt};

fn main() {
    let mut c = glib_futures::MainContext::default().unwrap();
    let l = glib::MainLoop::new(Some(&*c), false);

    let (sender, receiver) = oneshot::channel::<()>();

    let l_clone = l.clone();
    c.spawn(future::lazy(move |ctx| {
        println!("meh");

        let l = l_clone.clone();
        ctx.spawn(receiver.then(move |_| {
            println!("meh2");
            l.quit();
            Ok(())
        }));

        Ok(())
    }));

    let mut sender = Some(sender);
    let t = glib_futures::timeout(2000).and_then(move |_| {
        println!("meh3");
        // Get rid of sender to let the receiver trigger
        let _ = sender.take();

        Ok(())
    });
    c.spawn(t);

    let i = glib_futures::interval(500)
        .for_each(|_| {
            println!("meh4");
            Ok(())
        })
        .map(|_| ());
    c.spawn(i);

    l.run();
}
