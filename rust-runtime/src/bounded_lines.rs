//! Cancellation-safe, bounded tool output. Oversized lines are fully drained
//! and omitted rather than exposing a truncated credential or parsing a prefix.
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

pub(crate) const TOOL_LINE_LIMIT: usize = 32 * 1024;
const OMITTED: &str = "[OUTPUT:WARN] oversized tool output line omitted";

pub(crate) struct BoundedLines<R> {
    reader: BufReader<R>,
    line: Vec<u8>,
    overflow: bool,
}

impl<R: AsyncRead + Unpin> BoundedLines<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            line: Vec::new(),
            overflow: false,
        }
    }

    #[cfg(test)]
    pub(crate) async fn next_line(&mut self) -> io::Result<Option<String>> {
        self.next_line_inner(false).await
    }

    /// Reads the next bounded line while replacing invalid UTF-8 bytes (e.g. Windows
    /// CP949 console output from external media tools) instead of aborting the pipe.
    pub(crate) async fn next_line_lossy(&mut self) -> io::Result<Option<String>> {
        self.next_line_inner(true).await
    }

    async fn next_line_inner(&mut self, lossy: bool) -> io::Result<Option<String>> {
        loop {
            let bytes = self.reader.fill_buf().await?;
            if bytes.is_empty() {
                if self.line.is_empty() && !self.overflow {
                    return Ok(None);
                }
                return self.finish(lossy).map(Some);
            }
            let newline = bytes.iter().position(|b| *b == b'\n');
            let count = newline.map_or(bytes.len(), |i| i + 1);
            if !self.overflow {
                if count > TOOL_LINE_LIMIT.saturating_sub(self.line.len()) {
                    self.overflow = true;
                    self.line.clear();
                } else {
                    self.line.extend_from_slice(&bytes[..count]);
                }
            }
            self.reader.consume(count);
            if newline.is_some() {
                return self.finish(lossy).map(Some);
            }
        }
    }

    fn finish(&mut self, lossy: bool) -> io::Result<String> {
        if std::mem::take(&mut self.overflow) {
            return Ok(OMITTED.into());
        }
        if self.line.last() == Some(&b'\n') {
            self.line.pop();
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
        }
        let bytes = std::mem::take(&mut self.line);
        if lossy {
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn huge_line_is_omitted_and_next_progress_and_unicode_survive() {
        let input = format!(
            "{}secret_token\nKICK_DONE 1\r\n한글\nlast",
            "x".repeat(4 * 1024 * 1024)
        );
        let mut lines = BoundedLines::new(input.as_bytes());
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some(OMITTED));
        assert_eq!(
            lines.next_line().await.unwrap().as_deref(),
            Some("KICK_DONE 1")
        );
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("한글"));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("last"));
        assert!(lines.next_line().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn cancelled_read_retains_partial_line_and_overflow_state() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let mut lines = BoundedLines::new(reader);
        writer.write_all(b"partial").await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), lines.next_line())
                .await
                .is_err()
        );
        writer.write_all(b"-complete\n").await.unwrap();
        assert_eq!(
            lines.next_line().await.unwrap().as_deref(),
            Some("partial-complete")
        );
        lines.line = vec![b'x'; TOOL_LINE_LIMIT];
        writer.write_all(b"overflow").await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), lines.next_line())
                .await
                .is_err()
        );
        assert!(lines.overflow);
        assert!(lines.line.is_empty());
        writer.write_all(b"\nnext\n").await.unwrap();
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some(OMITTED));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("next"));
    }

    #[tokio::test]
    async fn invalid_utf8_and_line_boundary_do_not_change_normal_lines_semantics() {
        let bytes = vec![b'x'; TOOL_LINE_LIMIT - 1];
        let mut input = bytes.clone();
        input.push(b'\n');
        input.extend_from_slice(&[0xff, b'\n']);
        let mut lines = BoundedLines::new(input.as_slice());
        assert_eq!(lines.next_line().await.unwrap().unwrap().len(), bytes.len());
        assert_eq!(
            lines.next_line().await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let mut lossy = BoundedLines::new(&b"ok\n\xb0\xa1\r\nafter\n"[..]);
        assert_eq!(
            lossy.next_line_lossy().await.unwrap().as_deref(),
            Some("ok")
        );
        assert!(lossy.next_line_lossy().await.unwrap().is_some());
        assert_eq!(
            lossy.next_line_lossy().await.unwrap().as_deref(),
            Some("after")
        );
    }
}
