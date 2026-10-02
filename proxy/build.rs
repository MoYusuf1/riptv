//! Release builds embed their resources; normal source builds remain lightweight.
use std::{env, fs, path::Path};

fn collect(root: &Path, dir: &Path, entries: &mut Vec<(String, String)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir).expect("read bundle") {
        let path = entry.expect("bundle entry").path();
        assert!(!path.is_symlink(), "bundle must not contain symlinks");
        if path.is_dir() {
            collect(root, &path, entries);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            entries.push((relative, path.to_str().unwrap().to_owned()));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=IPTV_BUNDLE_DIR");
    let mut entries = Vec::new();
    if let Some(root) = env::var_os("IPTV_BUNDLE_DIR") {
        let root = fs::canonicalize(root).expect("bundle directory");
        assert!(
            root.join("web/index.html").is_file(),
            "bundle needs web app"
        );
        collect(&root, &root, &mut entries);
        entries.sort();
    }
    let mut source = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (relative, path) in entries {
        source.push_str(&format!("({relative:?}, include_bytes!({path:?})),\n"));
    }
    source.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("bundle.rs"),
        source,
    )
    .unwrap();
}
