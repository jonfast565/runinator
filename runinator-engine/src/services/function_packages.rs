//! application service for packaged-function metadata and content-addressed artifacts.
//!
//! The service keeps function-package persistence and artifact access behind an engine boundary;
//! transports supply authorization before calling it.

use std::sync::Arc;

use runinator_blob_core::BlobStore;
use runinator_models::{
    errors::SendableError,
    functions::{
        FunctionAlias, FunctionArtifact, FunctionCatalogEntry, FunctionInvocationTarget,
        FunctionPackage, FunctionPackageDetail, FunctionVersion, FunctionVersionRef,
        NewFunctionVersion,
    },
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, FunctionStore},
};
use uuid::Uuid;

use crate::repository;

/// Provides packaged-function operations for transport adapters.
#[derive(Clone)]
pub struct FunctionPackages<T> {
    store: Arc<T>,
    blobs: Arc<dyn BlobStore>,
}

impl<T> FunctionPackages<T> {
    pub fn new(store: Arc<T>, blobs: Arc<dyn BlobStore>) -> Self {
        Self { store, blobs }
    }
}

impl<T: FunctionStore + DefinitionStore + RuntimeStore> FunctionPackages<T> {
    pub async fn list(&self) -> Result<Vec<FunctionPackage>, SendableError> {
        repository::functions::fetch_packages(self.store.as_ref()).await
    }

    pub async fn catalog(&self) -> Result<Vec<FunctionCatalogEntry>, SendableError> {
        repository::functions::fetch_catalog(self.store.as_ref()).await
    }

    pub async fn fetch_package(
        &self,
        org_id: Option<Uuid>,
        namespace: Option<&str>,
        name: &str,
    ) -> Result<Option<FunctionPackage>, SendableError> {
        Ok(repository::functions::fetch_package_detail(
            self.store.as_ref(),
            org_id,
            namespace,
            name,
        )
        .await?
        .map(|detail| detail.package))
    }

    pub async fn fetch_package_detail(
        &self,
        org_id: Option<Uuid>,
        namespace: Option<&str>,
        name: &str,
    ) -> Result<Option<FunctionPackageDetail>, SendableError> {
        repository::functions::fetch_package_detail(self.store.as_ref(), org_id, namespace, name)
            .await
    }

    pub async fn publish(
        &self,
        request: &NewFunctionVersion,
    ) -> Result<FunctionVersion, SendableError> {
        repository::functions::publish_version(self.store.as_ref(), request).await
    }

    pub async fn archive(&self, package_id: Uuid) -> Result<bool, SendableError> {
        repository::functions::delete_package(self.store.as_ref(), package_id).await
    }

    pub async fn restore(&self, package_id: Uuid) -> Result<bool, SendableError> {
        repository::functions::restore_package(self.store.as_ref(), package_id).await
    }

    pub async fn set_alias(
        &self,
        package_id: Uuid,
        alias: &str,
        target: &FunctionVersionRef,
    ) -> Result<FunctionAlias, SendableError> {
        repository::functions::set_alias(self.store.as_ref(), package_id, alias, target).await
    }

    pub async fn delete_alias(&self, package_id: Uuid, alias: &str) -> Result<bool, SendableError> {
        repository::functions::delete_alias(self.store.as_ref(), package_id, alias).await
    }

    pub async fn newest_version(&self, package_id: Uuid) -> Result<Option<i64>, SendableError> {
        Ok(
            repository::functions::fetch_package_versions(self.store.as_ref(), package_id)
                .await?
                .first()
                .map(|version| version.version),
        )
    }

    pub async fn export_package(
        &self,
        export_id: Uuid,
    ) -> Result<Option<FunctionPackage>, SendableError> {
        repository::functions::fetch_export_package(self.store.as_ref(), export_id).await
    }

    pub async fn resolve_invocation_target(
        &self,
        export_id: Uuid,
    ) -> Result<Option<FunctionInvocationTarget>, SendableError> {
        repository::functions::resolve_invocation_target(self.store.as_ref(), export_id).await
    }

    pub async fn fetch_artifact(
        &self,
        digest: &str,
    ) -> Result<Option<FunctionArtifact>, SendableError> {
        repository::functions::fetch_artifact(self.store.as_ref(), digest).await
    }

    pub async fn put_artifact_if_absent(
        &self,
        digest: &str,
        bytes: Vec<u8>,
    ) -> Result<FunctionArtifact, SendableError> {
        repository::functions::put_artifact_if_absent(
            self.store.as_ref(),
            &self.blobs,
            digest,
            bytes,
        )
        .await
    }

    pub async fn open_artifact(
        &self,
        digest: &str,
    ) -> Result<repository::functions::ArtifactBytes, SendableError> {
        repository::functions::open_artifact(self.store.as_ref(), &self.blobs, digest, None).await
    }

    pub async fn packages_with_artifact(
        &self,
        digest: &str,
    ) -> Result<Vec<FunctionPackage>, SendableError> {
        repository::functions::packages_with_artifact(self.store.as_ref(), digest).await
    }
}
