//! The monthly allowance, on the relay's side: counting each box's bytes and
//! slowing the boxes the control plane says are past their allowance.
//!
//! iroh's relay library can't count per box, so our front door sits in
//! between: it takes each TLS connection, hands iroh a plain connection over
//! loopback, and copies the bytes across, counting them. The gate tells the
//! meter which connection is which box's own (through a header only the
//! front door sets). A box's own connection carries everything sent to and
//! from it through the relay, so only that one is counted, and only that one
//! is slowed.

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::Duration,
};

use iroh_relay::server::streams::Bucket;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zoomriya_keys::PublicKey;

/// The header the front door puts on `/relay` requests: which connection
/// this is. Anything a client sends under this name is replaced.
pub const CONN_HEADER: &str = "zoomriya-conn";

/// One connection through the front door.
#[derive(Debug)]
pub struct Conn {
    /// Where it really comes from.
    pub peer: SocketAddr,
    /// Set when the gate let a box in on it: the box's own connection.
    box_key: OnceLock<PublicKey>,
}

/// The meter.
#[derive(Debug, Default)]
pub struct Meter {
    /// Live connections, by the loopback port they reach iroh from.
    conns: Mutex<HashMap<u16, Arc<Conn>>>,
    /// Bytes per box not yet reported.
    used: Mutex<HashMap<PublicKey, u64>>,
    /// Boxes to slow, and how fast they may go, in bytes a second.
    slow: RwLock<(HashSet<PublicKey>, u32)>,
    /// One bucket per slowed box, shared by both directions.
    buckets: Mutex<HashMap<PublicKey, Bucket>>,
}

impl Meter {
    /// Notes a new connection, before it reaches iroh.
    pub fn open(&self, port: u16, peer: SocketAddr) -> Arc<Conn> {
        let conn = Arc::new(Conn {
            peer,
            box_key: OnceLock::new(),
        });
        if let Ok(mut c) = self.conns.lock() {
            c.insert(port, conn.clone());
        }
        conn
    }

    /// Forgets a connection that ended.
    pub fn close(&self, port: u16) {
        if let Ok(mut c) = self.conns.lock() {
            c.remove(&port);
        }
    }

    /// The connection that reached iroh from `port`.
    pub fn get(&self, port: u16) -> Option<Arc<Conn>> {
        self.conns.lock().ok().and_then(|c| c.get(&port).cloned())
    }

    /// The gate let a box in on this connection: count it.
    pub fn box_connected(&self, header: Option<&str>, box_key: PublicKey) {
        if let Some(conn) = header
            .and_then(|h| h.parse::<u16>().ok())
            .and_then(|p| self.get(p))
        {
            let _ = conn.box_key.set(box_key);
        }
    }

    fn add(&self, box_key: &PublicKey, n: usize) {
        if let Ok(mut u) = self.used.lock() {
            *u.entry(*box_key).or_default() += n as u64;
        }
    }

    /// Takes the counts so far, for a report.
    pub fn take(&self) -> Vec<(PublicKey, u64)> {
        self.used
            .lock()
            .map(|mut u| u.drain().collect())
            .unwrap_or_default()
    }

    /// Puts counts back after a report didn't get through.
    pub fn put_back(&self, counts: Vec<(PublicKey, u64)>) {
        if let Ok(mut u) = self.used.lock() {
            for (k, n) in counts {
                *u.entry(k).or_default() += n;
            }
        }
    }

    /// The boxes to slow, from the control plane.
    pub fn set_slow(&self, boxes: impl IntoIterator<Item = PublicKey>, bytes_per_second: u32) {
        let boxes: HashSet<PublicKey> = boxes.into_iter().collect();
        if let Ok(mut b) = self.buckets.lock() {
            b.retain(|k, _| boxes.contains(k));
        }
        if let Ok(mut s) = self.slow.write() {
            if s.1 != bytes_per_second
                && let Ok(mut b) = self.buckets.lock()
            {
                b.clear();
            }
            *s = (boxes, bytes_per_second.max(1000));
        }
    }

    /// Boxes slowed now.
    pub fn slowed(&self) -> usize {
        self.slow.read().map(|s| s.0.len()).unwrap_or(0)
    }

    /// When this box may send `n` more bytes, if it's slowed and has to wait.
    fn wait(&self, box_key: &PublicKey, n: usize) -> Option<tokio::time::Instant> {
        let bps = {
            let s = self.slow.read().ok()?;
            if !s.0.contains(box_key) {
                return None;
            }
            s.1
        };
        let mut b = self.buckets.lock().ok()?;
        let bucket = match b.entry(*box_key) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(
                Bucket::new(i64::from(bps), i64::from(bps), Duration::from_millis(100)).ok()?,
            ),
        };
        bucket.consume(n).err()
    }

    /// Copies both ways between the client and iroh until either side ends,
    /// counting and slowing the box's own connection.
    pub async fn pump<A, B>(&self, client: A, iroh: B, conn: &Conn)
    where
        A: AsyncRead + AsyncWrite + Unpin,
        B: AsyncRead + AsyncWrite + Unpin,
    {
        let (mut cr, mut cw) = tokio::io::split(client);
        let (mut ir, mut iw) = tokio::io::split(iroh);
        tokio::select! {
            _ = self.copy(&mut cr, &mut iw, conn) => {}
            _ = self.copy(&mut ir, &mut cw, conn) => {}
        }
        let _ = cw.shutdown().await;
        let _ = iw.shutdown().await;
    }

    async fn copy<R, W>(&self, r: &mut R, w: &mut W, conn: &Conn)
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = match r.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            if let Some(box_key) = conn.box_key.get() {
                self.add(box_key, n);
                if let Some(until) = self.wait(box_key, n) {
                    tokio::time::sleep_until(until).await;
                }
            }
            if w.write_all(&buf[..n]).await.is_err() || w.flush().await.is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoomriya_keys::SecretKey;

    #[tokio::test]
    async fn counts_only_boxes_and_slows_them() {
        let meter = Meter::default();
        let b = SecretKey::generate().public();
        let peer: SocketAddr = "192.0.2.1:5000".parse().expect("addr");

        // A device's connection: copied, not counted.
        let device = meter.open(1, peer);
        let (client, mut far) = tokio::io::duplex(1 << 16);
        let (iroh, mut near) = tokio::io::duplex(1 << 16);
        let pump = tokio::spawn({
            let meter = Arc::new(Meter::default());
            async move { meter.pump(client, iroh, &device).await }
        });
        far.write_all(b"hello").await.expect("write");
        let mut got = [0u8; 5];
        near.read_exact(&mut got).await.expect("read");
        assert_eq!(&got, b"hello");
        drop(far);
        drop(near);
        let _ = pump.await;

        // The box's own connection: counted both ways.
        let conn = meter.open(2, peer);
        meter.box_connected(Some("2"), b);
        meter.box_connected(Some("not a port"), b);
        let meter = Arc::new(meter);
        let (client, mut far) = tokio::io::duplex(1 << 16);
        let (iroh, mut near) = tokio::io::duplex(1 << 16);
        let m = meter.clone();
        let pump = tokio::spawn(async move { m.pump(client, iroh, &conn).await });
        far.write_all(&[1; 1000]).await.expect("write");
        let mut got = vec![0u8; 1000];
        near.read_exact(&mut got).await.expect("read");
        near.write_all(&[2; 500]).await.expect("write");
        let mut got = vec![0u8; 500];
        far.read_exact(&mut got).await.expect("read");
        assert_eq!(meter.take(), vec![(b, 1500)]);
        assert!(meter.take().is_empty());

        // Slowed: 10 kB/s with a 10 kB bucket, so 30 kB takes about 2 s.
        meter.set_slow([b], 10_000);
        assert_eq!(meter.slowed(), 1);
        let start = tokio::time::Instant::now();
        far.write_all(&[3; 30_000]).await.expect("write");
        let mut got = vec![0u8; 30_000];
        near.read_exact(&mut got).await.expect("read");
        assert!(start.elapsed() >= Duration::from_millis(1500));
        meter.set_slow([], 10_000);
        assert_eq!(meter.slowed(), 0);
        drop(far);
        drop(near);
        let _ = pump.await;
        meter.put_back(vec![(b, 7)]);
        assert_eq!(meter.take(), vec![(b, 30_007)]);
    }
}
