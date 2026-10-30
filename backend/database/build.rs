fn main() {
    // Cargo runs build.rs and interprets its stdout as build instructions. This directive
    // rebuilds the binary when SQLx-embedded migrations change; see the Cargo Book:
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#change-detection
    println!("cargo:rerun-if-changed=migrations");
}
