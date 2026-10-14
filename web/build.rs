/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::path::Path;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("emscripten") {
        return;
    }

    let loader = Path::new(env!("CARGO_MANIFEST_DIR")).join("loader.js");
    println!("cargo:rerun-if-changed={}", loader.display());
    println!("cargo:rustc-link-arg=-sINVOKE_RUN=0");
    println!("cargo:rustc-link-arg=-sEXPORTED_RUNTIME_METHODS=FS,callMain");
    println!("cargo:rustc-link-arg=--pre-js={}", loader.display());
}
