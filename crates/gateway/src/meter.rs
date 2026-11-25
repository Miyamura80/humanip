//! Per-connection metering: byte counting, metrics hooks and usage records.
//!
//! ```text
//! Conn::open(proto)          -> metrics::connection_opened
//!   conn.meter(upstream, ..) -> Counted<upstream>  (counts every byte written / read)
//!   ... splice ...
//! drop(conn)                 -> metrics::bytes, AccountStore::record_usage (spawn_blocking),
//!                               metrics::connection_closed(proto, result)
//! ```
//!
//! Billing happens in `Drop`, so a connection is charged exactly once
//! whichever way it ends after a successful dial (clean close, I/O error,
//! early return).

use crate::metrics::{self, ConnResult, Proto};
use humanip_core::accounts::{AccountStore, UsageEvent};
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Bytes seen on one upstream stream.
#[derive(Debug, Default)]
pub struct Counters {
    /// Written to the upstream (client to exit).
    pub up: AtomicU64,
    /// Read from the upstream (exit to client).
    pub down: AtomicU64,
}

impl Counters {
    pub fn get(&self) -> (u64, u64) {
        (
            self.up.load(Ordering::Relaxed),
            self.down.load(Ordering::Relaxed),
        )
    }
}

/// A stream that counts the bytes read from and written to it.
pub struct Counted<S> {
    inner: S,
    counters: Arc<Counters>,
}

impl<S> Counted<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            counters: Arc::default(),
        }
    }

    pub fn counters(&self) -> Arc<Counters> {
        self.counters.clone()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        let n = buf.filled().len().saturating_sub(before) as u64;
        self.counters.down.fetch_add(n, Ordering::Relaxed);
        r
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &r {
            self.counters.up.fetch_add(*n as u64, Ordering::Relaxed);
        }
        r
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// What a metered connection is billed as.
pub struct Billing {
    pub store: Arc<dyn AccountStore>,
    pub key_id: String,
    pub provider: String,
    pub domain: String,
    /// Provider list price, USD per GB, before the fee.
    pub provider_price_per_gb: f64,
}

/// One client connection's lifetime: metrics on open and close, and a usage
/// record on close once [`Conn::meter`] was called.
pub struct Conn {
    proto: Proto,
    result: ConnResult,
    bill: Option<(Billing, Arc<Counters>)>,
}

impl Conn {
    /// Starts tracking; the result defaults to `ProtocolError` until set.
    pub fn open(proto: Proto) -> Self {
        metrics::connection_opened(proto);
        Self {
            proto,
            result: ConnResult::ProtocolError,
            bill: None,
        }
    }

    pub fn set(&mut self, result: ConnResult) {
        self.result = result;
    }

    /// Call right after a successful dial: wraps the upstream so its bytes
    /// are billed when this `Conn` drops.
    pub fn meter<S>(&mut self, upstream: S, billing: Billing) -> Counted<S> {
        let counted = Counted::new(upstream);
        self.bill = Some((billing, counted.counters()));
        counted
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn record(store: &dyn AccountStore, ev: &UsageEvent) {
    if let Err(e) = store.record_usage(ev) {
        tracing::error!(key_id = %ev.key_id, provider = %ev.provider, error = %e, "usage not recorded");
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        if let Some((b, counters)) = self.bill.take() {
            let (bytes_up, bytes_down) = counters.get();
            metrics::bytes(&b.provider, bytes_up, bytes_down);
            let ev = UsageEvent {
                key_id: b.key_id,
                provider: b.provider,
                domain: b.domain,
                bytes_up,
                bytes_down,
                provider_price_per_gb: b.provider_price_per_gb,
                at_unix: now_unix(),
            };
            let store = b.store;
            match tokio::runtime::Handle::try_current() {
                Ok(h) => {
                    h.spawn_blocking(move || record(&*store, &ev));
                }
                Err(_) => record(&*store, &ev),
            }
        }
        metrics::connection_closed(self.proto, self.result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn counts_both_directions() {
        let (a, mut b) = tokio::io::duplex(64);
        let mut c = Counted::new(a);
        c.write_all(b"hello").await.unwrap();
        b.write_all(b"abc").await.unwrap();
        let mut buf = [0u8; 3];
        c.read_exact(&mut buf).await.unwrap();
        assert_eq!(c.counters().get(), (5, 3));
    }
}
