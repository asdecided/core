//! Binary segment codec for the persistent index store (ADR-104).
//!
//! Byte-for-byte port of `services/index_format.py` per
//! `rust/spec/index-store-format.md` §2. Fixed struct reads over a byte
//! buffer — no code-bearing deserialisation; every read is bounds-checked
//! and a segment's declared payload length must match its file exactly, so
//! truncation or trailing garbage fails closed on open (a cache miss).

use std::fmt;

/// 8 magic bytes opening every segment file.
pub const SEGMENT_MAGIC: &[u8; 8] = b"RACIDX01";
/// The binary layout version (v4: tags tier, ADR-109).
pub const SEGMENT_FORMAT_VERSION: u16 = 4;

const HEADER_SIZE: usize = 8 + 2 + 8; // magic | version u16 LE | payload_len u64 LE

/// A segment is corrupt, truncated, wrong-magic, or wrong-version. The store
/// treats this as a cache miss and rebuilds; it never escapes to a caller.
#[derive(Debug)]
pub struct IndexFormatError(pub String);

impl fmt::Display for IndexFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for IndexFormatError {}

fn err<T>(message: impl Into<String>) -> Result<T, IndexFormatError> {
    Err(IndexFormatError(message.into()))
}

// ---------------------------------------------------------------------------
// Writer — append-only encoder building one segment payload in memory.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u32(&mut self, value: u64) -> Result<(), IndexFormatError> {
        if value > u64::from(u32::MAX) {
            return err(format!("u32 out of range: {value}"));
        }
        self.buf.extend_from_slice(&(value as u32).to_le_bytes());
        Ok(())
    }

    pub fn u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn raw(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn blob(&mut self, data: &[u8]) -> Result<(), IndexFormatError> {
        self.u32(data.len() as u64)?;
        self.buf.extend_from_slice(data);
        Ok(())
    }

    pub fn text(&mut self, value: &str) -> Result<(), IndexFormatError> {
        self.blob(value.as_bytes())
    }

    /// A flag byte distinguishes `None` from the empty string.
    pub fn opt_text(&mut self, value: Option<&str>) -> Result<(), IndexFormatError> {
        match value {
            None => {
                self.buf.push(0);
                Ok(())
            }
            Some(v) => {
                self.buf.push(1);
                self.text(v)
            }
        }
    }

    pub fn text_list<S: AsRef<str>>(&mut self, values: &[S]) -> Result<(), IndexFormatError> {
        self.u32(values.len() as u64)?;
        for value in values {
            self.text(value.as_ref())?;
        }
        Ok(())
    }

    pub fn u32_list(&mut self, values: &[u32]) -> Result<(), IndexFormatError> {
        self.u32(values.len() as u64)?;
        for &value in values {
            self.u32(u64::from(value))?;
        }
        Ok(())
    }

    pub fn payload(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------------------
// Reader — bounds-checked decoder over a mapped segment payload.
// ---------------------------------------------------------------------------

pub struct Reader<'a> {
    view: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(view: &'a [u8]) -> Self {
        Self { view, pos: 0 }
    }

    pub fn at(view: &'a [u8], offset: usize) -> Self {
        Self { view, pos: offset }
    }

    fn require(&mut self, count: usize) -> Result<usize, IndexFormatError> {
        let end = self.pos.checked_add(count);
        match end {
            Some(end) if end <= self.view.len() => {
                let start = self.pos;
                self.pos = end;
                Ok(start)
            }
            _ => err("segment read past end (truncated or corrupt)"),
        }
    }

    pub fn u32(&mut self) -> Result<u32, IndexFormatError> {
        let start = self.require(4)?;
        Ok(u32::from_le_bytes(
            self.view[start..start + 4].try_into().expect("4 bytes"),
        ))
    }

    pub fn u64(&mut self) -> Result<u64, IndexFormatError> {
        let start = self.require(8)?;
        Ok(u64::from_le_bytes(
            self.view[start..start + 8].try_into().expect("8 bytes"),
        ))
    }

    pub fn blob(&mut self) -> Result<&'a [u8], IndexFormatError> {
        let length = self.u32()? as usize;
        let start = self.require(length)?;
        Ok(&self.view[start..start + length])
    }

    pub fn text(&mut self) -> Result<String, IndexFormatError> {
        Ok(self.text_ref()?.to_string())
    }

    /// Bounds-checked UTF-8 text borrowed directly from the segment. Callers
    /// must keep the value within the mapped reader's lifetime.
    pub fn text_ref(&mut self) -> Result<&'a str, IndexFormatError> {
        let raw = self.blob()?;
        match std::str::from_utf8(raw) {
            Ok(s) => Ok(s),
            Err(_) => err("segment text is not valid UTF-8"),
        }
    }

    pub fn opt_text(&mut self) -> Result<Option<String>, IndexFormatError> {
        let start = self.require(1)?;
        match self.view[start] {
            0 => Ok(None),
            1 => Ok(Some(self.text()?)),
            flag => err(format!("bad optional flag: {flag}")),
        }
    }

    pub fn text_list(&mut self) -> Result<Vec<String>, IndexFormatError> {
        let count = self.u32()?;
        let mut out = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            out.push(self.text()?);
        }
        Ok(out)
    }

    pub fn u32_list(&mut self) -> Result<Vec<u32>, IndexFormatError> {
        let count = self.u32()?;
        let mut out = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            out.push(self.u32()?);
        }
        Ok(out)
    }

    /// Require that a closed-format payload has no unparsed suffix.
    pub fn finish(self) -> Result<(), IndexFormatError> {
        if self.pos == self.view.len() {
            Ok(())
        } else {
            err("segment contains trailing payload bytes")
        }
    }
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Frame a payload as a segment file's bytes: magic, version, length, payload.
pub fn encode_segment(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
    out.extend_from_slice(SEGMENT_MAGIC);
    out.extend_from_slice(&SEGMENT_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Validate a mapped segment and return its payload slice (fail-closed).
pub fn segment_payload(view: &[u8]) -> Result<&[u8], IndexFormatError> {
    if view.len() < HEADER_SIZE {
        return err("segment shorter than its header");
    }
    if &view[..8] != SEGMENT_MAGIC {
        return err("bad segment magic (not an index-store segment)");
    }
    let version = u16::from_le_bytes(view[8..10].try_into().expect("2 bytes"));
    if version != SEGMENT_FORMAT_VERSION {
        return err(format!("unsupported segment format version: {version}"));
    }
    let payload_len = u64::from_le_bytes(view[10..18].try_into().expect("8 bytes"));
    if (view.len() - HEADER_SIZE) as u64 != payload_len {
        return err("segment length mismatch (truncated or corrupt)");
    }
    Ok(&view[HEADER_SIZE..])
}

/// Encode row blobs with a docid-indexed offset table for O(1) point access:
/// `count(u32) | offsets(count × u64, relative to end of table) | rows`.
pub fn write_indexed(rows: &[Vec<u8>]) -> Result<Vec<u8>, IndexFormatError> {
    let mut writer = Writer::new();
    writer.u32(rows.len() as u64)?;
    let mut running: u64 = 0;
    for row in rows {
        writer.u64(running);
        running += row.len() as u64;
    }
    for row in rows {
        writer.raw(row);
    }
    Ok(writer.payload())
}

/// Reader over a `write_indexed` payload — random access by row index.
pub struct IndexedSegment<'a> {
    view: &'a [u8],
    count: u32,
    data_start: usize,
}

impl<'a> IndexedSegment<'a> {
    pub fn new(view: &'a [u8]) -> Result<Self, IndexFormatError> {
        let mut header = Reader::new(view);
        let count = header.u32()?;
        let data_start = 4usize.saturating_add((count as usize).saturating_mul(8));
        if data_start > view.len() {
            return err("indexed-segment offset table truncated");
        }
        Ok(Self {
            view,
            count,
            data_start,
        })
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn row(&self, index: u32) -> Result<Reader<'a>, IndexFormatError> {
        if index >= self.count {
            return err(format!("row index out of range: {index}"));
        }
        let table_at = 4 + 8 * index as usize;
        let offset = u64::from_le_bytes(
            self.view[table_at..table_at + 8].try_into().expect("8 bytes"),
        );
        let start = self.data_start.checked_add(offset as usize);
        match start {
            Some(start) if start <= self.view.len() => Ok(Reader::at(self.view, start)),
            _ => err("indexed-segment row offset past end"),
        }
    }
}
