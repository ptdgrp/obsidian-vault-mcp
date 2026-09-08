# Ubiquitous Language

## Attachment

A non-Markdown file below the project root that the server may read and expose
to an LLM. Attachments are read-only: they are never edited, converted back to
disk, or indexed for semantic retrieval by this project.

## OCR feature

An opt-in Cargo feature that adds the local OCR runtime and model provisioning
needed to read scanned PDFs and image attachments. Without it, attachment
inspection may report that OCR is required, but does not attempt recognition.

## Export template

A named, content-neutral publication style applied when exporting Markdown.
Each template has format-specific adapters for HTML, DOCX, and PDF while
preserving the same semantic intent. Built-in templates are Default, Legal,
Official Document, and Research Paper.
