#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
    /// optional explicit slug; derived from `name` when omitted.
    #[serde(default)]
    pub slug: Option<String>,
}

impl Validate for CreateOrgRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        optional_text("slug", self.slug.as_deref(), SHORT_TEXT_MAX)?;
        if self
            .slug
            .as_deref()
            .is_some_and(|slug| slugify(slug) != slug)
        {
            return Err(ValidationError::new(
                "slug",
                "must contain lowercase letters, numbers, and single hyphens only",
            ));
        }
        Ok(())
    }
}
