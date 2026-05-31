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

    let dynarmic_out = cmake::build(workspace_root.join("vendor/dynarmic"));
    link_search(&dynarmic_out.join("lib"));
    link_lib("dynarmic");
    link_search(&dynarmic_out.join("build/externals/fmt"));
    link_lib("fmtd");
    link_search(&dynarmic_out.join("build/externals/mcl/src"));
    link_lib("mcl");

    cc::Build::new()
        .file(package_root.join("lib.cpp"))
        .cpp(true)
        .std("c++17")
        .include(dynarmic_out.join("include"))
        .compile("dynarmic_wrapper");
    rerun_if_changed(&package_root.join("lib.cpp"));
}
