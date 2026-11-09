fn main() {
    println!("cargo:rerun-if-changed=native");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .cpp(true)
        .file("native/c_abi.mm")
        .flag("-std=c++17")
        .flag("-ObjC++")
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=11.0")
        .compile("gpu_frames_c");
    for framework in ["Metal", "Foundation", "IOSurface", "CoreFoundation"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rustc-link-lib=c++");
}
