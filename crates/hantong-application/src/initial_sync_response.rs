//! Public initial-preview projections contain progress, never fetched metadata.
use hantong_contracts::{
    ErrorCode,
    initial_sync::{InitialSyncPlanSnapshot, InitialSyncReceipt},
};
use serde_json::{Value, json};

fn plan_value(v: &InitialSyncPlanSnapshot) -> Value {
    let p = v.progress();
    json!({"plan_id":hantong_contracts::wire_hex::encode(v.plan_id()),"account_id":v.account_id(),
        "wire_hex":v.mailbox().wire_hex(),"encoding":v.mailbox().encoding().as_str(),"generation":v.generation(),
        "version":v.version(),"is_current":v.is_current(),"binding_current":v.binding_current(),"stored_at_utc_ms":v.stored_at_utc_ms(),
        "progress":{"uid_validity":p.uid_validity(),"uid_next":p.uid_next(),"recent_pages":p.recent_pages(),
            "completed_page_count":p.completed_page_count(),"state":p.state().as_str(),"model_version":p.model_version(),
            "target_uid_ceiling":p.target_uid_ceiling(),"selected_lower_uid":p.selected_lower_uid(),"selected_page_count":p.selected_page_count(),
            "remaining_page_count":p.remaining_page_count(),"remaining_uid_span":p.remaining_uid_span(),"not_selected_before_uid":p.not_selected_before_uid()}})
}
pub fn render_initial_sync_receipt(v: &InitialSyncReceipt) -> Value {
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":hantong_contracts::wire_hex::encode(v.execution_id()),
        "action":v.action().as_str(),"state":v.state().as_str(),"plan":v.plan().map(plan_value),"error_code":v.error().map(ErrorCode::as_str)})
}
pub fn render_initial_sync_plan(v: &InitialSyncPlanSnapshot) -> Value {
    json!({"schema_version":1,"ok":true,"plan":plan_value(v)})
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::{
        initial_sync::*,
        uid_page::{UidMailboxEncoding, UidMailboxIdentity},
    };
    fn plan(next: u32) -> InitialSyncPlanSnapshot {
        InitialSyncPlanSnapshot::new(
            InitialSyncPlanIdentity::new(
                [1; 32],
                "acct",
                UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
                1,
            )
            .unwrap(),
            1,
            InitialSyncProgress::new(
                7,
                next,
                8,
                0,
                if next == 1 {
                    InitialSyncState::Complete
                } else {
                    InitialSyncState::Ready
                },
                1,
            )
            .unwrap(),
            true,
            true,
            1,
        )
        .unwrap()
    }
    #[test]
    fn sync01_initial_renderer_preserves_required_nulls_and_frozen_uid_span() {
        let admitted = InitialSyncReceipt::new(
            "acct",
            [1; 32],
            InitialSyncAction::Start,
            InitialSyncJobState::Admitted,
            None,
            None,
        )
        .unwrap();
        let value = render_initial_sync_receipt(&admitted);
        assert!(value["plan"].is_null());
        assert!(value["error_code"].is_null());
        assert_eq!(value.as_object().unwrap().len(), 8);
        for next in [1, u32::MAX] {
            let snapshot = plan(next);
            let receipt = InitialSyncReceipt::new(
                "acct",
                [1; 32],
                InitialSyncAction::Start,
                InitialSyncJobState::Succeeded,
                Some(snapshot.clone()),
                None,
            )
            .unwrap();
            let value = render_initial_sync_receipt(&receipt);
            assert_eq!(value["plan"], render_initial_sync_plan(&snapshot)["plan"]);
            assert_eq!(
                value["plan"]["progress"]["remaining_uid_span"],
                if next == 1 { 0 } else { 510 }
            );
            if next == 1 {
                assert!(value["plan"]["progress"]["selected_lower_uid"].is_null());
            }
            assert!(serde_json::to_vec(&value).unwrap().len() < 8192);
            assert!(value["plan"].get("messages").is_none());
        }
    }
}
