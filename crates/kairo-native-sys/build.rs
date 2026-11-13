//! Builds the C ABI ports. Syphon: `native/syphon/syphon_c.mm` plus Syphon's
//! base server, client and messaging (`native/syphon/vendor/Syphon`, BSD),
//! compiled straight in, as Electron's `native/syphon/build.mjs` does: no
//! Syphon.framework to ship and no Metal toolchain, since the copy is a blit.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=native");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("native/syphon");
    let vendor = root.join("vendor/Syphon");
    let prefix = vendor.join("Syphon_Prefix.pch");
    let mut sources: Vec<_> = std::fs::read_dir(&vendor)
        .expect("vendor/Syphon")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "m" || x == "c"))
        .collect();
    sources.sort();

    let mut objc = cc::Build::new();
    objc.include(root.join("vendor"))
        .include(&vendor)
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=11.0")
        .flag("-Wno-unused-parameter")
        .flag("-Wno-deprecated-declarations")
        .flag("-Wno-semicolon-before-method-body");
    for source in sources
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "m"))
    {
        objc.file(source);
    }
    objc.flag("-include")
        .flag(prefix.to_str().expect("utf-8 path"));
    objc.compile("kairo_syphon_vendor_objc");

    let mut c = cc::Build::new();
    c.flag("-mmacosx-version-min=11.0");
    for source in sources
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "c"))
    {
        c.file(source);
    }
    c.compile("kairo_syphon_vendor_c");

    cc::Build::new()
        .cpp(true)
        .file(root.join("syphon_c.mm"))
        .include(root.join("vendor"))
        .include(&vendor)
        .flag("-std=c++17")
        .flag("-ObjC++")
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=11.0")
        .flag("-Wno-unused-parameter")
        .flag("-Wno-deprecated-declarations")
        .flag("-include")
        .flag(prefix.to_str().expect("utf-8 path"))
        .compile("kairo_syphon_c");

    for framework in [
        "Metal",
        "Foundation",
        "IOSurface",
        "CoreFoundation",
        "Cocoa",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rustc-link-lib=c++");
}
