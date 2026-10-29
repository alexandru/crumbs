fn main() {
    // The SQLx migration macro embeds these files, so Cargo must rebuild when they change.
    println!("cargo:rerun-if-changed=migrations");
}
