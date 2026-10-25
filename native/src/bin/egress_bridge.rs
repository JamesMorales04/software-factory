//! Loopback TCP to Unix-socket bridge used inside the confined provider
//! sandbox. It has no credential and forwards only to the bound host broker.

use std::{env, time::Duration};

use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, UnixStream},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = env::var("FACTORY_EGRESS_LISTEN_PORT")?.parse()?;
    let socket = env::var("FACTORY_EGRESS_SOCKET")?;
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return Ok(());
        };
        let socket = socket.clone();
        tokio::spawn(async move {
            let Ok(mut upstream) = UnixStream::connect(socket).await else {
                return;
            };
            let _ = tokio::time::timeout(
                Duration::from_secs(150),
                tokio::io::copy_bidirectional(&mut stream, &mut upstream),
            )
            .await;
            let _ = stream.shutdown().await;
            let _ = upstream.shutdown().await;
        });
    }
}
