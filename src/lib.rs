extern crate futures_channel;
extern crate futures_core;
extern crate futures_executor;

extern crate gio;
extern crate glib;
extern crate glib_sys as glib_ffi;

use futures_channel::oneshot;

mod executor;
pub use executor::*;
mod sources;
pub use sources::*;
