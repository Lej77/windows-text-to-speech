fn main() {
    // Find SPDFID_Text amd SPDFID_WaveFormatEx
    println!("cargo:rustc-link-lib=dylib=sapi");
}
