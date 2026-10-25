//! Windows: embed the application icon (and version info) into the exe.

#[cfg(windows)]
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon.ico");
        res.set("ProductName", "PinShot");
        res.set("FileDescription", "PinShot");
        if let Err(e) = res.compile() {
            println!("cargo:warning=cannot embed the exe icon: {e}");
        }
    }
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
}

#[cfg(not(windows))]
fn main() {}
