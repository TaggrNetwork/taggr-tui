use anyhow::{Context, Result};
use image::DynamicImage;
use std::path::{Path, PathBuf};

use crate::api::Client;
use crate::api::models::Post;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub blob_id: String,
    pub bucket: String,
    pub offset: u64,
    pub len: u64,
}

impl ImageRef {
    pub fn cache_key(&self) -> String {
        format!("{}_{}_{}", self.bucket, self.offset, self.len)
    }
}

/// Extract image references from a post. Keys look like `<blob_id>@<bucket>`,
/// values are `[offset, len]` (see `post.rs` and `common.tsx:1362`).
pub fn image_refs(post: &Post) -> Vec<ImageRef> {
    post.files
        .iter()
        .filter_map(|(key, (offset, len))| {
            let (blob_id, bucket) = key.rsplit_once('@')?;
            Some(ImageRef {
                blob_id: blob_id.to_string(),
                bucket: bucket.to_string(),
                offset: *offset,
                len: *len,
            })
        })
        .collect()
}

pub fn has_images(post: &Post) -> bool {
    !post.files.is_empty()
}

/// True when the markdown body references the given blob id via `/blob/<id>`.
pub fn body_references(post: &Post, blob_id: &str) -> bool {
    post.body.contains(&format!("/blob/{blob_id}"))
}

fn cache_path(cache_dir: &Path, r: &ImageRef) -> PathBuf {
    cache_dir.join(format!("{}.img", r.cache_key()))
}

/// Fetches and decodes an image, using an on-disk cache. Image blobs are
/// immutable, so caching by (bucket, offset, len) is safe.
pub async fn load_or_fetch(
    http: &reqwest::Client,
    client: &Client,
    cache_dir: &Path,
    r: &ImageRef,
) -> Result<DynamicImage> {
    std::fs::create_dir_all(cache_dir).ok();
    let path = cache_path(cache_dir, r);
    let bytes = if path.exists() {
        std::fs::read(&path).with_context(|| format!("reading cache {}", path.display()))?
    } else {
        let url = client.image_url(&r.bucket, r.offset, r.len);
        let bytes = http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();
        std::fs::write(&path, &bytes).ok();
        bytes
    };
    let image = image::load_from_memory(&bytes).context("decoding image")?;
    Ok(image)
}
