use client_substrate::{
    CLIENT_SUBSTRATE_API_VERSION, ClientCommand, ClientCommandResult, ClientEventCursor,
    ClientSubstrate,
};
use conversation_core::ParticipantId;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let substrate = ClientSubstrate::builder().build()?;
    println!("client substrate api v{}", CLIENT_SUBSTRATE_API_VERSION);

    let user_id = ParticipantId::from("user-1");
    let agent_id = ParticipantId::from("agent-1");

    let conversation_id = match substrate
        .dispatch(ClientCommand::CreateConversation {
            title: Some("Commercial host smoke".to_string()),
            user_id: user_id.clone(),
            agent_id,
        })
        .await?
    {
        ClientCommandResult::ConversationCreated { conversation_id } => conversation_id,
        other => anyhow::bail!("unexpected create result: {other:?}"),
    };

    let message_id = match substrate
        .dispatch(ClientCommand::SubmitUserMessage {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "Hello from a commercial host".to_string(),
        })
        .await?
    {
        ClientCommandResult::UserMessageSubmitted { message_id } => message_id,
        other => anyhow::bail!("unexpected message result: {other:?}"),
    };

    substrate
        .dispatch(ClientCommand::StartAgentRun {
            conversation_id: conversation_id.clone(),
            trigger_message_id: message_id,
            requested_by: user_id,
        })
        .await?;

    let events = substrate.events_after(ClientEventCursor::beginning());
    let conversations = substrate.conversation_list_projection();
    let timeline = substrate.timeline_projection(conversation_id);
    let runs = substrate.run_projection();
    let diagnostics = substrate.default_diagnostic_bundle_plan();

    println!("events: {}", events.len());
    println!("conversations: {}", conversations.conversations.len());
    println!("timeline items: {}", timeline.items.len());
    println!("runs: {}", runs.runs.len());
    println!(
        "diagnostics secret scan required: {}",
        diagnostics.secret_scan_required
    );

    substrate.host_api_for_bridge().shutdown().await?;
    Ok(())
}
