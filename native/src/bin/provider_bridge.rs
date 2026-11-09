//! Unix-socket to loopback-TCP bridge for provider servers that only listen on
//! TCP inside the confined namespace.

use std::env;

use tokio::{
    io::AsyncWriteExt,
    net::{TcpStream, UnixListener},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = env::var("FACTORY_PROVIDER_SOCKET")?;
    let target = env::var("FACTORY_PROVIDER_TCP")?;
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    }
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return Ok(());
        };
        let target = target.clone();
        tokio::spawn(async move {
            let Ok(mut upstream) = TcpStream::connect(target).await else {
                return;
            };
            // The provider event stream is long-lived by protocol. A total
            // connection deadline would sever an otherwise healthy SSE stream
            // mid-turn; the sandbox and provider session own the lifecycle.
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            let _ = stream.shutdown().await;
            let _ = upstream.shutdown().await;
        });
    }
}
