use std::{fs, io::Read};

use anyhow::Context;
use quick_xml::{Reader, events::Event};
use schemars::JsonSchema;
use serde::Serialize;
use zip::ZipArchive;

use crate::{query::VaultQueries, vault::DEFAULT_MAX_READ_ATTACHMENT_CHARS};

/// The first attachment-reader slice supports native PDF text, DOCX body text,
/// and PNG through the same optional local OCR runtime used for scanned PDFs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentKind {
    Pdf,
    Docx,
    Png,
    Jpeg,
}

impl AttachmentKind {
    pub fn parse(path: &str) -> anyhow::Result<Self> {
        let extension = camino::Utf8Path::new(path)
            .extension()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "pdf" => Ok(Self::Pdf),
            "docx" => Ok(Self::Docx),
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            _ => anyhow::bail!(
                "unsupported attachment type for {path}; supported types are PDF, DOCX, PNG, and JPEG"
            ),
        }
    }

    fn format(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ReadAttachmentResult {
    /// Attachment path, optionally narrowed to a one-based PDF page.
    pub source: String,
    /// `pdf`, `docx`, `png`, or `jpeg`.
    pub format: String,
    /// `complete`, `partial`, or `ocr_required`.
    pub status: String,
    /// Native extraction or local OCR.
    pub extraction: String,
    pub content: String,
    /// PDF pages that could not be read natively and require local OCR.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_required_pages: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    /// OCR results from requested embedded DOCX images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_sources: Option<Vec<AttachmentOcrSource>>,
    #[serde(skip_serializing_if = "is_false")]
    #[schemars(with = "Option<bool>")]
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AttachmentOcrSource {
    /// Location inside the attachment package, for example `word/media/image1.png`.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_confidence: Option<f32>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl VaultQueries {
    #[tracing::instrument(
        name = "vault.query.read_attachment",
        skip_all,
        fields(operation.kind = "query", operation.name = "read_attachment"),
        err
    )]
    pub fn read_attachment(
        &self,
        requested_path: &str,
        page: Option<u32>,
        max_chars: Option<usize>,
        include_embedded_images: bool,
    ) -> anyhow::Result<ReadAttachmentResult> {
        let kind = AttachmentKind::parse(requested_path)?;
        let path = self.vault.resolve_attachment_path(requested_path)?;
        if !path.is_file() {
            anyhow::bail!("attachment does not exist: {requested_path}");
        }
        let maximum = self.vault.config().max_note_bytes;
        let size = fs::metadata(&path)?.len();
        if size > maximum as u64 {
            anyhow::bail!("attachment exceeds configured maximum size: {size} > {maximum} bytes");
        }
        let relative_path = self.vault.relative_path(&path);
        let budget = max_chars.unwrap_or(DEFAULT_MAX_READ_ATTACHMENT_CHARS);

        match kind {
            AttachmentKind::Pdf => read_pdf(&self.ocr_runtime, &path, &relative_path, page, budget),
            AttachmentKind::Docx => {
                if page.is_some() {
                    anyhow::bail!("page is only supported for PDF attachments");
                }
                let docx = read_docx(&path, include_embedded_images, &self.ocr_runtime)?;
                Ok(with_budget(
                    relative_path,
                    kind,
                    "native_text",
                    docx.content,
                    budget,
                    Vec::new(),
                    docx.warnings,
                    docx.ocr_sources,
                ))
            }
            AttachmentKind::Png | AttachmentKind::Jpeg => {
                if page.is_some() {
                    anyhow::bail!("page is only supported for PDF attachments");
                }
                read_image(&self.ocr_runtime, kind, &path, &relative_path, budget)
            }
        }
    }
}

fn read_pdf(
    _ocr_runtime: &crate::ocr::OcrRuntime,
    path: &camino::Utf8Path,
    relative_path: &str,
    page: Option<u32>,
    budget: usize,
) -> anyhow::Result<ReadAttachmentResult> {
    let selected = match page {
        Some(page) if page == 0 => anyhow::bail!("page must be greater than or equal to 1"),
        Some(page) => {
            let metadata = pdf_inspector::detect_pdf(path.as_str())
                .map_err(|error| anyhow::anyhow!("could not inspect PDF: {error}"))?;
            if page > metadata.page_count {
                anyhow::bail!("PDF page {page} does not exist");
            }
            Some(vec![page - 1])
        }
        None => None,
    };
    let pages = pdf_inspector::extract_pages_markdown(path.as_str(), selected.as_deref())
        .map_err(|error| anyhow::anyhow!("could not read PDF: {error}"))?;
    let mut text_pages = Vec::new();
    let mut ocr_required_pages = Vec::new();
    for extracted in pages.pages {
        let page_number = extracted.page + 1;
        if extracted.needs_ocr {
            ocr_required_pages.push(page_number);
        } else if !extracted.markdown.trim().is_empty() {
            text_pages.push(format!(
                "<!-- Page {page_number} -->\n{}",
                extracted.markdown
            ));
        }
    }
    let source = page
        .map(|page| format!("{relative_path}#page={page}"))
        .unwrap_or_else(|| relative_path.to_string());
    #[cfg(feature = "ocr-local")]
    if !ocr_required_pages.is_empty() {
        match read_pdf_with_local_ocr(_ocr_runtime, path, page) {
            Ok((content, extraction, hosted_recommended)) => {
                let warnings = hosted_recommended
                    .iter()
                    .map(|page| {
                        format!(
                            "local OCR could not establish sufficient fidelity for PDF page {page}"
                        )
                    })
                    .collect();
                return Ok(with_budget(
                    source,
                    AttachmentKind::Pdf,
                    extraction,
                    content,
                    budget,
                    hosted_recommended,
                    warnings,
                    Vec::new(),
                ));
            }
            Err(error) => {
                return Ok(with_budget(
                    source,
                    AttachmentKind::Pdf,
                    "native_text",
                    text_pages.join("\n\n"),
                    budget,
                    ocr_required_pages,
                    vec![format!("local OCR could not run: {error}")],
                    Vec::new(),
                ));
            }
        }
    }
    let warnings = (!ocr_required_pages.is_empty()).then(|| {
        "some PDF pages have no trustworthy native text; rebuild with --features ocr-local and configure the local OCR runtime".to_string()
    }).into_iter().collect();
    Ok(with_budget(
        source,
        AttachmentKind::Pdf,
        "native_text",
        text_pages.join("\n\n"),
        budget,
        ocr_required_pages,
        warnings,
        Vec::new(),
    ))
}

#[cfg(feature = "ocr-local")]
fn read_pdf_with_local_ocr(
    ocr_runtime: &crate::ocr::OcrRuntime,
    path: &camino::Utf8Path,
    page: Option<u32>,
) -> anyhow::Result<(String, &'static str, Vec<u32>)> {
    let result =
        pdf_inspector::vision::process_pdf_with_ocr(path.as_str(), ocr_runtime.pdf_options(page))
            .map_err(|error| anyhow::anyhow!("{error}"))?;
    let extraction = if result.pages_routed_to_ocr.is_empty() {
        "native_text"
    } else {
        "ocr"
    };
    Ok((
        result.markdown,
        extraction,
        result.pages_recommending_hosted,
    ))
}

struct DocxRead {
    content: String,
    warnings: Vec<String>,
    ocr_sources: Vec<AttachmentOcrSource>,
}

fn read_docx(
    path: &camino::Utf8Path,
    include_embedded_images: bool,
    ocr_runtime: &crate::ocr::OcrRuntime,
) -> anyhow::Result<DocxRead> {
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).context("could not open DOCX package")?;
    let xml = {
        let mut document = archive
            .by_name("word/document.xml")
            .context("DOCX package does not contain word/document.xml")?;
        let mut xml = String::new();
        document
            .read_to_string(&mut xml)
            .context("could not read DOCX body XML")?;
        xml
    };

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut paragraphs = Vec::new();
    let mut paragraph = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "p" => paragraph.clear(),
                "t" => in_text = true,
                "tab" => paragraph.push('\t'),
                "br" | "cr" => paragraph.push('\n'),
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "p" => {
                    if !paragraph.trim().is_empty() {
                        paragraphs.push(std::mem::take(&mut paragraph));
                    }
                }
                _ => {}
            },
            Ok(Event::Text(event)) if in_text => paragraph.push_str(&event.xml10_content()),
            Ok(Event::Eof) => break,
            Err(error) => return Err(anyhow::anyhow!("could not parse DOCX body XML: {error}")),
            _ => {}
        }
    }
    let mut result = DocxRead {
        content: paragraphs.join("\n\n"),
        warnings: Vec::new(),
        ocr_sources: Vec::new(),
    };
    if include_embedded_images {
        extract_docx_image_ocr(&mut archive, ocr_runtime, &mut result)?;
    }
    Ok(result)
}

#[cfg(not(feature = "ocr-local"))]
fn extract_docx_image_ocr(
    archive: &mut ZipArchive<fs::File>,
    _: &crate::ocr::OcrRuntime,
    result: &mut DocxRead,
) -> anyhow::Result<()> {
    if archive
        .file_names()
        .any(|name| name.starts_with("word/media/"))
    {
        result
            .warnings
            .push("embedded DOCX image OCR requires the optional ocr-local feature".to_string());
    }
    Ok(())
}

#[cfg(feature = "ocr-local")]
fn extract_docx_image_ocr(
    archive: &mut ZipArchive<fs::File>,
    ocr_runtime: &crate::ocr::OcrRuntime,
    result: &mut DocxRead,
) -> anyhow::Result<()> {
    use image::load_from_memory;

    let mut names = archive
        .file_names()
        .filter(|name| name.starts_with("word/media/"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort();
    for name in names {
        let extension = camino::Utf8Path::new(&name)
            .extension()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
            result.warnings.push(format!(
                "embedded image {name} is not a supported OCR format"
            ));
            continue;
        }
        let mut file = archive.by_name(&name)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        drop(file);
        let image = match load_from_memory(&bytes) {
            Ok(image) => image.into_rgb8(),
            Err(error) => {
                result
                    .warnings
                    .push(format!("could not decode embedded image {name}: {error}"));
                continue;
            }
        };
        let (width, height) = image.dimensions();
        match ocr_runtime.recognize_rgb(width, height, image.into_raw()) {
            Ok(page) => {
                let content = page
                    .spans
                    .into_iter()
                    .map(|span| span.text)
                    .collect::<Vec<_>>()
                    .join("\n");
                if !content.is_empty() {
                    if !result.content.is_empty() {
                        result.content.push_str("\n\n");
                    }
                    result
                        .content
                        .push_str(&format!("<!-- Embedded image {name} -->\n{content}"));
                }
                result.ocr_sources.push(AttachmentOcrSource {
                    source: name,
                    mean_confidence: page.mean_confidence,
                });
                result.warnings.extend(page.warnings);
            }
            Err(error) => result
                .warnings
                .push(format!("could not OCR embedded image {name}: {error}")),
        }
    }
    Ok(())
}

fn with_budget(
    source: String,
    kind: AttachmentKind,
    extraction: &str,
    content: String,
    budget: usize,
    ocr_required_pages: Vec<u32>,
    warnings: Vec<String>,
    ocr_sources: Vec<AttachmentOcrSource>,
) -> ReadAttachmentResult {
    let truncated = content.chars().count() > budget;
    let content = if truncated {
        content.chars().take(budget).collect()
    } else {
        content
    };
    let status = if ocr_required_pages.is_empty() {
        "complete"
    } else if content.is_empty() {
        "ocr_required"
    } else {
        "partial"
    };
    ReadAttachmentResult {
        source,
        format: kind.format().to_string(),
        status: status.to_string(),
        extraction: extraction.to_string(),
        content,
        ocr_required_pages: (!ocr_required_pages.is_empty()).then_some(ocr_required_pages),
        warnings: (!warnings.is_empty()).then_some(warnings),
        ocr_sources: (!ocr_sources.is_empty()).then_some(ocr_sources),
        truncated,
    }
}

#[cfg(not(feature = "ocr-local"))]
fn read_image(
    _: &crate::ocr::OcrRuntime,
    kind: AttachmentKind,
    _: &camino::Utf8Path,
    relative_path: &str,
    _: usize,
) -> anyhow::Result<ReadAttachmentResult> {
    Ok(with_budget(
        relative_path.to_string(),
        kind,
        "ocr",
        String::new(),
        0,
        vec![1],
        vec!["image reading requires the optional ocr-local feature".to_string()],
        Vec::new(),
    ))
}

#[cfg(feature = "ocr-local")]
fn read_image(
    ocr_runtime: &crate::ocr::OcrRuntime,
    kind: AttachmentKind,
    path: &camino::Utf8Path,
    relative_path: &str,
    budget: usize,
) -> anyhow::Result<ReadAttachmentResult> {
    use image::ImageReader;
    let image = ImageReader::open(path)?.decode()?.into_rgb8();
    let (width, height) = image.dimensions();
    let page = ocr_runtime.recognize_rgb(width, height, image.into_raw())?;
    let content = page
        .spans
        .into_iter()
        .map(|span| span.text)
        .collect::<Vec<_>>()
        .join("\n");
    let mut result = with_budget(
        relative_path.to_string(),
        kind,
        "ocr",
        content,
        budget,
        Vec::new(),
        page.warnings,
        Vec::new(),
    );
    if let Some(confidence) = page.mean_confidence {
        result
            .warnings
            .get_or_insert_default()
            .push(format!("OCR mean confidence: {confidence:.3}"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, fs, io::Write, sync::Arc};

    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::vault::{Vault, VaultConfig};

    fn queries() -> (tempfile::TempDir, VaultQueries) {
        let directory = tempdir().expect("tempdir");
        let root =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("utf8 root");
        let vault = Arc::new(Vault::open(&root, VaultConfig::default()).expect("vault"));
        (directory, VaultQueries::new(vault))
    }

    #[test]
    fn reads_docx_body_text_and_truncates_by_unicode_characters() {
        let (directory, queries) = queries();
        let path = directory.path().join("letter.docx");
        let file = fs::File::create(path).expect("create docx");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("word/document.xml", SimpleFileOptions::default())
            .expect("start XML");
        writer
            .write_all("<w:document xmlns:w=\"urn:test\"><w:body><w:p><w:r><w:t>你好 world</w:t></w:r></w:p><w:p><w:r><w:t>second</w:t></w:r></w:p></w:body></w:document>".as_bytes())
            .expect("write XML");
        writer.finish().expect("finish docx");

        let result = queries
            .read_attachment("letter.docx", None, Some(3), false)
            .expect("read DOCX");
        assert_eq!(result.format, "docx");
        assert_eq!(result.status, "complete");
        assert_eq!(result.content, "你好 ");
        assert!(result.truncated);
    }

    #[test]
    fn reads_native_pdf_text_and_validates_one_based_pages() {
        let (directory, queries) = queries();
        write_native_text_pdf(&directory.path().join("letter.pdf"));

        let result = queries
            .read_attachment("letter.pdf", Some(1), None, false)
            .expect("read PDF");
        assert_eq!(result.source, "letter.pdf#page=1");
        assert_eq!(result.format, "pdf");
        assert_eq!(result.status, "complete");
        assert!(result.content.contains("Hello PDF"));
        assert!(
            queries
                .read_attachment("letter.pdf", Some(2), None, false)
                .is_err()
        );
    }

    #[cfg(not(feature = "ocr-local"))]
    #[test]
    fn docx_embedded_images_report_that_ocr_is_required() {
        let (directory, queries) = queries();
        let path = directory.path().join("scanned.docx");
        let file = fs::File::create(path).expect("create docx");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("word/document.xml", SimpleFileOptions::default())
            .expect("start XML");
        writer
            .write_all(b"<w:document xmlns:w=\"urn:test\"><w:body/></w:document>")
            .expect("write XML");
        writer
            .start_file("word/media/scan.png", SimpleFileOptions::default())
            .expect("start image");
        writer
            .write_all(b"not decoded without OCR")
            .expect("write image");
        writer.finish().expect("finish docx");

        let result = queries
            .read_attachment("scanned.docx", None, None, true)
            .expect("inspect DOCX");
        assert_eq!(result.status, "complete");
        assert_eq!(
            result.warnings,
            Some(vec![
                "embedded DOCX image OCR requires the optional ocr-local feature".to_string()
            ])
        );
    }

    #[cfg(not(feature = "ocr-local"))]
    #[test]
    fn png_reports_that_local_ocr_is_required_when_feature_is_disabled() {
        let (directory, queries) = queries();
        fs::write(
            directory.path().join("scan.png"),
            b"not decoded without OCR",
        )
        .expect("write PNG");
        let result = queries
            .read_attachment("scan.png", None, None, false)
            .expect("inspect PNG");
        assert_eq!(result.status, "ocr_required");
        assert_eq!(result.ocr_required_pages, Some(vec![1]));
    }

    #[cfg(not(feature = "ocr-local"))]
    #[test]
    fn jpeg_reports_that_local_ocr_is_required_when_feature_is_disabled() {
        let (directory, queries) = queries();
        fs::write(
            directory.path().join("scan.jpeg"),
            b"not decoded without OCR",
        )
        .expect("write JPEG");
        let result = queries
            .read_attachment("scan.jpeg", None, None, false)
            .expect("inspect JPEG");
        assert_eq!(result.format, "jpeg");
        assert_eq!(result.status, "ocr_required");
        assert_eq!(result.ocr_required_pages, Some(vec![1]));
    }

    #[cfg(not(feature = "ocr-local"))]
    #[test]
    fn fixture_png_reports_a_stable_ocr_requirement() {
        let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let queries = VaultQueries::new(Arc::new(
            Vault::open(&root, VaultConfig::default()).expect("vault"),
        ));
        let result = queries
            .read_attachment(
                "tests/fixtures/截屏2026-09-03 21.11.11.png",
                None,
                None,
                false,
            )
            .expect("inspect PNG fixture");
        assert_eq!(result.format, "png");
        assert_eq!(result.status, "ocr_required");
        assert_eq!(result.ocr_required_pages, Some(vec![1]));
    }

    #[test]
    fn attachment_paths_reject_missing_extensions_and_vault_escape() {
        let (_directory, queries) = queries();
        assert!(queries.read_attachment("scan", None, None, false).is_err());
        assert!(
            queries
                .read_attachment("../scan.png", None, None, false)
                .is_err()
        );
    }

    #[test]
    fn fixture_readers_report_real_document_content() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for (filename, expected_text) in [
            ("川渝劳动人事争议典型案例目录.docx", "劳动者违反保密义务"),
            (
                "CL0fjWr9NqJmw6OUKz3rXwBy4k1A1OQvEMBqJG81.docx",
                "四川天府新区审计中心",
            ),
        ] {
            let content = read_docx(
                &camino::Utf8PathBuf::from_path_buf(fixtures.join(filename))
                    .expect("UTF-8 fixture path"),
                false,
                &crate::ocr::OcrRuntime::new(),
            )
            .expect("read DOCX fixture");
            assert!(
                content.content.contains(expected_text),
                "{filename} body text"
            );
        }
    }

    #[cfg(not(feature = "ocr-local"))]
    #[test]
    fn fixture_pdfs_report_native_text_or_an_explicit_ocr_boundary() {
        let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut config = VaultConfig::default();
        config.max_note_bytes = 16 * 1024 * 1024;
        let queries = VaultQueries::new(Arc::new(Vault::open(&root, config).expect("vault")));
        let scanned = queries
            .read_attachment(
                "tests/fixtures/CEU-July-2026-CN.pdf",
                Some(1),
                Some(512),
                false,
            )
            .expect("read scanned PDF fixture");
        assert_eq!(scanned.status, "ocr_required");
        assert_eq!(scanned.ocr_required_pages, Some(vec![1]));

        let native = queries
            .read_attachment(
                "tests/fixtures/深入理解-AI-Agent-李博杰-v1.1.pdf",
                Some(1),
                Some(512),
                false,
            )
            .expect("read native PDF fixture");
        assert_eq!(native.status, "complete");
        assert!(native.content.contains("深入理解 AI Agent"));
    }

    fn write_native_text_pdf(path: &std::path::Path) {
        let stream = "BT /F1 12 Tf 72 72 Td (Hello PDF) Tj ET\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ];
        let mut pdf = "%PDF-1.4\n".to_string();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            let _ = write!(pdf, "{} 0 obj\n{object}\nendobj\n", index + 1);
        }
        let xref = pdf.len();
        let _ = write!(pdf, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets.iter().skip(1) {
            let _ = writeln!(pdf, "{offset:010} 00000 n ");
        }
        let _ = write!(
            pdf,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        );
        fs::write(path, pdf).expect("write PDF");
    }
}
