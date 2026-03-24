// build.rs - Required for napi native modules
extern crate napi_build;

fn main() {
    napi_build::setup();
}