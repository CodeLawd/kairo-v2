fn main() {
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("-windows-msvc")) {
        // The windowing library imports TaskDialogIndirect from Common Controls v6.
        println!("cargo::rustc-link-arg-bin=kairo=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg-bin=kairo=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
