use super::*;

use runinator_ctl_core::cli::{
    BrokerIngressCommands, BrokerMessageCommands, DeadLetterCommands, ExternalIngressCommands,
};

pub(super) async fn ingress(
    client: &Client,
    command: &IngressCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        IngressCommands::External { command } => external(client, command).await?,
        IngressCommands::Broker { command } => broker(client, command).await?,
        IngressCommands::Messages { command } => {
            let BrokerMessageCommands::List {
                workflow_run,
                pipeline_run,
                adapter,
                channel,
                limit,
            } = command;
            client
                .fetch_broker_messages(
                    *workflow_run,
                    *pipeline_run,
                    *adapter,
                    channel.as_deref(),
                    *limit,
                )
                .await?
        }
        IngressCommands::DeadLetters { command } => {
            let DeadLetterCommands::List { channel, limit } = command;
            client
                .fetch_dead_letters(channel.as_deref(), *limit)
                .await?
        }
    };
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}

async fn external(client: &Client, command: &ExternalIngressCommands) -> Result<Value> {
    Ok(match command {
        ExternalIngressCommands::List {
            target_kind,
            target_id,
            state,
            limit,
        } => {
            client
                .fetch_external_ingress(
                    target_kind.as_deref(),
                    *target_id,
                    state.as_deref(),
                    *limit,
                )
                .await?
        }
        ExternalIngressCommands::Configure {
            target_kind,
            target_id,
            mode,
        } => {
            client
                .configure_external_ingress(target_kind, *target_id, mode.as_str())
                .await?
        }
        ExternalIngressCommands::Approve { id } => {
            client.decide_external_ingress(*id, "approve").await?
        }
        ExternalIngressCommands::Drop { id } => client.decide_external_ingress(*id, "drop").await?,
        ExternalIngressCommands::Release {
            target_kind,
            target_id,
        } => {
            client
                .release_external_ingress(target_kind, *target_id)
                .await?
        }
    })
}

async fn broker(client: &Client, command: &BrokerIngressCommands) -> Result<Value> {
    Ok(match command {
        BrokerIngressCommands::List {
            state,
            channel,
            limit,
        } => {
            client
                .fetch_broker_ingress(state.as_deref(), channel.as_deref(), *limit)
                .await?
        }
        BrokerIngressCommands::Session {
            scope_kind,
            scope_id,
        } => {
            client
                .fetch_broker_ingress_session(scope_kind, *scope_id)
                .await?
        }
        BrokerIngressCommands::Configure {
            scope_kind,
            scope_id,
            mode,
        } => {
            client
                .configure_broker_ingress_session(scope_kind, *scope_id, mode)
                .await?
        }
        BrokerIngressCommands::Renew {
            scope_kind,
            scope_id,
        } => {
            client
                .renew_broker_ingress_session(scope_kind, *scope_id)
                .await?
        }
        BrokerIngressCommands::Approve { id } => {
            client.decide_broker_ingress(*id, "approve").await?
        }
        BrokerIngressCommands::Drop { id } => client.decide_broker_ingress(*id, "drop").await?,
    })
}
