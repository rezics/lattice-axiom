use crate::BridgeError;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BUNDLE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ASSET_COUNT: usize = 1024;

/// Validated, immutable, bounded local application files.
///
/// Files are loaded once before creating `WebView`. Requests never read arbitrary
/// disk paths, follow symlinks, or observe changes to the source directory.
#[derive(Clone, Debug)]
pub struct AssetBundle {
    files: BTreeMap<String, Arc<[u8]>>,
}

impl AssetBundle {
    /// Read a compiled static app directory with an `index.html` entrypoint.
    ///
    /// # Errors
    /// Returns filesystem errors or invalid/oversized bundle diagnostics.
    pub fn from_directory(directory: impl AsRef<Path>) -> Result<Self, BridgeError> {
        let root = directory.as_ref().canonicalize()?;
        let mut files = BTreeMap::new();
        let mut pending = vec![root.clone()];
        let mut directories = BTreeSet::from([root.clone()]);
        let mut total = 0_usize;
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let file_type = entry.file_type()?;
                if file_type.is_symlink() {
                    return Err(BridgeError::Assets(
                        "symlinks are not permitted in web bundles".into(),
                    ));
                }
                let path = entry.path().canonicalize()?;
                if !path.starts_with(&root) {
                    return Err(BridgeError::Assets("asset escapes its bundle root".into()));
                }
                if file_type.is_dir() {
                    if directories.len() >= MAX_ASSET_COUNT || !directories.insert(path.clone()) {
                        return Err(BridgeError::Assets("too many bundle directories".into()));
                    }
                    pending.push(path);
                } else if file_type.is_file() {
                    if files.len() >= MAX_ASSET_COUNT || entry.metadata()?.len() > MAX_ASSET_BYTES {
                        return Err(BridgeError::Assets(
                            "asset count or size limit exceeded".into(),
                        ));
                    }
                    let relative = path
                        .strip_prefix(&root)
                        .map_err(|error| BridgeError::Assets(error.to_string()))?;
                    let key = relative
                        .to_str()
                        .ok_or_else(|| BridgeError::Assets("asset names must be UTF-8".into()))?
                        .replace('\\', "/");
                    let mut bytes = Vec::new();
                    std::fs::File::open(&path)?
                        .take(MAX_ASSET_BYTES + 1)
                        .read_to_end(&mut bytes)?;
                    total = total.saturating_add(bytes.len());
                    if total > MAX_BUNDLE_BYTES || bytes.len() as u64 > MAX_ASSET_BYTES {
                        return Err(BridgeError::Assets("bundle size limit exceeded".into()));
                    }
                    files.insert(key, Arc::from(bytes));
                }
            }
        }
        if !files.contains_key("index.html") {
            return Err(BridgeError::Assets(
                "missing index.html; build the product's web UI first".into(),
            ));
        }
        Ok(Self { files })
    }

    /// Find explicit override, executable-adjacent `client-ui`, or debug source.
    /// Release execution never depends on a source checkout or network server.
    ///
    /// # Errors
    /// Returns a missing-bundle diagnostic or the selected bundle's load error.
    pub fn discover() -> Result<Self, BridgeError> {
        if let Some(directory) = std::env::var_os("LATTICEAXIOM_WEB_UI_DIR") {
            return Self::from_directory(PathBuf::from(directory));
        }
        let executable = std::env::current_exe()?;
        if let Some(parent) = executable.parent() {
            let adjacent = parent.join("client-ui");
            if adjacent.join("index.html").is_file() {
                return Self::from_directory(adjacent);
            }
        }
        #[cfg(debug_assertions)]
        {
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../web-ui/dist");
            if source.join("index.html").is_file() {
                return Self::from_directory(source);
            }
        }
        Err(BridgeError::Assets("no packaged client-ui/index.html found; set LATTICEAXIOM_WEB_UI_DIR to a built UI directory".into()))
    }

    pub(crate) fn get(&self, request_path: &str) -> Option<(&[u8], &'static str)> {
        let decoded = percent_encoding::percent_decode_str(request_path)
            .decode_utf8()
            .ok()?;
        let path = decoded.strip_prefix('/')?;
        if path.contains(['\\', '\0', ':'])
            || path.split('/').any(|part| part == ".." || part == ".")
        {
            return None;
        }
        let key = if path.is_empty() { "index.html" } else { path };
        let content_type = match key.rsplit('.').next()? {
            "html" => "text/html; charset=utf-8",
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "json" => "application/json",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "webp" => "image/webp",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        self.files
            .get(key)
            .map(|bytes| (bytes.as_ref(), content_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_only_the_bundle_and_rejects_traversal() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("index.html"), "original")?;
        let bundle = AssetBundle::from_directory(root.path())?;
        std::fs::write(root.path().join("index.html"), "changed")?;
        assert_eq!(
            bundle.get("/").map(|(bytes, _)| bytes),
            Some(b"original".as_slice())
        );
        for path in [
            "/../secret",
            "/%2e%2e/secret",
            "/a%5cb",
            "/C:/secret",
            "/%00",
            "/missing",
        ] {
            assert!(bundle.get(path).is_none(), "{path}");
        }
        Ok(())
    }

    #[test]
    fn missing_entrypoint_is_an_actionable_failure() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        assert!(AssetBundle::from_directory(root.path()).is_err());
        Ok(())
    }
}
