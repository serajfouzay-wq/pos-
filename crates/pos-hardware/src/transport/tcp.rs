use std::io::Write;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::{PrinterTarget, TransportError};

const TIMEOUT: Duration = Duration::from_secs(4);

pub(super) fn send(
    target: &PrinterTarget,
    host: &str,
    port: u16,
    bytes: &[u8],
) -> Result<(), TransportError> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| TransportError::unreachable(target, e))?
        .next()
        .ok_or_else(|| TransportError::unreachable(target, "address did not resolve"))?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT)
        .map_err(|e| TransportError::unreachable(target, e))?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .and_then(|()| stream.write_all(bytes))
        .and_then(|()| stream.flush())
        .map_err(|e| TransportError::unreachable(target, e))?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn delivers_bytes_to_a_network_printer() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let reader = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut received = Vec::new();
            socket.read_to_end(&mut received).expect("read");
            received
        });
        let target = PrinterTarget::Tcp {
            host: "127.0.0.1".into(),
            port,
        };
        crate::transport::send(&target, b"\x1b@hello").expect("sent");
        assert_eq!(reader.join().expect("join"), b"\x1b@hello");
    }

    #[test]
    fn a_closed_port_is_unreachable() {
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let target = PrinterTarget::Tcp {
            host: "127.0.0.1".into(),
            port,
        };
        assert!(matches!(
            crate::transport::send(&target, b"x"),
            Err(TransportError::Unreachable { .. })
        ));
    }
}
