//! The in-process bounds on imports and exports (ADR-0084 I13): one import
//! or dry run per tenant and at most [`IMPORTS_AT_ONCE`] in the process; one
//! export per tenant. Each run holds a [`BusyGuard`] while it reads a
//! bundle into memory, and dropping the guard frees its place. The bounds
//! are this process's alone, which is the lead's (ADR-0006 d5).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

/// Imports and dry runs the process runs at once, across tenants.
pub const IMPORTS_AT_ONCE: usize = 2;

/// What a gate admits: the tenants running one, and the imports running in
/// this process, by id, which the sweep never ends as stopped (I11).
#[derive(Debug, Default)]
struct Running {
    tenants: BTreeSet<Uuid>,
    imports: BTreeSet<Uuid>,
}

/// One kind of run's bound: one per tenant, and at most `at_once` in all.
#[derive(Debug, Clone)]
pub struct BusyGate {
    running: Arc<Mutex<Running>>,
    at_once: usize,
}

impl BusyGate {
    #[must_use]
    pub fn new(at_once: usize) -> Self {
        Self {
            running: Arc::default(),
            at_once,
        }
    }

    /// A place for one run of `tenant_id`'s, or `None` when the tenant runs
    /// one already or the process runs `at_once`.
    #[must_use]
    pub fn try_enter(&self, tenant_id: Uuid) -> Option<BusyGuard> {
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if running.tenants.contains(&tenant_id) || running.tenants.len() >= self.at_once {
            return None;
        }
        running.tenants.insert(tenant_id);
        Some(BusyGuard {
            running: Arc::clone(&self.running),
            tenant_id,
            import_id: None,
        })
    }

    /// Whether this process runs the import `import_id` now.
    #[must_use]
    pub fn runs(&self, import_id: Uuid) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .imports
            .contains(&import_id)
    }

    /// The imports this process runs now.
    #[must_use]
    pub fn running_imports(&self) -> Vec<Uuid> {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .imports
            .iter()
            .copied()
            .collect()
    }
}

/// A run's place in its gate; dropping it frees the place.
#[derive(Debug)]
pub struct BusyGuard {
    running: Arc<Mutex<Running>>,
    tenant_id: Uuid,
    import_id: Option<Uuid>,
}

impl BusyGuard {
    /// Names the import this place runs, so the sweep leaves it running.
    pub fn running_import(&mut self, import_id: Uuid) {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .imports
            .insert(import_id);
        self.import_id = Some(import_id);
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        running.tenants.remove(&self.tenant_id);
        if let Some(id) = self.import_id {
            running.imports.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tenant_runs_one_at_a_time_and_the_process_runs_at_most_its_bound() {
        let gate = BusyGate::new(2);
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let first = gate.try_enter(a).expect("a's first run enters");
        assert!(gate.try_enter(a).is_none(), "a second run of a's waits");
        let second = gate.try_enter(b).expect("b enters beside a");
        assert!(gate.try_enter(c).is_none(), "a third tenant is over the bound");
        drop(first);
        assert!(gate.try_enter(c).is_some(), "a freed place admits c");
        drop(second);
    }

    #[test]
    fn a_named_import_is_running_until_its_guard_drops() {
        let gate = BusyGate::new(2);
        let id = Uuid::from_u128(9);
        let mut guard = gate.try_enter(Uuid::from_u128(1)).expect("enters");
        guard.running_import(id);
        assert!(gate.runs(id));
        drop(guard);
        assert!(!gate.runs(id));
    }
}
