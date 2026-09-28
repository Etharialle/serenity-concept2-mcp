//! Byte limits around the SDK's newline-delimited stdio transport.
//!
//! This adapter leaves JSON and MCP parsing to `rmcp`. It prevents an unfinished
//! or oversized line from growing the SDK's input buffer without limit.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, ReadBuf};

/// Maximum bytes before a frame's terminating LF; an optional CR counts.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
const READ_CHUNK_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy)]
enum InputFailure {
    Oversized,
    Incomplete,
    Read(io::ErrorKind),
}

impl InputFailure {
    fn error(self) -> io::Error {
        match self {
            Self::Oversized => io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP input frame exceeded the 64 KiB limit",
            ),
            Self::Incomplete => io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "MCP input ended before a frame's newline delimiter",
            ),
            Self::Read(kind) => io::Error::new(kind, "MCP input could not be read"),
        }
    }
}

/// Limits every incoming line while preserving the SDK's framing behavior.
///
/// A framing failure is terminal. Neither the failure nor Debug output contains
/// the incoming payload, and dropping a pending read leaves the byte count intact.
pub struct BoundedInput<R> {
    inner: R,
    current_frame_bytes: usize,
    failure: Option<InputFailure>,
}

impl<R> BoundedInput<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            current_frame_bytes: 0,
            failure: None,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BoundedInput<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        destination: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if destination.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if let Some(failure) = this.failure {
            return Poll::Ready(Err(failure.error()));
        }

        // Validate before exposing bytes to the SDK. A fixed buffer bounds the
        // amount read ahead even when the consumer offers a large destination.
        let mut bytes = [0_u8; READ_CHUNK_BYTES];
        let capacity = destination.remaining().min(bytes.len());
        let mut incoming = ReadBuf::new(&mut bytes[..capacity]);
        match Pin::new(&mut this.inner).poll_read(context, &mut incoming) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => {
                let failure = InputFailure::Read(error.kind());
                this.failure = Some(failure);
                return Poll::Ready(Err(failure.error()));
            }
            Poll::Ready(Ok(())) => {}
        }

        if incoming.filled().is_empty() && this.current_frame_bytes != 0 {
            let failure = InputFailure::Incomplete;
            this.failure = Some(failure);
            return Poll::Ready(Err(failure.error()));
        }
        for byte in incoming.filled() {
            if *byte == b'\n' {
                this.current_frame_bytes = 0;
            } else if this.current_frame_bytes == MAX_FRAME_BYTES {
                let failure = InputFailure::Oversized;
                this.failure = Some(failure);
                return Poll::Ready(Err(failure.error()));
            } else {
                this.current_frame_bytes += 1;
            }
        }
        destination.put_slice(incoming.filled());
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct SplitReader {
        bytes: Vec<u8>,
        position: usize,
        chunk_size: usize,
    }

    impl SplitReader {
        fn new(bytes: Vec<u8>, chunk_size: usize) -> Self {
            Self {
                bytes,
                position: 0,
                chunk_size,
            }
        }
    }

    impl AsyncRead for SplitReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            destination: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let this = self.get_mut();
            let count = destination
                .remaining()
                .min(this.chunk_size)
                .min(this.bytes.len() - this.position);
            destination.put_slice(&this.bytes[this.position..this.position + count]);
            this.position += count;
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn preserves_multiple_frames_across_split_reads() {
        let input = b"{\"first\":1}\n{\"second\":2}\r\n\n".to_vec();
        for chunk_size in [1, 3, 8, 1024] {
            let mut bounded = BoundedInput::new(SplitReader::new(input.clone(), chunk_size));
            let mut output = Vec::new();
            bounded.read_to_end(&mut output).await.unwrap();
            assert_eq!(output, input);
        }
    }

    #[tokio::test]
    async fn accepts_exact_limit_and_resets_at_each_newline() {
        let mut input = vec![b'a'; MAX_FRAME_BYTES];
        input.push(b'\n');
        input.extend(std::iter::repeat_n(b'b', MAX_FRAME_BYTES));
        input.push(b'\n');
        let mut bounded = BoundedInput::new(SplitReader::new(input.clone(), 13));
        let mut output = Vec::new();
        bounded.read_to_end(&mut output).await.unwrap();
        assert_eq!(output, input);
    }

    #[tokio::test]
    async fn rejects_one_byte_over_limit_across_reads_with_a_sticky_sanitized_error() {
        let mut input = vec![b'x'; MAX_FRAME_BYTES + 1];
        input.push(b'\n');
        input.extend_from_slice(b"valid-next-frame\n");
        let mut bounded = BoundedInput::new(SplitReader::new(input, 17));
        let mut output = Vec::new();
        let error = bounded.read_to_end(&mut output).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.len() <= MAX_FRAME_BYTES);
        assert!(!error.to_string().contains("xxxx"));
        let subsequent = bounded.read(&mut [0_u8; 10]).await.unwrap_err();
        assert_eq!(subsequent.to_string(), error.to_string());
    }

    #[tokio::test]
    async fn rejects_eof_inside_a_frame_without_echoing_it() {
        let input = b"complete\nPRIVATE_INCOMPLETE_INPUT".to_vec();
        let mut bounded = BoundedInput::new(SplitReader::new(input, 2));
        let error = bounded.read_to_end(&mut Vec::new()).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert!(!error.to_string().contains("PRIVATE"));
    }

    #[tokio::test]
    async fn accepts_empty_input_and_does_not_consume_for_an_empty_buffer() {
        let mut empty = BoundedInput::new(std::io::Cursor::new(Vec::<u8>::new()));
        assert_eq!(empty.read_to_end(&mut Vec::new()).await.unwrap(), 0);

        let input = b"frame\n".to_vec();
        let mut bounded = BoundedInput::new(std::io::Cursor::new(input.clone()));
        assert_eq!(bounded.read(&mut []).await.unwrap(), 0);
        let mut output = Vec::new();
        bounded.read_to_end(&mut output).await.unwrap();
        assert_eq!(output, input);
    }

    #[tokio::test]
    async fn counts_an_optional_carriage_return_toward_the_limit() {
        let mut input = vec![b'a'; MAX_FRAME_BYTES];
        input.extend_from_slice(b"\r\n");
        let mut bounded = BoundedInput::new(std::io::Cursor::new(input));
        assert_eq!(
            bounded
                .read_to_end(&mut Vec::new())
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn cancellation_keeps_the_partial_frame_count() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let mut bounded = BoundedInput::new(reader);
        writer.write_all(b"first").await.unwrap();
        let mut output = Vec::new();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                bounded.read_to_end(&mut output),
            )
            .await
            .is_err()
        );
        assert_eq!(output, b"first");
        assert_eq!(bounded.current_frame_bytes, 5);
        writer.write_all(b"\nnext\n").await.unwrap();
        drop(writer);
        bounded.read_to_end(&mut output).await.unwrap();
        assert_eq!(output, b"first\nnext\n");
    }

    #[tokio::test]
    async fn inner_io_errors_are_sanitized() {
        struct FailedReader;
        impl AsyncRead for FailedReader {
            fn poll_read(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
                _: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
                Poll::Ready(Err(io::Error::other("PRIVATE_READER_ERROR")))
            }
        }
        let mut bounded = BoundedInput::new(FailedReader);
        let error = bounded.read(&mut [0_u8; 1]).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(!error.to_string().contains("PRIVATE"));
    }
}
