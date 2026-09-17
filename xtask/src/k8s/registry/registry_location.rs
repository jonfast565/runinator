#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct RegistryLocation {
    pub(super) api_base: Url,
    pub(super) repository_prefix: String,
}

impl RegistryLocation {
    pub(super) fn parse(value: &str) -> Result<Self> {
        let value = value.trim().trim_end_matches('/');
        anyhow::ensure!(!value.is_empty(), "local registry cannot be empty");
        anyhow::ensure!(
            !value.contains("://"),
            "--local-registry must be a Docker registry reference without a URL scheme"
        );

        let (authority, repository_prefix) = value
            .split_once('/')
            .map_or((value, ""), |(authority, prefix)| (authority, prefix));
        anyhow::ensure!(!authority.is_empty(), "local registry host cannot be empty");
        let api_base = Url::parse(&format!("http://{authority}/"))
            .with_context(|| format!("invalid local registry '{value}'"))?;
        anyhow::ensure!(
            api_base.host_str().is_some(),
            "local registry '{value}' has no host"
        );

        Ok(Self {
            api_base,
            repository_prefix: repository_prefix.trim_matches('/').to_string(),
        })
    }

    pub(super) fn repository(&self, image_name: &str) -> String {
        if self.repository_prefix.is_empty() {
            image_name.to_string()
        } else {
            format!("{}/{image_name}", self.repository_prefix)
        }
    }

    pub(super) fn endpoint(
        &self,
        repository: &str,
        operation: &str,
        reference: Option<&str>,
    ) -> Url {
        let mut url = self.api_base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .expect("an HTTP registry URL supports path segments");
            segments.push("v2");
            for segment in repository.split('/') {
                segments.push(segment);
            }
            segments.push(operation);
            if let Some(reference) = reference {
                segments.push(reference);
            }
        }
        url
    }
}
