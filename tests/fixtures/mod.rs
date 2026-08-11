// Each test target that declares `mod fixtures;` compiles this whole file and
// uses a slice of it, so the rest reports as dead. Same situation as
// tests/common/mod.rs.
#![allow(dead_code, unused_imports)]

pub mod create_repos;
pub mod test_data;