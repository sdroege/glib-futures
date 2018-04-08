extern crate futures_channel;
extern crate futures_core;
extern crate futures_executor;
extern crate futures_util;

extern crate gio;
extern crate glib;
extern crate glib_sys as glib_ffi;

use futures_channel::oneshot;
use futures_util::future;
use futures_util::{FutureExt, StreamExt};

mod executor;
mod sources;

fn main() {
    let mut c = executor::MainContext::default().unwrap();
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
    let t = sources::timeout(2000).and_then(move |_| {
        println!("meh3");
        // Get rid of sender to let the receiver trigger
        let _ = sender.take();

        Ok(())
    });
    c.spawn(t);

    let i = sources::interval(500)
        .for_each(|_| {
            println!("meh4");
            Ok::<(), futures_core::Never>(())
        })
        .map(|_| ());
    c.spawn(i);

    l.run();
}
