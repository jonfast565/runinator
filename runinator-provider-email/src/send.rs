use std::env;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};
use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use serde_json::{Value, json};

use crate::params::{EmailSendParams, NotificationPayload, NotificationSendParams};

pub(crate) async fn send_email(
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let params: EmailSendParams =
        serde_json::from_value(request.parameters.clone()).map_err(|err| {
            Box::new(RuntimeError::new(
                "email.invalid_params".into(),
                err.to_string(),
            )) as SendableError
        })?;
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
        .map_err(|err| {
            Box::new(RuntimeError::new(
                "email.smtp_config".into(),
                err.to_string(),
            )) as SendableError
        })?
        .port(port);
    let mailer = if !user.is_empty() {
        transport_builder
            .credentials(Credentials::new(user.clone(), password))
            .build()
    } else {
        transport_builder.build()
    };

    mailer.send(email).await.map_err(|err| {
        Box::new(RuntimeError::new("email.smtp_send".into(), err.to_string())) as SendableError
    })?;

    let notification_id = post_notification(NotificationPayload {
        workflow_run_id: request.run_id,
        channel: "email".into(),
        severity: "info".into(),
        title: format!("Email: {}", params.subject),
        body: Some(body_text),
        target: Some(params.to.clone()),
        metadata: json!({ "subject": params.subject, "from": from }),
    })
    .await;

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
    let params: NotificationSendParams = serde_json::from_value(request.parameters.clone())
        .map_err(|err| {
            Box::new(RuntimeError::new(
                "notification.invalid_params".into(),
                err.to_string(),
            )) as SendableError
        })?;
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
    .await;

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

async fn post_notification(payload: NotificationPayload) -> Option<i64> {
    let service_url = env::var("RUNINATOR_SERVICE_URL").ok()?;
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
    let client = reqwest::Client::new();
    let response = client.post(url).json(&body).send().await.ok()?;
    let json: Value = response.json().await.ok()?;
    json.get("id").and_then(Value::as_i64)
}

fn invalid(message: &str) -> SendableError {
    Box::new(RuntimeError::new("email.invalid".into(), message.into())) as SendableError
}
