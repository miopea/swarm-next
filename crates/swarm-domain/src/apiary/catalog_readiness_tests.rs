use super::{
    ApiaryId, FederationCatalogAcknowledgement, FederationCatalogBlocker,
    FederationCatalogReadiness, FederationProjectManifestEntry, FederationProjectReadiness,
    JiraConnectionState, JiraProjectBindingId,
};

fn project(id: &str, ready: bool) -> FederationProjectReadiness {
    FederationProjectReadiness {
        project: FederationProjectManifestEntry {
            project_id: id.into(),
            project_key: id.into(),
            project_name: id.into(),
        },
        binding_id: ready.then(JiraProjectBindingId::new),
        access_verified: ready,
        workflow_mapped: ready,
    }
}

fn catalog(projects: Vec<FederationProjectReadiness>) -> FederationCatalogReadiness {
    FederationCatalogReadiness::evaluate(
        Some(FederationCatalogAcknowledgement {
            apiary_id: ApiaryId::new(),
            policy_revision: 1,
            promoted_project_catalog_digest: "test-digest".into(),
            project_count: projects.len(),
            snapshot_issued_at: 10,
            snapshot_expires_at: 100,
            acknowledged_at: 11,
        }),
        1,
        JiraConnectionState::Ready,
        projects,
        50,
    )
}

#[test]
fn adding_inaccessible_department_preserves_existing_project_readiness() {
    let before = catalog(vec![project("DEV", true)]);
    let after = catalog(vec![project("DEV", true), project("IT", false)]);
    assert!(before.is_ready());
    assert!(after.is_ready());
    assert_eq!(after.projects.len(), 2);
    assert!(!after.projects[1].is_ready());
}

#[test]
fn losing_last_eligible_project_requires_setup_and_recovers_in_place() {
    let blocked = catalog(vec![project("DEV", false), project("IT", false)]);
    assert_eq!(
        blocked.blockers,
        vec![FederationCatalogBlocker::ProjectAccessNotReady]
    );
    assert!(catalog(vec![project("DEV", true), project("IT", false)]).is_ready());
    // An empty shared catalog is still a valid coordination-only setup.
    assert!(catalog(Vec::new()).is_ready());
}

#[test]
fn partial_project_readiness_does_not_bypass_catalog_or_connection_checks() {
    let ready = catalog(vec![project("DEV", true), project("IT", false)]);
    let stale = FederationCatalogReadiness::evaluate(
        ready.acknowledgement.clone(),
        2,
        JiraConnectionState::NotConnected,
        ready.projects.clone(),
        100,
    );
    assert_eq!(
        stale.blockers,
        vec![
            FederationCatalogBlocker::CatalogStale,
            FederationCatalogBlocker::PolicyRevisionChanged,
            FederationCatalogBlocker::IntegrationNotReady,
        ]
    );
    let missing = FederationCatalogReadiness::evaluate(
        None,
        1,
        JiraConnectionState::Ready,
        ready.projects,
        50,
    );
    assert_eq!(
        missing.blockers,
        vec![FederationCatalogBlocker::CatalogMissing]
    );
}
