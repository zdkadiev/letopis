use sha2::{Digest, Sha256};
use std::{env, fs};

fn main() {
    const SOURCE: &str = "vendor/vec1/vec1.c";
    const HASH: &str = "a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9";
    for input in [SOURCE, "vendor/vec1/README.md", "build.rs"] {
        println!("cargo:rerun-if-changed={input}");
    }
    println!("cargo:rerun-if-env-changed=DEP_SQLITE3_INCLUDE");
    println!("cargo:rerun-if-env-changed=TARGET");
    let source = fs::read(SOURCE).expect("read pinned vec1 source");
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        HASH,
        "vec1 checksum mismatch"
    );
    let include = env::var_os("DEP_SQLITE3_INCLUDE").expect("bundled SQLite header metadata");
    cc::Build::new()
        .file(SOURCE)
        .include(include)
        .define("SQLITE_CORE", None)
        .define("sqlite3_extension_init", "letopis_vec1_init")
        .warnings(false)
        .compile("letopis_vec1");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=pthread");
    }
}
