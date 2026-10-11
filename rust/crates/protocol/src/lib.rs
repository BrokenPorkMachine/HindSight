//! Bounded frame transport for worker replies.
//!
//! Control/page/capture traffic stays deliberately small. Full static-analysis
//! reports are a separate bounded bulk class because the modern report contains
//! dozens of independently bounded evidence sections.
use std::io::{self, Read, Write};

pub const MAX_CONTROL_FRAME: usize = 256 * 1024;
pub const MAX_ANALYSIS_FRAME: usize = 8 * 1024 * 1024;
// Backward-compatible name for callers/tests that mean a normal control frame.
pub const MAX_FRAME: usize = MAX_CONTROL_FRAME;

pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    read_frame_with_limit(reader, MAX_CONTROL_FRAME)
}

pub fn read_frame_with_limit(reader: &mut impl Read, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    loop {
        match reader.read(&mut length[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut length[1..])?;
    let count = u32::from_be_bytes(length) as usize;
    if count > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame exceeds limit"));
    }
    let mut body = vec![0; count];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write_frame_with_limit(writer, body, MAX_CONTROL_FRAME)
}

pub fn write_frame_with_limit(writer: &mut impl Write, body: &[u8], limit: usize) -> io::Result<()> {
    if body.len() > limit || body.len() > u32::MAX as usize {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame exceeds limit"));
    }
    writer.write_all(&(body.len() as u32).to_be_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test] fn round_trip_and_clean_eof() {
        let mut data = vec![];
        write_frame(&mut data, b"hello").unwrap();
        let mut cursor = Cursor::new(data);
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(b"hello".to_vec()));
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }
    #[test] fn rejects_oversize_before_body() {
        let mut cursor = Cursor::new(((MAX_CONTROL_FRAME + 1) as u32).to_be_bytes());
        assert_eq!(read_frame(&mut cursor).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
    #[test] fn analysis_frame_is_separate_and_bounded() {
        let body = vec![b'a'; MAX_CONTROL_FRAME + 1];
        assert!(write_frame(&mut Vec::new(), &body).is_err());
        let mut framed = Vec::new();
        write_frame_with_limit(&mut framed, &body, MAX_ANALYSIS_FRAME).unwrap();
        let mut cursor = Cursor::new(framed);
        assert_eq!(read_frame_with_limit(&mut cursor, MAX_ANALYSIS_FRAME).unwrap().unwrap().len(), body.len());
    }
    #[test] fn truncated_header_is_not_clean_eof() {
        assert_eq!(read_frame(&mut Cursor::new([0, 0])).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }
    #[test] fn truncated_body_is_rejected() {
        assert_eq!(read_frame(&mut Cursor::new([0, 0, 0, 2, 1])).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }
}
