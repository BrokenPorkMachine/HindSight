//! Admission model used by the Swift worker supervisor. No physical-memory monitoring yet.
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LeaseId(u64);
impl LeaseId { pub fn raw(self)->u64 {self.0} pub fn from_raw(value:u64)->Self {Self(value)} }
#[derive(Debug, PartialEq, Eq)]
pub enum AdmissionError { ZeroRequest, OverBudget, TooManyWorkers, IdExhausted }

pub struct Governor {
    budget_mib: u64,
    max_workers: usize,
    next_id: u64,
    leases: BTreeMap<LeaseId, u64>,
}
impl Governor {
    pub fn new(budget_mib: u64, max_workers: usize) -> Self {
        Self { budget_mib, max_workers, next_id: 1, leases: BTreeMap::new() }
    }
    pub fn reserved_mib(&self) -> u64 { self.leases.values().sum() }
    pub fn admit(&mut self, requested_mib: u64) -> Result<LeaseId, AdmissionError> {
        if requested_mib == 0 { return Err(AdmissionError::ZeroRequest); }
        if self.leases.len() >= self.max_workers { return Err(AdmissionError::TooManyWorkers); }
        if requested_mib > self.budget_mib.saturating_sub(self.reserved_mib()) {
            return Err(AdmissionError::OverBudget);
        }
        let next = self.next_id.checked_add(1).ok_or(AdmissionError::IdExhausted)?;
        let lease = LeaseId(self.next_id);
        self.next_id = next;
        self.leases.insert(lease, requested_mib);
        Ok(lease)
    }
    pub fn revoke(&mut self, lease: LeaseId) -> bool { self.leases.remove(&lease).is_some() }
    pub fn may_publish(&self, lease: LeaseId) -> bool { self.leases.contains_key(&lease) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn cancelled_lease_cannot_publish() {
        let mut g = Governor::new(768, 2);
        let lease = g.admit(384).unwrap();
        assert!(g.may_publish(lease));
        assert!(g.revoke(lease));
        assert!(!g.may_publish(lease));
        assert!(!g.revoke(lease));
    }
    #[test] fn refuses_memory_overcommit() {
        let mut g = Governor::new(768, 2);
        g.admit(512).unwrap();
        assert_eq!(g.admit(257), Err(AdmissionError::OverBudget));
    }
    #[test] fn refuses_extra_worker() {
        let mut g = Governor::new(768, 2);
        g.admit(1).unwrap(); g.admit(1).unwrap();
        assert_eq!(g.admit(1), Err(AdmissionError::TooManyWorkers));
    }
    #[test] fn revocation_releases_budget_without_reusing_id() {
        let mut g = Governor::new(10, 1);
        let a = g.admit(10).unwrap(); g.revoke(a);
        let b = g.admit(10).unwrap();
        assert_ne!(a, b); assert_eq!(g.reserved_mib(), 10);
    }
}

/// Deterministic queue admission for long-running, memory-heavy background jobs.
/// Keep interactive work on the coordinator control lane; this queue only dispatches
/// jobs when both concurrency slots and the reservation budget are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority { Background, Utility, UserInitiated, Interactive }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError { Full, Duplicate, InvalidRequest }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingJob { pub id: u64, pub generation: u64, pub priority: JobPriority, pub memory_mib: u64 }
pub struct PriorityScheduler {
    pub governor: Governor,
    queued: Vec<PendingJob>,
    max_queued: usize,
    active: BTreeMap<u64, (u64, LeaseId)>,
}
impl PriorityScheduler {
    pub fn new(memory_mib: u64, concurrency: usize, max_queued: usize) -> Self {
        Self { governor: Governor::new(memory_mib, concurrency), queued: Vec::new(), max_queued, active: BTreeMap::new() }
    }
    pub fn enqueue(&mut self, job: PendingJob) -> Result<(), QueueError> {
        if job.memory_mib == 0 || job.id == 0 { return Err(QueueError::InvalidRequest); }
        if self.queued.iter().any(|x| x.id == job.id) || self.active.contains_key(&job.id) { return Err(QueueError::Duplicate); }
        if self.queued.len() >= self.max_queued { return Err(QueueError::Full); }
        self.queued.push(job);
        Ok(())
    }
    /// Cancel pending jobs and revoke leases from older generations. Callers must
    /// separately signal running worker processes and drop late replies.
    pub fn cancel_older_than(&mut self, generation: u64) -> Vec<u64> {
        self.queued.retain(|job| job.generation >= generation);
        let stale: Vec<u64> = self.active.iter().filter_map(|(&id, &(gen, _))| if gen < generation { Some(id) } else { None }).collect();
        for id in &stale { self.complete(*id); }
        stale
    }
    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.queued.len();
        self.queued.retain(|j| j.id != id);
        if before != self.queued.len() { return true; }
        self.complete(id)
    }
    pub fn dispatch_next(&mut self) -> Option<PendingJob> {
        // Highest priority first; FIFO for jobs with identical priority.
        // Skip an oversized request instead of allowing it to block smaller jobs.
        let mut indices: Vec<usize> = (0..self.queued.len()).collect();
        indices.sort_by_key(|&i| std::cmp::Reverse(self.queued[i].priority));
        for i in indices {
            let job = self.queued[i];
            if let Ok(lease) = self.governor.admit(job.memory_mib) {
                self.queued.remove(i);
                self.active.insert(job.id, (job.generation, lease));
                return Some(job);
            }
        }
        None
    }
    pub fn complete(&mut self, id: u64) -> bool {
        if let Some((_, lease)) = self.active.remove(&id) { self.governor.revoke(lease) } else { false }
    }
    pub fn can_publish(&self, id: u64, generation: u64) -> bool {
        self.active.get(&id).map(|&(g, lease)| g == generation && self.governor.may_publish(lease)).unwrap_or(false)
    }
    pub fn pending_count(&self) -> usize { self.queued.len() }
}
#[cfg(test)]
mod priority_tests {
    use super::*;
    fn job(id:u64, gen:u64, priority:JobPriority, mib:u64)->PendingJob { PendingJob{id,generation:gen,priority,memory_mib:mib} }
    #[test] fn priority_fifo_and_budget() {
        let mut s=PriorityScheduler::new(256,1,3);
        s.enqueue(job(1,1,JobPriority::Background,64)).unwrap();
        s.enqueue(job(2,1,JobPriority::Interactive,128)).unwrap();
        s.enqueue(job(3,1,JobPriority::Interactive,64)).unwrap();
        assert_eq!(s.dispatch_next().unwrap().id,2);
        assert!(s.dispatch_next().is_none());
        assert!(s.complete(2));
        assert_eq!(s.dispatch_next().unwrap().id,3);
    }
    #[test] fn cancellation_revokes_publication() {
        let mut s=PriorityScheduler::new(256,2,2);
        s.enqueue(job(1,4,JobPriority::Utility,32)).unwrap();
        s.dispatch_next().unwrap();
        assert!(s.can_publish(1,4));
        assert_eq!(s.cancel_older_than(5),vec![1]);
        assert!(!s.can_publish(1,4));
    }
    #[test] fn queue_has_limits_and_unique_ids() {
        let mut s=PriorityScheduler::new(256,1,1);
        s.enqueue(job(1,1,JobPriority::Utility,32)).unwrap();
        assert_eq!(s.enqueue(job(1,1,JobPriority::Utility,32)),Err(QueueError::Duplicate));
        assert_eq!(s.enqueue(job(2,1,JobPriority::Utility,32)),Err(QueueError::Full));
    }
}
