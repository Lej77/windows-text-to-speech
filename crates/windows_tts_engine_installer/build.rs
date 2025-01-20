fn main() {
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#change-detection
    println!("cargo::rerun-if-changed=build.rs"); // <- enable fine grained change detection.

    // https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        println!("cargo::rerun-if-changed=\"Cargo.toml\"");
        // println!("cargo::rerun-if-changed=\"{ICON}\"");

        let res = winresource::WindowsResource::new();
        // res.set_icon(ICON);
        res.compile().unwrap();
    }
}
