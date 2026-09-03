//! Shared local-OCR configuration and execution.
//!
//! Attachment readers must not each choose their own cache, model, confidence
//! threshold, or download policy. This module is their single seam.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct OcrRuntime {
    #[cfg(feature = "ocr-local")]
    local: local::LocalRuntime,
}

/// A locally installed, checksum-verified OCR model set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledOcrModels {
    pub(crate) manifest_id: String,
    pub(crate) revision: String,
    pub(crate) model_directory: PathBuf,
}

#[cfg(feature = "ocr-local")]
pub(crate) fn install_models(model_cache_directory: &Path) -> anyhow::Result<InstalledOcrModels> {
    use pdf_inspector::vision::{
        HttpModelDownloader, ModelDownloadPolicy, ModelStore, PP_OCR_V6_SMALL,
    };

    // `ModelStore` owns the lock, atomic writes, exact-size validation, and
    // SHA-256 verification.  This explicit command is the only acquisition
    // path; attachment reads always resolve models offline.
    let models = ModelStore::new(model_cache_directory).resolve_or_download(
        &PP_OCR_V6_SMALL,
        ModelDownloadPolicy::IfMissing,
        &HttpModelDownloader::default(),
    )?;
    Ok(InstalledOcrModels {
        manifest_id: models.manifest_id().to_owned(),
        revision: models.revision().to_owned(),
        model_directory: model_cache_directory
            .join(models.manifest_id())
            .join(models.revision()),
    })
}

#[cfg(not(feature = "ocr-local"))]
pub(crate) fn install_models(_: &Path) -> anyhow::Result<InstalledOcrModels> {
    anyhow::bail!(
        "install-ocr-models requires building obsidian-vault-mcp with --features ocr-local"
    )
}

impl OcrRuntime {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(feature = "ocr-local")]
            local: local::LocalRuntime::new(),
        }
    }
}

#[cfg(feature = "ocr-local")]
mod local {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use anyhow::Context;
    use pdf_inspector::vision::{
        ModelDownloadPolicy, ModelStore, OarOcrEngine, OcrEngine, OcrMode, OcrOptions, OcrPage,
        OcrPdfOptions, PP_OCR_V6_SMALL, PageTransform, RenderPixelFormat, RenderedPage,
    };

    use super::OcrRuntime;

    const MODEL_DIRECTORY_ENV: &str = "OBSIDIAN_VAULT_MCP_OCR_MODEL_DIR";
    const MINIMUM_CONFIDENCE: f32 = 0.45;

    #[derive(Debug)]
    pub(crate) struct LocalRuntime {
        model_directory: Option<PathBuf>,
        engine: Mutex<Option<Arc<OarOcrEngine>>>,
    }

    impl LocalRuntime {
        pub(super) fn new() -> Self {
            Self {
                model_directory: std::env::var_os(MODEL_DIRECTORY_ENV).map(PathBuf::from),
                engine: Mutex::new(None),
            }
        }

        fn options(&self, mode: OcrMode) -> OcrOptions {
            let options = OcrOptions::new()
                .mode(mode)
                .minimum_confidence(MINIMUM_CONFIDENCE)
                // Runtime installation owns model acquisition. Reads remain
                // offline and deterministic even when a cache is missing.
                .model_downloads(ModelDownloadPolicy::Offline);
            match &self.model_directory {
                Some(path) => options.model_directory(path),
                None => options,
            }
        }

        fn engine(&self) -> anyhow::Result<Arc<OarOcrEngine>> {
            let mut engine = self.engine.lock().expect("OCR engine lock");
            if let Some(engine) = engine.as_ref() {
                return Ok(Arc::clone(engine));
            }
            let options = self.options(OcrMode::Force);
            let models = ModelStore::from_options(&options)?
                .resolve(&PP_OCR_V6_SMALL)
                .with_context(|| {
                    format!(
                        "local OCR model is unavailable; install it or set {MODEL_DIRECTORY_ENV}"
                    )
                })?;
            let loaded = Arc::new(OarOcrEngine::from_models(&models)?);
            *engine = Some(Arc::clone(&loaded));
            Ok(loaded)
        }
    }

    impl OcrRuntime {
        fn local(&self) -> &LocalRuntime {
            &self.local
        }

        pub(crate) fn pdf_options(&self, page: Option<u32>) -> OcrPdfOptions {
            let mut options = OcrPdfOptions::auto().ocr(self.local().options(OcrMode::Auto));
            if let Some(page) = page {
                options = options.page_numbers([page]);
            }
            options
        }

        pub(crate) fn recognize_rgb(
            &self,
            width: u32,
            height: u32,
            pixels: Vec<u8>,
        ) -> anyhow::Result<OcrPage> {
            let transform = PageTransform::from_corners(
                width,
                height,
                (0.0, f64::from(height)),
                (f64::from(width), f64::from(height)),
                (0.0, 0.0),
            )
            .context("could not establish OCR coordinate transform")?;
            let page = RenderedPage::new(
                1,
                width as f32,
                height as f32,
                width,
                height,
                width as usize * RenderPixelFormat::Rgb8.bytes_per_pixel(),
                RenderPixelFormat::Rgb8,
                pixels,
                transform,
            )?;
            self.local()
                .engine()?
                .recognize(&[page], &self.local().options(OcrMode::Force))?
                .into_iter()
                .next()
                .context("OCR returned no page")
        }
    }
}
