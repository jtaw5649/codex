use anyhow::Result;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookActivityEvent;
use codex_protocol::protocol::HookActivityHook;
use codex_protocol::protocol::HookActivityStatus;
use codex_protocol::protocol::HookActivityTool;
use pretty_assertions::assert_eq;

#[test]
fn hook_activity_event_round_trip() -> Result<()> {
    let event = HookActivityEvent {
        status: HookActivityStatus::Blocked,
        reason: Some("blocked by guard".into()),
        tool: None,
        hooks: vec![],
    };

    let json_event = serde_json::to_value(EventMsg::HookActivity(event.clone()))?;
    let decoded: EventMsg = serde_json::from_value(json_event)?;

    match decoded {
        EventMsg::HookActivity(decoded_event) => assert_eq!(decoded_event, event),
        _ => panic!("expected HookActivity event"),
    }

    Ok(())
}

#[test]
fn hook_activity_event_round_trip_with_tool_and_hooks() -> Result<()> {
    let event = HookActivityEvent {
        status: HookActivityStatus::Blocked,
        reason: Some("blocked by guard".into()),
        tool: Some(HookActivityTool {
            name: "Edit".into(),
            past_tense: "Edited".into(),
        }),
        hooks: vec![HookActivityHook {
            name: "tdd-guard".into(),
            decision: "block".into(),
        }],
    };

    let json_event = serde_json::to_value(EventMsg::HookActivity(event.clone()))?;
    let decoded: EventMsg = serde_json::from_value(json_event)?;

    match decoded {
        EventMsg::HookActivity(decoded_event) => assert_eq!(decoded_event, event),
        _ => panic!("expected HookActivity event"),
    }

    Ok(())
}
