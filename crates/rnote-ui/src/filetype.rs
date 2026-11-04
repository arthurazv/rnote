// Imports
use gtk4::{gio, prelude::*};
use tracing::warn;

/// File types supported by Rnote.
#[derive(Debug)]
pub(crate) enum FileType {
    Folder,
    RnoteFile,
    VectorImageFile,
    BitmapImageFile,
    XoppFile,
    PdfFile,
    PlaintextFile,
    Unsupported,
}

impl FileType {
    pub(crate) fn lookup_file_type(file: &gio::File) -> Self {
        if let Ok(info) = file.query_info(
            "standard::*",
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        ) {
            match info.file_type() {
                gio::FileType::Regular => {
                    if let Some(content_type) = info.content_type() {
                        let ct = content_type.as_str();
                        if ct == "application/rnote" || ct == "com.github.flxzt.rnote.document" {
                            return Self::RnoteFile;
                        } else if ct == "image/svg+xml" || ct == "public.svg-image" {
                            return Self::VectorImageFile;
                        } else if ct == "application/pdf" || ct == "com.adobe.pdf" {
                            return Self::PdfFile;
                        } else if ct == "application/x-xopp" {
                            return Self::XoppFile;
                        } else if ct.starts_with("image/")
                            || (ct.starts_with("public.") && ct.contains("image"))
                            || ct == "public.png"
                            || ct == "public.jpeg"
                        {
                            return Self::BitmapImageFile;
                        } else if ct == "text/plain"
                            || ct == "public.plain-text"
                            || ct == "public.text"
                        {
                            return Self::PlaintextFile;
                        }
                    }
                }
                gio::FileType::Directory => {
                    return Self::Folder;
                }
                file_type => {
                    warn!("Looking up file type failed, unsupported file type `{file_type:?}`");
                    return Self::Unsupported;
                }
            }
        } else {
            warn!("Looking up file type failed, failed to query FileInfo from file `{file:?}`.");
        }

        // match on file extensions as fallback
        let ext = file
            .path()
            .and_then(|p| p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()))
            .or_else(|| {
                file.basename()
                    .and_then(|b| b.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()))
            })
            .or_else(|| {
                let uri = file.uri();
                let clean_uri = uri.split('?').next().unwrap_or(&uri);
                clean_uri.rsplit('.').next().map(|e| e.to_ascii_lowercase())
            });

        if let Some(extension_str) = ext {
            match extension_str.as_str() {
                "rnote" => {
                    return Self::RnoteFile;
                }
                "svg" => {
                    return Self::VectorImageFile;
                }
                "jpg" | "jpeg" | "png" | "bmp" | "webp" | "tiff" | "tif" => {
                    return Self::BitmapImageFile;
                }
                "xopp" => {
                    return Self::XoppFile;
                }
                "pdf" => {
                    return Self::PdfFile;
                }
                "txt" => {
                    return Self::PlaintextFile;
                }
                _ => {}
            }
        } else {
            warn!("Looking up file type failed, no extension found for file `{file:?}`.");
        }

        Self::Unsupported
    }
}
