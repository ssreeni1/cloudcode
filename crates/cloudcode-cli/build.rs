use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::Builder;

fn parse_checksums(path: &Path) -> std::collections::HashMap<String, String> {
    let Ok(content) = fs::read_to_string(path) else {
        return std::collections::HashMap::new();
    };

    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let file = parts.next()?;
            Some((file.trim_start_matches('*').to_string(), hash.to_string()))
        })
        .collect()
}

fn sha256_hex(path: &Path) -> String {
    let output = std::process::Command::new("shasum")
        .args(["-a", "256", &path.to_string_lossy()])
        .output()
        .expect("failed to run shasum -a 256");
    if !output.status.success() {
        panic!("shasum -a 256 failed for {}", path.display());
    }

    String::from_utf8(output.stdout)
        .expect("shasum output was not utf-8")
        .split_whitespace()
        .next()
        .expect("shasum output missing digest")
        .to_string()
}

fn create_source_bundle(workspace_root: &Path, out_dir: &Path) -> PathBuf {
    let bundle_path = out_dir.join("cloudcode-source.tar.gz");
    let file = fs::File::create(&bundle_path).unwrap_or_else(|e| {
        panic!(
            "failed to create source bundle {}: {e}",
            bundle_path.display()
        )
    });
    let encoder = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(encoder);

    for file_name in ["Cargo.toml", "Cargo.lock"] {
        let path = workspace_root.join(file_name);
        println!("cargo:rerun-if-changed={}", path.display());
        tar.append_path_with_name(&path, Path::new("cloudcode-src").join(file_name))
            .unwrap_or_else(|e| panic!("failed to add {} to source bundle: {e}", path.display()));
    }

    let crates_dir = workspace_root.join("crates");
    println!("cargo:rerun-if-changed={}", crates_dir.display());
    tar.append_dir_all("cloudcode-src/crates", &crates_dir)
        .unwrap_or_else(|e| panic!("failed to add crates/ to source bundle: {e}"));

    tar.finish()
        .expect("failed to finalize source bundle tarball");

    bundle_path
}

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
    println!("cargo:rerun-if-env-changed=CLOUDCODE_EMBED_DAEMONS");

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_dir_path = Path::new(&out_dir);
    let mut code = String::new();
    let embed_raw = env::var("CLOUDCODE_EMBED_DAEMONS").unwrap_or_default();
    let embed_binaries = embed_raw == "1";
    let require_embedded = env::var("CLOUDCODE_REQUIRE_EMBEDDED_DAEMONS")
        .map(|value| value == "1")
        .unwrap_or(embed_binaries);
    let checksums_path = Path::new(&bin_dir).join("SHA256SUMS");
    let checksums = parse_checksums(&checksums_path);
    println!("cargo:rerun-if-changed={}", checksums_path.display());
    println!("cargo:rerun-if-env-changed=CLOUDCODE_REQUIRE_EMBEDDED_DAEMONS");

    for (triple, const_name, checksum_const) in [
        (
            "x86_64-unknown-linux-gnu",
            "DAEMON_X86_64",
            "DAEMON_X86_64_SHA256",
        ),
        (
            "aarch64-unknown-linux-gnu",
            "DAEMON_AARCH64",
            "DAEMON_AARCH64_SHA256",
        ),
    ] {
        let path = format!("{}/{}/cloudcode-daemon", bin_dir, triple);
        println!("cargo:rerun-if-changed={}", path);

        let checksum_key = format!("{}/cloudcode-daemon", triple);
        if embed_binaries && Path::new(&path).exists() {
            let abs = fs::canonicalize(&path).unwrap();
            let digest = sha256_hex(&abs);
            match checksums.get(&checksum_key) {
                Some(expected) if expected == &digest => {
                    code += &format!(
                        "pub const {}: Option<&[u8]> = Some(include_bytes!(r\"{}\"));\n",
                        const_name,
                        abs.display()
                    );
                    code += &format!(
                        "pub const {}: Option<&str> = Some(\"{}\");\n",
                        checksum_const, digest
                    );
                }
                Some(expected) => {
                    panic!(
                        "Checksum mismatch for {}: expected {}, got {}",
                        checksum_key, expected, digest
                    );
                }
                None => {
                    panic!(
                        "Missing checksum entry for {} in {}",
                        checksum_key,
                        checksums_path.display()
                    );
                }
            }
        } else if require_embedded {
            panic!(
                "Missing embedded daemon binary for {} at {}. Release/Homebrew builds must provide embedded daemon payloads.",
                triple, path
            );
        } else {
            code += &format!("pub const {}: Option<&[u8]> = None;\n", const_name);
            code += &format!("pub const {}: Option<&str> = None;\n", checksum_const);
        }
    }

    let source_bundle = create_source_bundle(workspace_root, out_dir_path);
    let source_bundle_abs =
        fs::canonicalize(&source_bundle).expect("failed to canonicalize source bundle");
    let source_bundle_digest = sha256_hex(&source_bundle_abs);
    println!("cargo:rerun-if-changed={}", source_bundle_abs.display());
    code += &format!(
        "pub const SOURCE_BUNDLE: &[u8] = include_bytes!(r\"{}\");\n",
        source_bundle_abs.display()
    );
    code += &format!(
        "pub const SOURCE_BUNDLE_SHA256: &str = \"{}\";\n",
        source_bundle_digest
    );

    fs::write(format!("{}/embedded_daemons.rs", out_dir), code).unwrap();
}
