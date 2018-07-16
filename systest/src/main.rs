#![allow(bad_style, improper_ctypes)]

extern crate miniz_sys;
extern crate libc;

use libc::*;
use miniz_sys::*;

include!(concat!(env!("OUT_DIR"), "/all.rs"));
