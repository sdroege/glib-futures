// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

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
