// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the MIT license, see the LICENSE file or <http://opensource.org/licenses/MIT>

extern crate futures_channel;
extern crate futures_core;
extern crate futures_executor;

extern crate glib;
extern crate glib_sys as glib_ffi;

#[cfg(feature = "gio_api")]
extern crate gio as gio_;

mod executor;
pub use executor::*;
mod sources;
pub use sources::*;

#[cfg(feature = "gio_api")]
pub mod gio;
