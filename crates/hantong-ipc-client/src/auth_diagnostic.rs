//! Fixed labels only; never accepts wire data, identities, paths or key material.
use hantong_contracts::ErrorCode;
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Connect,
    BeginTimeout,
    FinishTimeout,
    BeginWrite,
    ChallengeRead,
    ChallengeParse,
    Lifetime,
    Registration,
    Epoch,
    Nonce,
    Scope,
    ClientContext,
    ServerContext,
    ClientKey,
    ServerKey,
    TimeBinding,
    ServerEncode,
    ServerSignature,
    ClientEncode,
    ClientSignature,
    FinishWrite,
    ReadyRead,
    ReadyParse,
}
impl Stage {
    fn line(self) -> &'static str {
        match self {
            Self::Connect => "hantong_handshake stage=connect outcome=failed",
            Self::BeginTimeout => "hantong_handshake stage=begin_timeout outcome=failed",
            Self::FinishTimeout => "hantong_handshake stage=finish_timeout outcome=failed",
            Self::BeginWrite => "hantong_handshake stage=begin_write outcome=failed",
            Self::ChallengeRead => "hantong_handshake stage=challenge_read outcome=failed",
            Self::ChallengeParse => "hantong_handshake stage=challenge_parse outcome=failed",
            Self::Lifetime => "hantong_handshake stage=lifetime outcome=failed",
            Self::Registration => "hantong_handshake stage=registration outcome=failed",
            Self::Epoch => "hantong_handshake stage=epoch outcome=failed",
            Self::Nonce => "hantong_handshake stage=nonce outcome=failed",
            Self::Scope => "hantong_handshake stage=scope outcome=failed",
            Self::ClientContext => "hantong_handshake stage=client_context outcome=failed",
            Self::ServerContext => "hantong_handshake stage=server_context outcome=failed",
            Self::ClientKey => "hantong_handshake stage=client_key outcome=failed",
            Self::ServerKey => "hantong_handshake stage=server_key outcome=failed",
            Self::TimeBinding => "hantong_handshake stage=time_binding outcome=failed",
            Self::ServerEncode => "hantong_handshake stage=server_encode outcome=failed",
            Self::ServerSignature => "hantong_handshake stage=server_signature outcome=failed",
            Self::ClientEncode => "hantong_handshake stage=client_encode outcome=failed",
            Self::ClientSignature => "hantong_handshake stage=client_signature outcome=failed",
            Self::FinishWrite => "hantong_handshake stage=finish_write outcome=failed",
            Self::ReadyRead => "hantong_handshake stage=ready_read outcome=failed",
            Self::ReadyParse => "hantong_handshake stage=ready_parse outcome=failed",
        }
    }
}
pub(crate) fn step<T>(stage: Stage, result: Result<T, ErrorCode>) -> Result<T, ErrorCode> {
    result.inspect_err(|_| eprintln!("{}", stage.line()))
}
pub(crate) fn require(condition: bool, stage: Stage) -> Result<(), ErrorCode> {
    step(
        stage,
        if condition {
            Ok(())
        } else {
            Err(ErrorCode::AuthRequired)
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli15_handshake_diagnostic_preserves_result_and_has_only_fixed_labels() {
        let sentinel = "synthetic_token_code_state_secret_never_log";
        assert_eq!(step(Stage::ReadyParse, Ok(sentinel)), Ok(sentinel));
        assert_eq!(
            step::<()>(Stage::ReadyParse, Err(ErrorCode::ScopeDenied)),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(require(false, Stage::Epoch), Err(ErrorCode::AuthRequired));
        assert_eq!(require(true, Stage::Epoch), Ok(()));
        for stage in [
            Stage::Connect,
            Stage::BeginTimeout,
            Stage::FinishTimeout,
            Stage::BeginWrite,
            Stage::ChallengeRead,
            Stage::ChallengeParse,
            Stage::Lifetime,
            Stage::Registration,
            Stage::Epoch,
            Stage::Nonce,
            Stage::Scope,
            Stage::ClientContext,
            Stage::ServerContext,
            Stage::ClientKey,
            Stage::ServerKey,
            Stage::TimeBinding,
            Stage::ServerEncode,
            Stage::ServerSignature,
            Stage::ClientEncode,
            Stage::ClientSignature,
            Stage::FinishWrite,
            Stage::ReadyRead,
            Stage::ReadyParse,
        ] {
            let line = stage.line();
            assert!(!line.contains(sentinel));
            assert!(line.starts_with("hantong_handshake stage="));
            assert!(line.ends_with(" outcome=failed"));
            assert!(
                line.bytes()
                    .all(|b| b.is_ascii_lowercase() || b"_ =".contains(&b))
            );
        }
    }
}
