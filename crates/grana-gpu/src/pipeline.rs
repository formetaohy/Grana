use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wgpu::{Device, PipelineCache, PipelineCacheDescriptor};

const MAXIMUM_BYTES: u64 = 64 << 20;

#[derive(Clone)]
pub struct Pipelines {
    inner: Arc<Inner>,
}

struct Inner {
    cache: Option<PipelineCache>,
    path: Option<PathBuf>,
    saved: AtomicBool,
}

impl Pipelines {
    pub fn new(device: &Device) -> Self {
        let path = grana_dir::cache().map(|dir| dir.join("pipeline").join("cache.bin"));
        let cache = device
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
            .then(|| {
                let data = path.as_ref().and_then(|path| read(path));
                unsafe {
                    device.create_pipeline_cache(&PipelineCacheDescriptor {
                        label: Some("grana.pipeline.cache"),
                        data: data.as_deref(),
                        fallback: true,
                    })
                }
            });
        Self {
            inner: Arc::new(Inner {
                cache,
                path,
                saved: AtomicBool::new(false),
            }),
        }
    }

    pub fn cache(&self) -> Option<&PipelineCache> {
        self.inner.cache.as_ref()
    }

    pub fn save(&self) {
        let Some(cache) = self.inner.cache.as_ref() else {
            return;
        };
        let Some(path) = self.inner.path.as_ref() else {
            return;
        };
        if self.inner.saved.swap(true, Ordering::Relaxed) {
            return;
        }
        let Some(data) = cache.get_data() else {
            return;
        };
        if data.is_empty() {
            return;
        }
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let temporary = path.with_extension("tmp");
        if fs::write(&temporary, &data).is_err() {
            return;
        }
        let _ = fs::rename(&temporary, path);
    }
}

fn read(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() == 0 || metadata.len() > MAXIMUM_BYTES {
        return None;
    }
    fs::read(path).ok()
}
