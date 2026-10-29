//! No authenticated sessions or mail dispatch exist in this bootstrap surface.
use crate::{MAX_FRAME_SIZE, RpcRequest, decode_request};
use hantong_contracts::ErrorCode;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub fn response(request: RpcRequest) -> Value {
    let result = dispatch(&request);
    match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":request.id,"result":value}),
        Err(code) => {
            json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code.as_str(),"message":code.as_str()}})
        }
    }
}

fn dispatch(request: &RpcRequest) -> Result<Value, ErrorCode> {
    if request.method.starts_with("mail.") {
        return Err(ErrorCode::ClientNotRegistered);
    }
    if !matches!(
        request.method.as_str(),
        "system.version" | "system.readiness"
    ) {
        return Err(ErrorCode::InvalidInput);
    }
    let params = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if params.len() != 1 {
        return Err(ErrorCode::InvalidInput);
    }
    let version = params
        .get("protocol_version")
        .and_then(Value::as_u64)
        .ok_or(ErrorCode::InvalidInput)?;
    if version != 1 {
        return Err(ErrorCode::VersionMismatch);
    }
    if request.method == "system.version" {
        Ok(
            json!({"product":"mail-hantong","version":env!("CARGO_PKG_VERSION"),"protocol_version":1}),
        )
    } else {
        Ok(
            json!({"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1}),
        )
    }
}

pub async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<(), ErrorCode> {
    let length = stream
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::InvalidInput)? as usize;
    if !(1..=MAX_FRAME_SIZE).contains(&length) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut frame = vec![0; length + 4];
    frame[..4].copy_from_slice(&(length as u32).to_le_bytes());
    stream
        .read_exact(&mut frame[4..])
        .await
        .map_err(|_| ErrorCode::InvalidInput)?;
    let request = decode_request(&frame)?;
    let body = serde_json::to_vec(&response(request)).map_err(|_| ErrorCode::InternalError)?;
    stream
        .write_all(&(body.len() as u32).to_le_bytes())
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    stream
        .write_all(&body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    stream
        .flush()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    // DisconnectNamedPipe discards unread bytes. Receipt keeps the response
    // alive until this client consumed it; this is never a mail approval.
    let receipt = stream
        .read_u8()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if receipt != 0x06 {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

/// One connection at a time; a cancelled/expired exchange is discarded entirely.
#[cfg(windows)]
pub async fn serve_until<F, R>(shutdown: F, on_ready: R) -> Result<(), ErrorCode>
where
    F: std::future::Future<Output = Result<(), ErrorCode>>,
    R: FnOnce(),
{
    let mut server = hantong_platform::LocalPipeServer::bind()?;
    tokio::pin!(shutdown);
    on_ready();
    loop {
        let mut connection = tokio::select! {
            biased;
            result=&mut shutdown => return result,
            accepted=server.accept() => match accepted {
                Ok(connection)=>connection,
                Err(ErrorCode::ScopeDenied | ErrorCode::PolicyBlocked | ErrorCode::ClientNotRegistered)=>continue,
                Err(code)=>return Err(code),
            }
        };
        tokio::select! {
            biased;
            result=&mut shutdown => return result,
            _=tokio::time::timeout(std::time::Duration::from_secs(5),exchange(&mut connection)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: "test-1".into(),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn sec03_pre_registration_returns_only_minimal_metadata() {
        let actual = response(request("system.readiness", json!({"protocol_version":1})));
        assert_eq!(
            actual,
            json!({"jsonrpc":"2.0","id":"test-1","result":{
                "transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1
            }})
        );
        let version = response(request("system.version", json!({"protocol_version":1})));
        assert_eq!(
            version["result"],
            json!({"product":"mail-hantong","version":env!("CARGO_PKG_VERSION"),"protocol_version":1})
        );
    }

    #[test]
    fn sec03_mail_calls_cannot_claim_session_or_approval() {
        for method in [
            "mail.accounts.list",
            "mail.send.execute",
            "mail.approvals.issue",
        ] {
            let actual = response(request(
                method,
                json!({"approved":true,"role":"human","session_id":"stolen","client_id":"gui"}),
            ));
            assert_eq!(actual["error"]["code"], "CLIENT_NOT_REGISTERED");
            assert!(actual.get("result").is_none());
            assert!(!actual.to_string().contains("stolen"));
        }
    }

    #[test]
    fn cli06_negotiation_rejects_unknown_fields_and_mismatched_versions() {
        for params in [
            json!({}),
            json!({"protocol_version":"1"}),
            json!({"protocol_version":1,"role":"gui"}),
        ] {
            assert_eq!(
                response(request("system.readiness", params))["error"]["code"],
                "INVALID_INPUT"
            );
        }
        assert_eq!(
            response(request("system.readiness", json!({"protocol_version":2})))["error"]["code"],
            "VERSION_MISMATCH"
        );
        assert_eq!(
            response(request("system.stop", json!({"protocol_version":1})))["error"]["code"],
            "INVALID_INPUT"
        );
    }

    #[test]
    fn sec08_request_debug_never_exposes_untrusted_fields() {
        let value = RpcRequest {
            id: "private-id".into(),
            method: "private-method".into(),
            params: json!({"body":"secret-value"}),
        };
        let debug = format!("{value:?}");
        for text in ["private-id", "private-method", "secret-value"] {
            assert!(!debug.contains(text));
        }
    }

    #[tokio::test]
    async fn cli06_exchange_handles_fragmented_frames_without_waiting_for_eof() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let body=r#"{"jsonrpc":"2.0","id":"unicode-한","method":"system.readiness","params":{"protocol_version":1}}"#.as_bytes();
        let task = tokio::spawn(async move { exchange(&mut server).await });
        client
            .write_all(&(body.len() as u32).to_le_bytes())
            .await
            .unwrap();
        for part in body.chunks(7) {
            client.write_all(part).await.unwrap();
        }
        let len = client.read_u32_le().await.unwrap();
        let mut out = vec![0; len as usize];
        client.read_exact(&mut out).await.unwrap();
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["id"], "unicode-한");
        assert_eq!(value["result"]["mail_ready"], false);
        client.write_u8(0x06).await.unwrap();
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn sec03_malformed_and_oversized_frames_produce_no_reflected_response() {
        for frame in [
            vec![0, 0, 0, 0],
            ((MAX_FRAME_SIZE + 1) as u32).to_le_bytes().to_vec(),
            vec![1, 0, 0, 0, b'{'],
        ] {
            let (mut client, mut server) = tokio::io::duplex(32);
            let task = tokio::spawn(async move { exchange(&mut server).await });
            client.write_all(&frame).await.unwrap();
            assert!(task.await.unwrap().is_err());
            let mut output = Vec::new();
            client.read_to_end(&mut output).await.unwrap();
            assert!(output.is_empty());
        }
    }
}
