//! The name a TLS connection asks for (SNI), read from its first message
//! before anything is decrypted, so the front door can pass the connection
//! on unopened (`front.rs`, `[[pass_through]]`).

/// What the first bytes say.
#[derive(Debug, PartialEq, Eq)]
pub enum Sni {
    /// The name asked for, lowercase.
    Name(String),
    /// A TLS hello with no name, or not TLS at all.
    None,
    /// Not all of the hello has arrived yet.
    More,
}

/// Biggest hello we wait for.
pub const MAX_HELLO: usize = 16 * 1024;

/// Reads the name from a ClientHello at the start of `buf`.
pub fn read(buf: &[u8]) -> Sni {
    if buf.len() < 5 {
        return Sni::More;
    }
    // A handshake record.
    if buf[0] != 0x16 {
        return Sni::None;
    }
    let len = usize::from(u16::from_be_bytes([buf[3], buf[4]]));
    if len > MAX_HELLO {
        return Sni::None;
    }
    let Some(record) = buf.get(5..5 + len) else {
        return Sni::More;
    };
    hello(record).map_or(Sni::None, Sni::Name)
}

fn hello(b: &[u8]) -> Option<String> {
    let mut r = Reader { b, at: 0 };
    // ClientHello.
    if r.u8()? != 1 {
        return None;
    }
    let len = r.u24()?;
    let body = r.take(len)?;
    let mut r = Reader { b: body, at: 0 };
    r.take(2 + 32)?; // version, random
    let n = usize::from(r.u8()?);
    r.take(n)?; // session ID
    let n = usize::from(r.u16()?);
    r.take(n)?; // cipher suites
    let n = usize::from(r.u8()?);
    r.take(n)?; // compression
    let n = usize::from(r.u16()?);
    let mut ext = Reader {
        b: r.take(n)?,
        at: 0,
    };
    while ext.at < ext.b.len() {
        let kind = ext.u16()?;
        let n = usize::from(ext.u16()?);
        let data = ext.take(n)?;
        if kind != 0 {
            continue;
        }
        let mut list = Reader { b: data, at: 0 };
        let n = usize::from(list.u16()?);
        let mut names = Reader {
            b: list.take(n)?,
            at: 0,
        };
        while names.at < names.b.len() {
            let kind = names.u8()?;
            let n = usize::from(names.u16()?);
            let name = names.take(n)?;
            if kind == 0 {
                return std::str::from_utf8(name).ok().map(str::to_ascii_lowercase);
            }
        }
    }
    None
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    fn u24(&mut self) -> Option<usize> {
        self.take(3)
            .map(|b| (usize::from(b[0]) << 16) | (usize::from(b[1]) << 8) | usize::from(b[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real ClientHello for `turn.zoomriya.net`, made by rustls.
    fn hello_for(name: &str) -> Vec<u8> {
        use std::sync::Arc;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        let mut conn = rustls::ClientConnection::new(
            Arc::new(config),
            rustls::pki_types::ServerName::try_from(name.to_string()).expect("name"),
        )
        .expect("client");
        let mut out = Vec::new();
        conn.write_tls(&mut out).expect("hello");
        out
    }

    #[test]
    fn reads_the_name() {
        let h = hello_for("TURN.zoomriya.net");
        assert_eq!(read(&h), Sni::Name("turn.zoomriya.net".into()));
        assert_eq!(read(&h[..h.len() - 1]), Sni::More);
        assert_eq!(read(&h[..3]), Sni::More);
        assert_eq!(read(b"GET / HTTP/1.1\r\n"), Sni::None);
    }
}
