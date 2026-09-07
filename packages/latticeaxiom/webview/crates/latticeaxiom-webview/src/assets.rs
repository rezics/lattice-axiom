use crate::BridgeError;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
    generated: GeneratedImages,
}

/// Bounded runtime-only PNG previews shared with the local protocol handler.
#[derive(Clone, Debug, Default)]
pub struct GeneratedImages(Arc<Mutex<GeneratedImageCache>>);

#[derive(Debug, Default)]
struct GeneratedImageCache {
    clock: u64,
    images: BTreeMap<String, (Arc<[u8]>, u64)>,
}

impl GeneratedImages {
    /// Publishes a content-addressed PNG into a 256-image, 32 MiB cache.
    ///
    /// # Errors
    /// Rejects malformed keys, oversized images, or a poisoned cache lock.
    pub fn publish(&self, key: &str, png: Vec<u8>) -> Result<(), BridgeError> {
        if key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()) || png.len() > 128 * 1024
        {
            return Err(BridgeError::Assets("invalid generated image".into()));
        }
        let mut cache = self
            .0
            .lock()
            .map_err(|_| BridgeError::Assets("preview cache lock failed".into()))?;
        if cache.images.len() >= 256
            && !cache.images.contains_key(key)
            && let Some(oldest) = cache
                .images
                .iter()
                .min_by_key(|(_, (_, age))| *age)
                .map(|(key, _)| key.clone())
        {
            cache.images.remove(&oldest);
        }
        cache.clock = cache.clock.saturating_add(1);
        let age = cache.clock;
        cache.images.insert(key.to_owned(), (Arc::from(png), age));
        Ok(())
    }

    fn get(&self, key: &str) -> Option<Arc<[u8]>> {
        let mut cache = self.0.lock().ok()?;
        cache.clock = cache.clock.saturating_add(1);
        let age = cache.clock;
        let (image, used) = cache.images.get_mut(key)?;
        *used = age;
        Some(image.clone())
    }

    /// Checks and retains a visible preview in the bounded LRU cache.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
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
        Ok(Self {
            files,
            generated: GeneratedImages::default(),
        })
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

    /// Shares a product-owned preview cache with this immutable application bundle.
    #[must_use]
    pub fn with_generated(mut self, generated: GeneratedImages) -> Self {
        self.generated = generated;
        self
    }

    pub(crate) fn get(&self, request_path: &str) -> Option<(Arc<[u8]>, &'static str)> {
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
        if let Some(key) = key
            .strip_prefix("generated/")
            .and_then(|key| key.strip_suffix(".png"))
        {
            return self.generated.get(key).map(|bytes| (bytes, "image/png"));
        }
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
            .map(|bytes| (bytes.clone(), content_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_images_keep_visible_entries_and_bound_retention() -> Result<(), BridgeError> {
        let cache = GeneratedImages::default();
        for number in 0..256 {
            cache.publish(&format!("{number:064x}"), vec![1])?;
        }
        assert!(cache.contains(&format!("{:064x}", 0)));
        cache.publish(&format!("{:064x}", 256), vec![2])?;
        assert!(cache.contains(&format!("{:064x}", 0)));
        assert!(!cache.contains(&format!("{:064x}", 1)));
        assert!(cache.publish("../outside", vec![1]).is_err());
        assert!(
            cache
                .publish(&format!("{:064x}", 257), vec![0; 128 * 1024 + 1])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn snapshots_only_the_bundle_and_rejects_traversal() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("index.html"), "original")?;
        let bundle = AssetBundle::from_directory(root.path())?;
        std::fs::write(root.path().join("index.html"), "changed")?;
        assert_eq!(
            bundle.get("/").map(|(bytes, _)| bytes.to_vec()),
            Some(b"original".to_vec())
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
