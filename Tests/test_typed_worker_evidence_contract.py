from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
def read(p): return (ROOT/p).read_text()

def test_worker_emits_bounded_typed_evidence_sideband_without_changing_stdout_schema():
    worker = read('rust/crates/worker/src/preview.rs')
    assert 'VG_EVIDENCE\\t' in worker
    assert 'VG_EVIDENCE_MANIFEST\\t' in worker
    assert 'evidence_cap=20_000usize' in worker
    assert 'stage_fingerprint' in worker
    assert 'analysis_result' in worker

def test_coordinator_filters_sideband_out_of_diagnostics_and_validates_it():
    supervisor = read('PlatformServices/CoordinatorService/WorkerSupervisor.swift')
    messages = read('Shared/WorkbenchMessages.swift')
    assert 'line.hasPrefix("VG_EVIDENCE\\t")' in supervisor
    assert 'line.hasPrefix("VG_EVIDENCE_MANIFEST\\t")' in supervisor
    assert 'WorkerEvidenceBundle' in messages and 'func validated()' in messages
    assert 'producer == "worker-native-evidence-v1"' in messages

def test_cache_v3_persists_typed_evidence_but_reads_v2():
    store = read('PlatformServices/CoordinatorService/ProjectStore.swift')
    assert 'analysisCacheSchema = 3' in store
    assert 'AnalysisCacheEnvelopeV2' in store
    assert 'let evidence: WorkerEvidenceBundle?' in store
    assert 'evidence: evidence?.validated()' in store

def test_app_prefers_worker_native_evidence_and_keeps_compatibility_supplement():
    model = read('App/WorkbenchModel.swift')
    core = read('App/AnalysisEvidenceCore.swift')
    assert 'workerEvidence: WorkerEvidenceBundle?' in model
    assert 'build(worker: bundle, compatibilitySources: sources)' in model
    assert 'nativeSources' in core and 'missingSources' in core
    assert 'compatibility adapter' in model and 'worker-native evidence' in model
