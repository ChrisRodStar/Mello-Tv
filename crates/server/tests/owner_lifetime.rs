use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Server {
    child: Child,
    port: u16,
    _data: tempfile::TempDir,
}

impl Server {
    fn start(app_owned: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let data = tempfile::tempdir()?;
        let web = data.path().join("web");
        std::fs::create_dir(&web)?;
        std::fs::write(web.join("index.html"), "server lifetime test")?;
        std::fs::File::create(web.join("large.bin"))?.set_len(64 * 1024 * 1024)?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_stream-server"));
        command.args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--db-path",
        ]);
        command.arg(data.path());
        command.arg("--static-dir").arg(&web);
        if app_owned {
            command.arg("--exit-on-stdin-close");
        }
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut server = Self {
            child,
            port,
            _data: data,
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = server.child.try_wait()? {
                return Err(format!("server exited before readiness: {status}").into());
            }
            if server.is_ready() {
                return Ok(server);
            }
            if Instant::now() >= deadline {
                return Err("server did not become ready".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn is_ready(&self) -> bool {
        let Ok(mut socket) = TcpStream::connect(("127.0.0.1", self.port)) else {
            return false;
        };
        let timeout = Some(Duration::from_secs(1));
        if socket.set_read_timeout(timeout).is_err()
            || socket.set_write_timeout(timeout).is_err()
            || socket
                .write_all(
                    b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .is_err()
        {
            return false;
        }
        let mut response = [0; 128];
        match socket.read(&mut response) {
            Ok(count) => response[..count].starts_with(b"HTTP/1.1 200"),
            Err(_) => false,
        }
    }

    fn wait_for_exit(&mut self) -> TestResult {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(status) = self.child.try_wait()? {
                assert!(status.success(), "server shutdown failed: {status}");
                // A clean exit must also release the listener for the next run.
                let listener = socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::STREAM,
                    Some(socket2::Protocol::TCP),
                )?;
                listener.set_reuse_address(true)?;
                let address = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
                listener.bind(&address.into())?;
                listener.listen(1)?;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("server survived owner disconnect or termination".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn app_owned_server_exits_when_owner_pipe_closes() -> TestResult {
    let mut server = Server::start(true)?;
    assert!(server.child.try_wait()?.is_none());
    drop(server.child.stdin.take());
    server.wait_for_exit()
}

#[test]
fn standalone_server_survives_closed_stdin() -> TestResult {
    let mut server = Server::start(false)?;
    drop(server.child.stdin.take());
    thread::sleep(Duration::from_millis(250));
    assert!(server.child.try_wait()?.is_none());
    assert!(
        server.is_ready(),
        "standalone server stopped serving after stdin EOF"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn app_owned_server_can_stop_while_owner_pipe_is_open() -> TestResult {
    let mut server = Server::start(true)?;
    assert!(server.child.stdin.is_some());
    let status = Command::new("/bin/kill")
        .args(["-TERM", &server.child.id().to_string()])
        .status()?;
    assert!(status.success());
    server.wait_for_exit()
}

#[test]
fn owner_disconnect_does_not_wait_forever_for_a_slow_client() -> TestResult {
    let mut server = Server::start(true)?;
    let mut slow_client = TcpStream::connect(("127.0.0.1", server.port))?;
    slow_client.set_read_timeout(Some(Duration::from_secs(2)))?;
    slow_client.write_all(b"GET /large.bin HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    let mut response = [0; 128];
    let count = slow_client.read(&mut response)?;
    assert!(response[..count].starts_with(b"HTTP/1.1 200"));
    // Leave the 64 MB response unread so graceful shutdown has a live request.
    drop(server.child.stdin.take());
    server.wait_for_exit()?;
    drop(slow_client);
    Ok(())
}
