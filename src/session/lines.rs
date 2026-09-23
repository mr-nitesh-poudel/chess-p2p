//! Reading a peer's lines without letting the peer decide how much memory we
//! spend on them.
//!
//! tokio's own `Lines` keeps reading until it finds a newline, however far
//! away that is. Anyone who can dial us — and in the lobby that is anyone who
//! knows our endpoint id, before they have proved anything — could send one
//! endless line and grow our memory until the process dies. This reads at most
//! [`MAX_LINE`] bytes looking for the newline, and gives up past that.

use std::io;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

/// The longest line either side ever has reason to send. The handshake's
/// longest is a SPAKE2 message in hex, well under 200 bytes, and a game's
/// messages are shorter still. This leaves plenty of room for new games
/// without leaving room for abuse.
pub const MAX_LINE: usize = 4096;

/// Newline-delimited text from a stream, one line at a time.
pub struct Lines<R> {
    inner: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> Lines<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner: BufReader::new(inner),
            buf: Vec::new(),
        }
    }

    /// The next line, without its `\n` or `\r\n`, or `None` once the stream
    /// ends. A line that runs past [`MAX_LINE`], or is not UTF-8, is an error:
    /// there is no sensible way to carry on reading from someone sending
    /// either.
    pub async fn next_line(&mut self) -> io::Result<Option<String>> {
        self.buf.clear();
        // One byte over the limit, so a line of exactly MAX_LINE bytes still
        // has room for its newline.
        let limit = MAX_LINE as u64 + 1;
        let read = (&mut self.inner)
            .take(limit)
            .read_until(b'\n', &mut self.buf)
            .await?;
        if read == 0 {
            return Ok(None);
        }
        if self.buf.last() == Some(&b'\n') {
            self.buf.pop();
            if self.buf.last() == Some(&b'\r') {
                self.buf.pop();
            }
        } else if read as u64 == limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("peer sent a line over {MAX_LINE} bytes"),
            ));
        }
        // Otherwise the stream ended partway through a line; hand back what
        // there was, as tokio's `Lines` does.
        String::from_utf8(std::mem::take(&mut self.buf))
            .map(Some)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "peer sent a line that is not UTF-8",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn all(input: &[u8]) -> Vec<io::Result<Option<String>>> {
        let mut lines = Lines::new(input);
        let mut out = Vec::new();
        loop {
            let next = lines.next_line().await;
            let done = !matches!(next, Ok(Some(_)));
            out.push(next);
            if done {
                return out;
            }
        }
    }

    fn texts(results: Vec<io::Result<Option<String>>>) -> Vec<String> {
        results
            .into_iter()
            .filter_map(|r| r.ok().flatten())
            .collect()
    }

    #[tokio::test]
    async fn reads_lines_and_strips_their_endings() {
        let got = texts(all(b"move e2e4\r\nresign\nlast without newline").await);
        assert_eq!(got, ["move e2e4", "resign", "last without newline"]);
    }

    #[tokio::test]
    async fn a_line_of_exactly_the_limit_is_fine() {
        let mut input = vec![b'x'; MAX_LINE];
        input.push(b'\n');
        let got = texts(all(&input).await);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len(), MAX_LINE);
    }

    #[tokio::test]
    async fn an_endless_line_is_refused_rather_than_buffered() {
        // Ten megabytes and no newline: without the limit this would all be
        // read into memory before anything noticed.
        let input = vec![b'x'; 10 * 1024 * 1024];
        let mut lines = Lines::new(&input[..]);
        let err = lines.next_line().await.expect_err("should refuse");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            lines.buf.capacity() <= 2 * (MAX_LINE + 1),
            "held {} bytes",
            lines.buf.capacity()
        );
    }

    #[tokio::test]
    async fn bytes_that_are_not_text_are_refused() {
        let err = Lines::new(&b"\xff\xfe\n"[..])
            .next_line()
            .await
            .expect_err("not UTF-8");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
