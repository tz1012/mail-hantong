# 논리 데이터 모델 v1

상태: 2026-10-29 연속 개발 위임에 따른 초안. **사람 검토 필요.** 이 문서는 물리 DB 스키마나 암호화 구현의 승인을 뜻하지 않는다.

## 공통 규칙

- 모든 영속 객체는 소유 계정이 있으면 `account_id`를 반드시 가진다. 계정 범위가 필요한 요청은 명시적인 `account_id` 또는 비어 있지 않은 `account_ids`를 받으며 빈 목록은 전체를 뜻하지 않는다.
- 저장 시각·만료 시각은 UTC로 저장하고, 사용자의 timezone은 표시·예약 입력 해석에만 사용한다. 예약 발송의 기준 시각은 `scheduled_at_utc`다.
- 외부 `Message-ID`는 누락·중복될 수 있으므로 내부 유일 키로 쓰지 않는다. 메일 식별은 `account_id + mailbox_id + uid_validity + uid` 같은 제공자 위치 키와 내부 버전을 함께 사용한다.
- 변경 요청은 `operation_id`, `expected_version`, `payload_hash`를 받는다. 버전이 다르면 덮어쓰지 않고 `CONFLICT`를 반환한다. 목록·캐시·커서의 부분 실패를 숨기지 않는다.
- 토큰·앱 비밀번호·API 키 원문은 일반 DB·설정·로그·감사·백업에 저장하지 않는다. 비밀은 Secrets 모듈의 불투명 참조만 저장한다. 메일 원문은 신뢰할 수 없는 데이터이며 감사 이유나 정책 명령으로 복사하지 않는다.
- 발송은 Outbox/SendIntent를 통해서만 진행한다. 승인 요청과 1회 소비 승인은 요청 해시·계정·초안 버전·세션·만료에 결속되며, 승인 발급은 신뢰 채널에서만 한다.

## 엔터티

| 엔터티 | 핵심 필드·식별자 | 경계와 비고 |
| --- | --- | --- |
| Account | `account_id`, provider, provider_subject, primary_address, display_name, color, number, status, capabilities, granted_scopes, settings_version | 모든 공급자 객체의 소유 범위. archive 가상 계정은 서버 동기화·발송 불가 |
| CredentialRef | `account_id`, secret_store_key, token_status, updated_at | 토큰·암호 값 자체는 일반 DB에 저장하지 않음 |
| Message | `internal_message_id`, account_id, provider_message_id, provider_thread_id, internet_message_id, headers, body_ref, received_at, server_version, cache_state | 원문 Message-ID는 유일 키가 아님 |
| Mailbox and Membership | `account_id`, mailbox_id, server_path, delimiter, special_use, uidvalidity, uid, flags | IMAP 위치 키는 계정·사서함·UIDVALIDITY·UID의 조합 |
| Thread | `account_id`, provider_thread_id, subject, latest_at, participant_summary | 다른 계정 자동 결합 금지 |
| Label and MessageLabel | account_id, provider_label_id, name, type, mapping | Gmail 다중 라벨 관계 보존 |
| Attachment | account_id, message_id/draft_id, provider_attachment_id, filename, MIME, size, hash, encrypted_blob_ref, cache_state | 안전한 캐시 참조만 저장 |
| Draft | draft_id, account_id, provider_draft_id, version, recipients, body_ref, attachment_refs, signature_id, alias_id, sync_state | 초안과 발송 분리 |
| SyncState | account_id, mailbox_id, provider cursor, watch/subscription expiry, initial_sync_range, last_success_at, retry_after, sync_status | UID·history·delta 상태를 제공자별로 격리 |
| Operation and SendIntent | operation_id, account_id, client_id, kind, target_ids, payload_hash, idempotency_key, state, attempt_count, last_error, provider_result | 결과 불명은 `SEND_UNKNOWN`, 자동 재발송 금지 |
| ApprovalRequest | approval_request_id, client_id, operation_hash, account_id, target_snapshot_ref, display_summary, risk_level, status, created_at, expires_at, decided_at, decided_channel, deny_reason | 비밀·메일 원문을 저장하지 않음 |
| Approval | approval_id, approval_request_id, operation_hash, account_id, target_snapshot, draft_version, expires_at, consumed_at | 1회 소비 |
| ClientRegistration | client_id, kind, display_name, allowed_accounts, allowed_tools, read_only, rate_limits, status, approved_at, approved_channel | 인증 증거·철회 epoch는 ADR-019에서 확정 |
| EventLog | event_seq, account_id, kind, object_ref, created_at | 본문·제목 저장 금지, 보존 기간 적용 |
| ClientCursor | client_id, stream, cursor, acked_at | 만료 시 `CURSOR_EXPIRED` |
| ConnectionSession | session_id, account_kind, route, completed_steps, state, expires_at, handoff_link_hash, bound_host | 계정 연결 마법사·handoff의 진행 상태. IPC 인증 세션과 별개 |
| SetupState | step_id, status, completed_by, completed_at, result_ref | 사람 단계 결과 참조 |
| ImportJob | job_id, source_type, source_hash, target, progress_cursor, imported, skipped, failed_list_ref | 지원 조합·보존 판정 별도 계약 |
| Snooze | account_id, message_ref, wake_at_utc, hide_method, original_location, status | 원래 위치 보존 |
| QuickAction | action_id, owner, name, steps, key_slot, version | 실행 가능 범위는 정책으로 제한 |
| ObservedAddress / SavedContact | contact_id, scope, account_id, display_name, addresses, notes, group_ids, source, version | 관측 주소와 저장 연락처 분리 |
| AgentRun and AuditEvent | run_id, request_id, client_id, model_policy, tool_name, approval_id, timestamp, outcome, redacted_details | 원문·비밀을 redacted_details에 복사 금지 |
| Rule and ModelPolicy | owner, account_scope, allowed_actions, max_targets, provider_or_model, data_scope, version, enabled | 결정적 규칙과 AI 정책 분리 |
| Device (2단계) | device_id, public_key, allowed_accounts, approval_capable, paired_at, revoked_at | 2단계에서만 활성화 |
| ScheduledSend | schedule_id, account_id, draft_id, draft_version, scheduled_at_utc, timezone_id, schedule_version, approval_ref, operation_id, state, cancelled_at, last_error | 예약 수정·취소는 버전/CAS와 재승인 적용 |
| LocalTag and PinnedView | tag_id, account_id, name, color, version; pinned_view_id, owner, account_id, target_kind/id, position | 서버 라벨과 로컬 태그 ID 혼용 금지 |
| Classification and RuleExecution | account_id, message_id, category, source, rule_id/version, reason, user_override, previous_state_ref, execution_id, quota_use, result | 감사 이유에 메일 원문 복사 금지 |

## 관계·보호 경계

`Account`가 모든 메일·폴더·연락처·예약·규칙의 범위를 결정한다. `ClientRegistration`의 허용 범위는 인증된 세션의 권한과 교집합을 이루며 요청 본문의 client ID나 TTY가 이를 넓히지 못한다. `Draft` → `SendIntent` → `ApprovalRequest` → `Approval` → `Outbox` 순서를 건너뛰는 발송 경로는 없다. 승인 상태(`Approval`)와 발송 원장 상태(`Outbox`/`Operation`)는 별개이며, 어느 한쪽의 상태만으로 다른 쪽을 승인·완료로 해석하지 않는다. `SecretHandle`만 계정 자격증명을 가리키며 데이터 모델에 평문 필드는 없다.

`Device`는 2단계 엔터티다. `ConnectionSession`은 계정 연결 절차를 이어가기 위한 객체이며 해당 절차의 만료·호스트 결속 규칙을 따른다. ADR-019의 IPC 인증 세션은 별도 서버 메모리 객체이며 transport 연결 종료 때 폐기하고 직렬화하지 않는다. `EventLog`·`AgentRun`은 진단에 필요한 최소 메타데이터만 보관하며 제목·본문·주소·토큰을 기본 마스킹한다. 백업은 이 모델의 암호화·보존 정책을 우회할 수 없다.

## 물리 구현으로 넘길 때의 검토 항목

물리 테이블·마이그레이션·암호화 페이지·첨부 원자성·인덱스·보존 정책은 M1-04/ADR-004에서 별도로 확정한다. 이 문서의 초안과 JSON Schema는 해당 카드 완료 또는 API 활성화를 주장하지 않는다.
