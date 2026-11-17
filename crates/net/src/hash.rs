//! Asset hash: host and client must run the same defs and levels.

use std::io;
use std::path::{Path, PathBuf};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// [`fnv`] over `bytes` with every `\r\n` taken as `\n`.
fn fnv_lf(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0;
    while i < bytes.len() {
        let crlf = bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n');
        if !crlf {
            h = fnv(h, &bytes[i..=i]);
        }
        i += 1;
    }
    h
}

/// FNV-1a 64 over `defs/*.ron` and `levels/*.ron` under `assets`, in sorted relative-path
/// order: each path's bytes, a 0 byte, the file's bytes, a 0 byte. Every `\r\n` in a file is
/// hashed as `\n` (a lone `\r` stays), so a Windows checkout with CRLF line endings hashes
/// like a Linux one. A missing directory
/// contributes nothing. Only top-level `*.ron` files (extension matched case-sensitively) in
/// `defs/` and `levels/` count; subdirectories and other files are ignored.
pub fn assets_hash(assets: &Path) -> io::Result<u64> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for dir in ["defs", "levels"] {
        let entries = match std::fs::read_dir(assets.join(dir)) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "ron") && path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                files.push((format!("{dir}/{name}"), path));
            }
        }
    }
    files.sort();
    let mut h = FNV_OFFSET;
    for (rel, path) in files {
        h = fnv(h, rel.as_bytes());
        h = fnv(h, &[0]);
        h = fnv_lf(h, &std::fs::read(path)?);
        h = fnv(h, &[0]);
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rr-net-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("defs")).unwrap();
        fs::create_dir_all(d.join("levels")).unwrap();
        d
    }

    fn put(root: &Path, rel: &str, body: &str) {
        fs::write(root.join(rel), body).unwrap();
    }

    #[test]
    fn assets_hash_is_stable_and_sensitive() {
        let a = temp("a");
        put(&a, "defs/enemies.ron", "one");
        put(&a, "defs/weapons.ron", "two");
        put(&a, "levels/x.ron", "level");
        let h = assets_hash(&a).unwrap();
        assert_eq!(assets_hash(&a).unwrap(), h);

        // Created in the opposite order.
        let b = temp("b");
        put(&b, "levels/x.ron", "level");
        put(&b, "defs/weapons.ron", "two");
        put(&b, "defs/enemies.ron", "one");
        assert_eq!(assets_hash(&b).unwrap(), h);

        // A non-.ron file is ignored.
        put(&b, "defs/notes.txt", "ignored");
        assert_eq!(assets_hash(&b).unwrap(), h);

        // One byte changed.
        put(&b, "levels/x.ron", "levem");
        assert_ne!(assets_hash(&b).unwrap(), h);
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn assets_hash_ignores_crlf() {
        let lf = temp("lf");
        put(&lf, "defs/enemies.ron", "(\n  a: 1,\n)\n");
        put(&lf, "levels/x.ron", "level\nline\r");
        let crlf = temp("crlf");
        put(&crlf, "defs/enemies.ron", "(\r\n  a: 1,\r\n)\r\n");
        put(&crlf, "levels/x.ron", "level\r\nline\r");
        assert_eq!(assets_hash(&crlf).unwrap(), assets_hash(&lf).unwrap());
        // A lone `\r` is content, not a line ending.
        put(&crlf, "levels/x.ron", "level\nline\n");
        assert_ne!(assets_hash(&crlf).unwrap(), assets_hash(&lf).unwrap());
        let _ = fs::remove_dir_all(&lf);
        let _ = fs::remove_dir_all(&crlf);
    }

    #[test]
    fn shipped_assets_hash() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let h = assets_hash(&assets).expect("hashes");
        assert_ne!(h, 0);
    }
}
