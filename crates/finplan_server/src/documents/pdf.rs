//! PDF text layers, page by page. Pure Rust (`lopdf`); no OCR: a page with no
//! text layer comes back empty and the caller decides what to do with a
//! scanned document.

/// A PDF's extracted pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfText {
    /// Text of each page, in order (empty for a page with no text layer).
    pub pages: Vec<String>,
    /// Pages whose content could not be read at all.
    pub unreadable_pages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// Not a PDF, or damaged past reading.
    Unreadable,
    /// Password-protected.
    Encrypted,
}

/// Decompressed page content ceiling, against decompression bombs.
const MAX_PAGE_CONTENT: usize = 32 * 1024 * 1024;

/// Whether `head` starts like a PDF.
pub fn sniff(head: &[u8]) -> bool {
    head.starts_with(b"%PDF-")
        || head
            .iter()
            .take(1024)
            .collect::<Vec<_>>()
            .windows(5)
            .any(|w| w.iter().map(|b| **b).eq(*b"%PDF-"))
}

/// Extract each page's text. Parsing untrusted PDFs can panic in dependencies,
/// so a panic is an unreadable file, not a crashed request.
pub fn extract(bytes: &[u8]) -> Result<PdfText, PdfError> {
    let bytes = bytes.to_vec();
    std::panic::catch_unwind(move || extract_inner(&bytes)).unwrap_or(Err(PdfError::Unreadable))
}

fn extract_inner(bytes: &[u8]) -> Result<PdfText, PdfError> {
    let doc = lopdf::Document::load_mem(bytes).map_err(|_| PdfError::Unreadable)?;
    if doc.is_encrypted() {
        return Err(PdfError::Encrypted);
    }
    let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    if numbers.is_empty() {
        return Err(PdfError::Unreadable);
    }
    let mut unreadable_pages = 0;
    let pages = numbers
        .iter()
        .map(
            |n| match doc.extract_text_with_limit(&[*n], MAX_PAGE_CONTENT) {
                Ok(text) => tidy(&text),
                Err(_) => {
                    unreadable_pages += 1;
                    String::new()
                }
            },
        )
        .collect();
    Ok(PdfText {
        pages,
        unreadable_pages,
    })
}

/// Trim trailing spaces from lines and collapse runs of blank lines.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.replace('\r', "\n").lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_owned()
}
