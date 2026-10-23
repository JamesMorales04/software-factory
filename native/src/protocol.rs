use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("transport ended before response")]
    EndOfStream,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
}

pub struct JsonRpc<R, W> {
    reader: BufReader<R>,
    writer: W,
    next_id: u64,
}

impl<R, W> JsonRpc<R, W>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
        }
    }

    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), RpcError> {
        self.send(json!({"method":method,"params":params})).await
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).await? == 0 {
                return Err(RpcError::EndOfStream);
            }
            let value: Value = serde_json::from_str(&line)?;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    return Err(RpcError::Protocol(error.to_string()));
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| RpcError::Protocol("response has no result".into()));
            }
            if value.get("id").is_some() {
                return Err(RpcError::Protocol("unexpected response id".into()));
            }
            if value.get("method").is_some() {
                // Unsolicited notifications do not satisfy this request. Approval
                // requests carry an id and are rejected above until policy handling exists.
                continue;
            }
        }
    }

    async fn send(&mut self, value: Value) -> Result<(), RpcError> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn correlates_response_across_notification() {
        let input = b"{\"method\":\"notice\",\"params\":{}}\n{\"id\":1,\"result\":{\"ok\":true}}\n";
        let output = Vec::new();
        let mut rpc = JsonRpc::new(&input[..], output);
        assert_eq!(
            rpc.request("initialize", json!({})).await.unwrap(),
            json!({"ok":true})
        );
    }

    #[tokio::test]
    async fn rejects_server_request_without_authorization_handler() {
        let input =
            b"{\"id\":99,\"method\":\"item/commandExecution/requestApproval\",\"params\":{}}\n";
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        assert!(matches!(
            rpc.request("initialize", json!({})).await,
            Err(RpcError::Protocol(_))
        ));
    }
}
