#![allow(bad_style, improper_ctypes)]

extern crate libc;
extern crate miniz_sys;

use libc::*;
use miniz_sys::*;

include!(concat!(env!("OUT_DIR"), "/all.rs"));
