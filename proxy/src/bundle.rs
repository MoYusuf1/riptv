//! Single-file releases unpack trusted, compiled-in resources into a private session folder.
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

include!(concat!(env!("OUT_DIR"), "/bundle.rs"));
static ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

pub struct Bundle(PathBuf);

impl Drop for Bundle {
    fn drop(&mut self) {
        // Only this process's randomly named, newly created directory is owned here.
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn prepare() -> io::Result<Option<Bundle>> {
    if FILES.is_empty() {
        return Ok(None);
    }
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
    let token: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let folder = std::env::temp_dir().join(format!("riptv-{token}"));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&folder)?;
    let bundle = Bundle(folder.clone());
    for &(relative, bytes) in FILES {
        if !safe_path(relative) {
            return Err(io::Error::other("invalid embedded resource path"));
        }
        // Packaging stores each file zstd-compressed (`name.zst`): a third of the download.
        let compressed = relative.ends_with(".zst");
        let relative = relative.strip_suffix(".zst").unwrap_or(relative);
        let path = folder.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::File::create(&path)?;
        if compressed {
            unpack_into(bytes, &mut file)?;
        } else {
            use std::io::Write;
            file.write_all(bytes)?;
        }
        #[cfg(unix)]
        if relative.starts_with("bin/") {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
    }
    ROOT.set(folder)
        .map_err(|_| io::Error::other("bundle already initialized"))?;
    Ok(Some(bundle))
}

fn unpack_into(compressed: &[u8], out: &mut impl io::Write) -> io::Result<()> {
    let mut decoder =
        ruzstd::decoding::StreamingDecoder::new(compressed).map_err(io::Error::other)?;
    io::copy(&mut decoder, out)?;
    Ok(())
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_paths_outside_private_folder() {
        for path in ["", "/etc/passwd", "../file", "web/../../file"] {
            assert!(!safe_path(path));
        }
        assert!(safe_path("web/assets/app.wasm"));
        assert!(safe_path("bin/ffmpeg.exe"));
    }

    #[test]
    fn unpacks_what_packaging_compressed() {
        // `python3 -c "from compression import zstd; print(list(zstd.compress(b'RIPTV ' * 4, 19)))"`
        let packed = [
            40, 181, 47, 253, 32, 24, 109, 0, 0, 56, 82, 73, 80, 84, 86, 32, 82, 1, 0, 153, 139, 17,
        ];
        let mut out = Vec::new();
        unpack_into(&packed, &mut out).unwrap();
        assert_eq!(out, b"RIPTV ".repeat(4));
        assert!(unpack_into(b"not zstd", &mut Vec::new()).is_err());
    }
}
