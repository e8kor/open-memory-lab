//! The search settings' use cases (ADR-0049 d4): an admin key reads which
//! pipelines the deployment serves and sets its tenant's default among them,
//! journalled, so a tenant changes how it is searched without a restart.

use serde_json::json;

use oml_domain::{Role, event_types};

use super::{App, require_role};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::search::{Pipeline, PostingsHealth};
use crate::types::{AppError, Bearer};

/// What `GET /v2/search` answers: the deployment's served pipelines and
/// default, the tenant's journalled default, and the one a recall naming
/// none runs now (ADR-0049 d4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSettings {
    pub enabled: Vec<Pipeline>,
    pub deployment_default: Pipeline,
    /// The tenant's journalled default, by name, as journalled: a name the
    /// deployment stopped serving stays here and is not `effective`.
    pub tenant_default: Option<String>,
    pub effective: Pipeline,
    /// The tenant's postings health, read by `GET /v2/search` where the
    /// deployment serves `bm25-postings` and the store keeps postings, for
    /// doctor (ADR-0052 d12); `None` otherwise.
    pub postings: Option<PostingsHealth>,
    /// The tenant's valid time, read where the deployment serves a time
    /// pipeline, for doctor (proposed ADR-0059 d12); `None` otherwise.
    pub valid_time: Option<crate::search::ValidTimeCounts>,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `GET /v2/search`, admin by key role (ADR-0049 d4).
    pub async fn search_settings(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<SearchSettings, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "search_settings")?;
        let mut settings = self.settings_of(admin.tenant_id).await?;
        if self.search.serves(Pipeline::Bm25Postings) || self.search.needs_sparse() {
            settings.postings = self.journal.postings_health(admin.tenant_id).await?;
        }
        if self
            .search
            .enabled()
            .iter()
            .any(|p| p.retrievers().contains(&crate::search::RetrieverKind::Time))
        {
            let now = self.clock.now().unix_micros();
            settings.valid_time = self.journal.valid_time_counts(admin.tenant_id, now).await?;
        }
        Ok(settings)
    }

    /// `POST /v2/search/default`, admin by key role: the tenant's default
    /// becomes `pipeline`, which the deployment must serve, or, with `None`,
    /// follows the deployment's again; journalled as `search.default_set`
    /// with the name and the admin key (ADR-0049 d4). Setting the default it
    /// already has journals nothing.
    pub async fn set_search_default(
        &self,
        bearer: impl Into<Bearer<'_>>,
        pipeline: Option<&str>,
    ) -> Result<SearchSettings, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "search_default")?;
        let tenant_id = admin.tenant_id;
        let chosen = match pipeline {
            None => None,
            Some(name) => Some(
                Pipeline::parse(name)
                    .filter(|p| self.search.serves(*p))
                    .ok_or_else(|| AppError::SearchUnavailable {
                        name: name.to_owned(),
                        served: self.served_pipelines(),
                    })?,
            ),
        };
        let name = chosen.map(|p| p.as_str().to_owned());
        if self.journal.tenant_search_default(tenant_id).await? == name {
            return self.settings_of(tenant_id).await;
        }
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::SEARCH_DEFAULT_SET,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "pipeline": name,
                "by_key_id": admin.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::SearchDefaultSet { pipeline: name },
            None,
        )
        .await?;
        self.settings_of(tenant_id).await
    }

    async fn settings_of(&self, tenant_id: uuid::Uuid) -> Result<SearchSettings, AppError> {
        let tenant_default = self.journal.tenant_search_default(tenant_id).await?;
        let effective = tenant_default
            .as_deref()
            .and_then(Pipeline::parse)
            .filter(|p| self.search.serves(*p))
            .unwrap_or_else(|| self.search.default_pipeline());
        Ok(SearchSettings {
            enabled: self.search.enabled().to_vec(),
            deployment_default: self.search.default_pipeline(),
            tenant_default,
            effective,
            postings: None,
            valid_time: None,
        })
    }
}
