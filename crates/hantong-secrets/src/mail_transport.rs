use std::{
    fmt, io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use lettre::transport::smtp::client::AsyncTokioStream;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncRead, AsyncWrite, BufReader, ReadBuf};

pub(super) const MAX_RECEIVED_BYTES: usize = 256 * 1024;

/// Prevent Lettre's outer BufReader from retaining plaintext across STARTTLS.
/// The private lower buffer amortizes OS reads; any prefetched bytes remain
/// UNDER Rustls at upgrade and must be authenticated as TLS records.
pub(super) struct StarttlsReadGate<S> {
    inner: BufReader<BoundedAsyncStream<S>>,
    upgraded: Arc<AtomicBool>,
}
impl<S: AsyncRead + Unpin> StarttlsReadGate<S> {
    pub(super) fn new(inner: S) -> (Self, Arc<AtomicBool>) {
        let upgraded = Arc::new(AtomicBool::new(false));
        (
            Self {
                inner: BufReader::new(BoundedAsyncStream::new(inner)),
                upgraded: upgraded.clone(),
            },
            upgraded,
        )
    }
}
impl<S> fmt::Debug for StarttlsReadGate<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StarttlsReadGate(<redacted>)")
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for StarttlsReadGate<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let allowed = if self.upgraded.load(Ordering::Acquire) {
            buf.remaining()
        } else {
            1
        };
        let (result, count) = {
            let mut limited = ReadBuf::new(buf.initialize_unfilled_to(allowed));
            let result = Pin::new(&mut self.inner).poll_read(cx, &mut limited);
            (result, limited.filled().len())
        };
        buf.advance(count);
        result
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for StarttlsReadGate<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Async stream wrapper with a hard cumulative read budget.
pub(super) struct BoundedAsyncStream<S> {
    inner: S,
    remaining: usize,
}

impl<S> BoundedAsyncStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            remaining: MAX_RECEIVED_BYTES,
        }
    }

    #[cfg(test)]
    fn with_limit(inner: S, limit: usize) -> Self {
        Self {
            inner,
            remaining: limit,
        }
    }
}

impl<S> fmt::Debug for BoundedAsyncStream<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedAsyncStream")
            .field("remaining", &self.remaining)
            .field("inner", &"<redacted>")
            .finish()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for BoundedAsyncStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.remaining == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bounded input limit exceeded",
            )));
        }
        let allowed = self.remaining.min(buf.remaining());
        let (result, consumed) = {
            let storage = buf.initialize_unfilled_to(allowed);
            let mut limited = ReadBuf::new(storage);
            let result = Pin::new(&mut self.inner).poll_read(cx, &mut limited);
            (result, limited.filled().len())
        };
        buf.advance(consumed);
        self.remaining = self.remaining.saturating_sub(consumed);
        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for BoundedAsyncStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Adapter for lettre's Tokio SMTP client trait with a caller-supplied peer address.
pub(super) struct LettreTokioStream<S> {
    inner: BoundedAsyncStream<S>,
    peer_addr: SocketAddr,
}

impl<S> LettreTokioStream<S> {
    pub fn new(inner: S, peer_addr: SocketAddr) -> Self {
        Self {
            inner: BoundedAsyncStream::new(inner),
            peer_addr,
        }
    }
}

impl<S> fmt::Debug for LettreTokioStream<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LettreTokioStream")
            .field("peer_addr", &self.peer_addr)
            .field("inner", &"<redacted>")
            .finish()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for LettreTokioStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for LettreTokioStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
impl<S> AsyncTokioStream for LettreTokioStream<S>
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + fmt::Debug,
{
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.peer_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::sync::{Arc, Mutex};
    use std::task::Poll;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    #[derive(Debug)]
    struct Failing;
    impl AsyncRead for Failing {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::other("fixture failure")))
        }
    }
    impl AsyncWrite for Failing {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _data: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(0))
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    struct PointerReader {
        expected: usize,
    }
    impl AsyncRead for PointerReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let pointer = buf.initialize_unfilled_to(1).as_ptr() as usize;
            assert_eq!(pointer, self.expected);
            buf.put_slice(b"x");
            Poll::Ready(Ok(()))
        }
    }

    struct PendingOnce {
        polls: usize,
    }
    impl AsyncRead for PendingOnce {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.polls == 0 {
                self.polls = 1;
                Poll::Pending
            } else {
                buf.put_slice(b"x");
                Poll::Ready(Ok(()))
            }
        }
    }

    struct TrackingWriter {
        events: Arc<Mutex<Vec<&'static str>>>,
    }
    impl AsyncWrite for TrackingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.events.lock().unwrap().push("write");
            Poll::Ready(Ok(data.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.events.lock().unwrap().push("flush");
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.events.lock().unwrap().push("shutdown");
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fragmented_input_and_exact_limit_are_accepted() {
        let (mut writer, reader) = duplex(8);
        let task = tokio::spawn(async move {
            writer.write_all(b"frag").await.unwrap();
            writer.write_all(b"mented").await.unwrap();
        });
        let mut stream = BoundedAsyncStream::with_limit(reader, 10);
        let mut out = vec![0; 10];
        stream.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"fragmented");
        task.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn excess_input_stops_at_budget() {
        let (mut writer, reader) = duplex(8);
        writer.write_all(b"12345").await.unwrap();
        let mut stream = BoundedAsyncStream::with_limit(reader, 5);
        let mut out = vec![0; 5];
        stream.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"12345");
        assert_eq!(
            stream.read(&mut [0; 1]).await.unwrap_err().kind(),
            ErrorKind::InvalidData
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn underlying_io_error_is_forwarded_without_extra_read() {
        let mut stream = BoundedAsyncStream::with_limit(Failing, 3);
        let error = stream.read(&mut [0; 1]).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_uses_caller_storage_without_allocating_a_second_buffer() {
        let mut output = [0u8; 4];
        let expected = output.as_mut_ptr() as usize;
        let mut stream = BoundedAsyncStream::with_limit(PointerReader { expected }, 1);
        stream.read_exact(&mut output[..1]).await.unwrap();
        assert_eq!(&output[..1], b"x");
    }

    #[test]
    fn pending_read_does_not_consume_budget() {
        let mut output = [0u8; 1];
        let mut stream = BoundedAsyncStream::with_limit(PendingOnce { polls: 0 }, 1);
        let mut read_buf = ReadBuf::new(&mut output);
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        {
            let mut pinned = Pin::new(&mut stream);
            assert!(matches!(
                pinned.as_mut().poll_read(&mut cx, &mut read_buf),
                Poll::Pending
            ));
        }
        assert_eq!(stream.remaining, 1);
        let pinned = Pin::new(&mut stream);
        assert!(matches!(
            pinned.poll_read(&mut cx, &mut read_buf),
            Poll::Ready(Ok(()))
        ));
        assert_eq!(stream.remaining, 0);
    }

    #[test]
    fn zero_capacity_and_exhausted_reads_do_not_poll_inner() {
        let mut zero = BoundedAsyncStream::with_limit(Failing, 0);
        let mut empty = ReadBuf::new(&mut []);
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(matches!(
            Pin::new(&mut zero).poll_read(&mut cx, &mut empty),
            Poll::Ready(Ok(()))
        ));

        let mut exhausted = BoundedAsyncStream::with_limit(Failing, 0);
        let mut data = [0u8; 1];
        let mut read_buf = ReadBuf::new(&mut data);
        let error = match Pin::new(&mut exhausted).poll_read(&mut cx, &mut read_buf) {
            Poll::Ready(Err(error)) => error,
            other => panic!("unexpected poll result: {other:?}"),
        };
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_flush_and_shutdown_are_forwarded() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut stream = BoundedAsyncStream::with_limit(
            TrackingWriter {
                events: Arc::clone(&events),
            },
            0,
        );
        stream.write_all(b"x").await.unwrap();
        stream.flush().await.unwrap();
        stream.shutdown().await.unwrap();
        assert_eq!(&*events.lock().unwrap(), &["write", "flush", "shutdown"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn zero_capacity_read_is_normal_and_write_is_forwarded() {
        let (_writer, reader) = duplex(8);
        let mut stream = BoundedAsyncStream::with_limit(reader, 0);
        assert_eq!(stream.read(&mut []).await.unwrap(), 0);
        let (peer, peer_reader) = duplex(8);
        let mut output = BoundedAsyncStream::with_limit(peer, 0);
        output.write_all(b"out").await.unwrap();
        let mut received = [0; 3];
        let mut peer_reader = peer_reader;
        peer_reader.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"out");
        output.shutdown().await.unwrap();
    }

    #[test]
    fn debug_does_not_include_inner_value() {
        let stream = BoundedAsyncStream::with_limit("secret-inner", 1);
        let text = format!("{stream:?}");
        assert!(!text.contains("secret-inner"));
        assert!(text.contains("<redacted>"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lettre_adapter_reports_fixed_peer_address() {
        let (_peer, stream) = duplex(8);
        let address = "127.0.0.1:2525".parse().unwrap();
        let adapter = LettreTokioStream::new(stream, address);
        assert_eq!(AsyncTokioStream::peer_addr(&adapter).unwrap(), address);
    }

    #[tokio::test]
    async fn ext03_m201_starttls_gate_keeps_prefetch_below_tls_and_preserves_budget() {
        let (mut peer, stream) = duplex(16);
        peer.write_all(b"abcd").await.unwrap();
        let (mut gate, upgraded) = StarttlsReadGate::new(stream);
        let mut output = [0; 16];
        assert_eq!(gate.read(&mut output).await.unwrap(), 1);
        assert_eq!(output[0], b'a');
        assert_eq!(gate.inner.buffer(), b"bcd");
        assert_eq!(gate.inner.get_ref().remaining, MAX_RECEIVED_BYTES - 4);
        upgraded.store(true, Ordering::Release);
        assert_eq!(gate.read(&mut output).await.unwrap(), 3);
        assert_eq!(&output[..3], b"bcd");
        assert_eq!(gate.inner.get_ref().remaining, MAX_RECEIVED_BYTES - 4);
        peer.write_all(b"e").await.unwrap();
        assert_eq!(gate.read(&mut output).await.unwrap(), 1);
        assert_eq!(gate.inner.get_ref().remaining, MAX_RECEIVED_BYTES - 5);
    }

    #[tokio::test(start_paused = true)]
    async fn ext03_m201_cancel_drops_gate_with_prefetched_data_and_transport() {
        let (mut peer, stream) = duplex(16);
        peer.write_all(b"buffered fixture").await.unwrap();
        let (mut gate, _upgraded) = StarttlsReadGate::new(stream);
        let future = async move {
            let mut byte = [0];
            gate.read_exact(&mut byte).await.unwrap();
            assert!(!gate.inner.buffer().is_empty());
            std::future::pending::<()>().await;
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), future)
                .await
                .is_err()
        );
        assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
        assert!(peer.write_all(b"after cancellation").await.is_err());
    }
}
