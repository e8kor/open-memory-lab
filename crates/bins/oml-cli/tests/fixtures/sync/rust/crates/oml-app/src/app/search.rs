//! The search settings' use cases (ADR-0049 d4): an admin key reads which
//! pipelines the deployment serves and sets its tenant's default among them,
//! journalled, so a tenant changes how it is searched without a restart.

