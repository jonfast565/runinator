use std::env;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};

use crate::errors::{
    INVALID, INVALID_PARAMS, NOTIFICATION_INVALID_PARAMS, NOTIFICATION_POST, NOTIFICATION_RESPONSE,
    NOTIFICATION_SERVICE_URL, SMTP_CONFIG, SMTP_SEND,
};
use crate::params::{EmailSendParams, NotificationPayload, NotificationSendParams};

pub(crate) async fn send_email(
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let params: EmailSendParams = serde_json::from_value(request.parameters.clone().into())
        .map_err(|err| INVALID_PARAMS.error(err))?;
    if params.to.trim().is_empty() {
        return Err(invalid("missing recipient"));
    }
    if params.subject.trim().is_empty() {
        return Err(invalid("missing subject"));
    }

    let host = params
        .smtp_host
        .or_else(|| env::var("SMTP_HOST").ok())
        .ok_or_else(|| invalid("missing smtp_host (provide via parameters or SMTP_HOST env)"))?;
    let port = params
        .smtp_port
        .or_else(|| env::var("SMTP_PORT").ok().and_then(|v| v.parse().ok()))
        .unwrap_or(587u16);
    let user = params
        .smtp_user
        .or_else(|| env::var("SMTP_USER").ok())
        .unwrap_or_default();
    let password = params
        .smtp_password
        .or_else(|| env::var("SMTP_PASSWORD").ok())
        .unwrap_or_default();
    let from = params
        .from
        .or_else(|| env::var("SMTP_FROM").ok())
        .unwrap_or_else(|| user.clone());

    if from.trim().is_empty() {
        return Err(invalid(
            "missing from address (provide via parameters or SMTP_FROM env)",
        ));
    }

    let from_mailbox: Mailbox = from.parse().map_err(|err: lettre::address::AddressError| {
        invalid(&format!("invalid from address: {err}"))
    })?;
    let to_mailbox: Mailbox = params
        .to
        .parse()
        .map_err(|err: lettre::address::AddressError| {
            invalid(&format!("invalid to address: {err}"))
        })?;

    let mut builder = Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject(&params.subject);

    let body_text = params.body.clone().unwrap_or_default();
    let email = if let Some(html) = params.html_body.clone() {
        builder = builder.header(ContentType::TEXT_HTML);
        builder.body(html)
    } else {
        builder = builder.header(ContentType::TEXT_PLAIN);
        builder.body(body_text.clone())
    }
    .map_err(|err| invalid(&format!("failed to build email: {err}")))?;

    let transport_builder = AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
        .map_err(|err| SMTP_CONFIG.error(err))?
        .port(port);
    let mailer = if !user.is_empty() {
        transport_builder
            .credentials(Credentials::new(user.clone(), password))
            .build()
    } else {
        transport_builder.build()
    };

    mailer
        .send(email)
        .await
        .map_err(|err| SMTP_SEND.error(err))?;

    let notification_id = post_notification(NotificationPayload {
        workflow_run_id: request.run_id,
        channel: "email".into(),
        severity: "info".into(),
        title: format!("Email: {}", params.subject),
        body: Some(body_text),
        target: Some(params.to.clone()),
        metadata: json!({ "subject": params.subject, "from": from }),
    })
    .await
    .ok();

    let output = json!({
        "sent": true,
        "notification_id": notification_id,
        "recipient": params.to
    });

    Ok(TaskExecutionResult {
        message: Some(format!("Email sent to {}", params.to)),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) async fn send_notification(
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let params: NotificationSendParams = serde_json::from_value(request.parameters.clone().into())
        .map_err(|err| NOTIFICATION_INVALID_PARAMS.error(err))?;
    if params.title.trim().is_empty() {
        return Err(invalid("missing title"));
    }
    let notification_id = post_notification(NotificationPayload {
        workflow_run_id: request.run_id,
        channel: "in_app".into(),
        severity: params.severity.unwrap_or_else(|| "info".into()),
        title: params.title.clone(),
        body: params.body,
        target: params.target,
        metadata: params.metadata,
    })
    .await?;

    let output = json!({
        "notification_id": notification_id,
        "title": params.title,
    });

    Ok(TaskExecutionResult {
        message: Some(format!("Notification posted: {}", params.title)),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

async fn post_notification(payload: NotificationPayload) -> Result<String, SendableError> {
    let service_url = env::var("RUNINATOR_SERVICE_URL")
        .map_err(|_| NOTIFICATION_SERVICE_URL.error("missing RUNINATOR_SERVICE_URL"))?;
    if service_url.trim().is_empty() {
        return Err(NOTIFICATION_SERVICE_URL.error("empty RUNINATOR_SERVICE_URL"));
    }
    let url = format!("{}/notifications", service_url.trim_end_matches('/'));
    let body = json!({
        "workflow_run_id": payload.workflow_run_id,
        "channel": payload.channel,
        "severity": payload.severity,
        "title": payload.title,
        "body": payload.body,
        "target": payload.target,
        "metadata": payload.metadata,
    });
    // bound the notification post so a stalled service url cannot hang the action indefinitely.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|err| NOTIFICATION_POST.error(err))?;
    let response = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|err| NOTIFICATION_POST.error(err))?;
    let status = response.status();
    if !status.is_success() {
        return Err(NOTIFICATION_POST.error(format!("notification service returned {status}")));
    }
    let json: Value = response
        .json()
        .await
        .map_err(|err| NOTIFICATION_RESPONSE.error(err))?;
    json.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| NOTIFICATION_RESPONSE.error("missing notification id"))
}

fn invalid(message: &str) -> SendableError {
    INVALID.error(message)
}
