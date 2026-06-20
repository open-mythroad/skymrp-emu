use std::path::Path;

fn rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
}

fn link_search(path: &Path) {
    println!("cargo:rustc-link-search=native={}", path.to_str().unwrap());
}

fn link_lib(lib: &str) {
    println!("cargo:rustc-link-lib=static={}", lib);
}

fn main() {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = package_root.join("../../..");
    let sonivox_root = workspace_root.join("vendor/sonivox");

    let sonivox_out = cmake::Config::new(&sonivox_root)
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_TESTING", "OFF")
        .define("BUILD_APPLICATION", "OFF")
        .define("SF2_SUPPORT", "OFF")
        .define("ZLIB_SUPPORT", "OFF")
        .build();

    link_search(&sonivox_out.join("lib"));
    link_lib("sonivox");
    println!("cargo:rustc-link-arg=-lsonivox");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-arg=-lm");
    }

    rerun_if_changed(&sonivox_root);
}
