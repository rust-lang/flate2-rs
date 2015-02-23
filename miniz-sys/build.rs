extern crate gcc;

fn main() {
    gcc::compile_library("libminiz.a", &["miniz.c"]);
}
