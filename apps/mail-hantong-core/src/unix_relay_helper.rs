//! Core-owned one-request Unix relay.
//!
//! The helper receives only a fixed profile selector and a bounded request.
//! Authentication and signing remain inside the already-running Core broker.
//! No key, credential, signature, approval, or retry API is exposed here.

#![cfg(unix)]

use hantong_contracts::ErrorCode;
use hantong_ipc_client::unix_relay;
use std::io::{Read, Write};

const MAX_LIFETIME: std::time::Duration = std::time::Duration::from_secs(40);

/// Runs the short-lived relay entrypoint. Authentication and signing remain in
/// the already-running Core broker; this process only transports one bounded
/// request and response.
pub fn run() -> Result<(), ErrorCode> {
    let mut stdin = std::io::stdin().lock();
    let body = read_sync_frame(&mut stdin)?;
    let request = unix_relay::RelayRequest::decode(&body)?;
    let response = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            tokio::time::timeout(MAX_LIFETIME, unix_relay::broker_exchange(&request)).await
        })
        .map_err(|_| ErrorCode::CoreUnavailable)??;
    unix_relay::validate_response(&response)?;
    let mut stdout = std::io::stdout().lock();
    let length = u32::try_from(response.len()).map_err(|_| ErrorCode::InvalidInput)?;
    stdout
        .write_all(&length.to_le_bytes())
        .and_then(|_| stdout.write_all(&response))
        .and_then(|_| stdout.flush())
        .map_err(|_| ErrorCode::CoreUnavailable)
}

fn read_sync_frame(reader: &mut impl Read) -> Result<Vec<u8>, ErrorCode> {
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let length = u32::from_le_bytes(header) as usize;
    if !(1..=unix_relay::MAX_FRAME).contains(&length) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let mut extra = [0_u8; 1];
    match reader.read(&mut extra) {
        Ok(0) => Ok(body),
        _ => Err(ErrorCode::InvalidInput),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli07_helper_reads_exactly_one_bounded_frame() {
        let mut good = 2_u32.to_le_bytes().to_vec();
        good.extend_from_slice(b"{}");
        assert_eq!(read_sync_frame(&mut good.as_slice()).unwrap(), b"{}");
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(
            read_sync_frame(&mut trailing.as_slice()),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            read_sync_frame(&mut &good[..5]),
            Err(ErrorCode::CoreUnavailable)
        );
        let oversized = ((unix_relay::MAX_FRAME + 1) as u32).to_le_bytes();
        assert_eq!(
            read_sync_frame(&mut oversized.as_slice()),
            Err(ErrorCode::InvalidInput)
        );
    }
}
