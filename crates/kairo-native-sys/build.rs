//! Builds the macOS native code: the recorder (prompt 064), a C ABI port of
//! Electron's `native/recorder/recorder.mm` at commit af978c5.
fn main() {
    println!("cargo:rerun-if-changed=native");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .cpp(true)
        .file("native/recorder.mm")
        .flag("-std=c++17")
        .flag("-ObjC++")
        .flag("-fobjc-arc")
        .flag("-Wno-deprecated-declarations")
        .flag("-mmacosx-version-min=11.0")
        .compile("kairo_recorder");
    for framework in [
        "AVFoundation",
        "CoreMedia",
        "CoreVideo",
        "Accelerate",
        "Foundation",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rustc-link-lib=c++");
}
