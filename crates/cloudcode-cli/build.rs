use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not determine workspace root");

    let bin_dir = env::var("DAEMON_BINARY_DIR").unwrap_or_else(|_| {
        workspace_root
            .join("target")
            .join("daemon-binaries")
            .to_string_lossy()
            .to_string()
    });

    println!("cargo:rerun-if-env-changed=DAEMON_BINARY_DIR");

    let out_dir = env::var("OUT_DIR").unwrap();
    let mut code = String::new();

    for (triple, const_name) in [
        ("x86_64-unknown-linux-gnu", "DAEMON_X86_64"),
        ("aarch64-unknown-linux-gnu", "DAEMON_AARCH64"),
    ] {
        let path = format!("{}/{}/cloudcode-daemon", bin_dir, triple);
        println!("cargo:rerun-if-changed={}", path);

        if Path::new(&path).exists() {
            let abs = fs::canonicalize(&path).unwrap();
            code += &format!(
                "pub const {}: Option<&[u8]> = Some(include_bytes!(r\"{}\"));\n",
                const_name,
                abs.display()
            );
        } else {
            code += &format!("pub const {}: Option<&[u8]> = None;\n", const_name);
        }
    }

    fs::write(format!("{}/embedded_daemons.rs", out_dir), code).unwrap();
}
