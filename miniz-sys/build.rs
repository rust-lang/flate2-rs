extern crate gcc;

use std::default::Default;

fn main() {
    gcc::compile_library("libminiz.a", &Default::default(), &["miniz.c"]);
}
