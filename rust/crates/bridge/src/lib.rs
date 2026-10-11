use std::os::raw::c_char;
use reworkbench_model::ImageOffset;

static CAPABILITIES: &[u8] = b"{\"version\":\"0.5.9\",\"release_channel\":\"stable\",\"abi\":1,\"project_integrity_check\":true,\"case_recovery\":true,\"coordinator_handshake_validation\":true,\"coordinator_session_rehydration\":true,\"beta_diagnostics\":true,\"launch_session_continuity\":true,\"automatic_last_project_reopen\":true,\"stale_launch_session_recovery\":true,\"adaptive_workspace_topology\":true,\"responsive_workbench_chrome\":true,\"overflow_safe_metric_bands\":true,\"experimental_machine_profiles\":true,\"rust_bridge\":true,\"analysis\":true,\"execution\":true,\"runtime_governor\":true,\"header_worker\":true,\"project_notes\":true,\"source_capture\":true,\"macho_layout\":true,\"arm64_preview\":true,\"instruction_pages\":true,\"bounded_page_cache\":true,\"integer_memory_decode\":true,\"function_discovery\":true,\"cfg\":true,\"xrefs\":true,\"call_graph\":true,\"string_scan\":true,\"dataflow\":true,\"function_intelligence\":true,\"pc_relative_data_references\":true,\"analysis_insights\":true,\"bounded_dominator_solver\":true,\"per_function_dominator_budget\":true,\"analysis_worker_signal_diagnostics\":true,\"task_layouts\":true,\"dominators\":true,\"phi_sites\":true,\"runtime_metadata\":true,\"cross_navigation\":true,\"dominance_frontiers\":true,\"runtime_records\":true,\"annotated_disassembly\":true,\"ssa_rename\":true,\"runtime_graph\":true,\"swift_nominal_types\":true,\"selector_xrefs\":true,\"dyld_chained_fixups\":true,\"objc_protocol_graph\":true,\"swift_field_descriptors\":true,\"interblock_value_flow\":true,\"dyld_chained_page_starts\":true,\"swift_field_records\":true,\"predecessor_value_flow\":true,\"evidence_layout\":true,\"dyld_chained_pointer_decode\":true,\"swift_field_ownership\":true,\"iterative_value_propagation\":true,\"indirect_call_resolution\":true,\"resolution_layout\":true,\"navigation_history\":true,\"arm64e_pointer_format_recognition\":true,\"arm64e_pointer_field_decode\":true,\"dyld_multistart_expansion\":true,\"context_navigation_history\":true,\"context_inspector\":true,\"dylib_ordinal_resolution\":true,\"resolved_dyld_imports\":true,\"authenticated_bind_target_composition\":true,\"clickable_evidence_rows\":true,\"indirect_symbol_table\":true,\"lazy_nonlazy_pointer_resolution\":true,\"export_trie\":true,\"symbol_provenance\":true,\"linkage_explorer\":true,\"joined_linkage_records\":true,\"symbol_caller_provenance\":true,\"got_auth_got_reconstruction\":true,\"import_export_collision_analysis\":true,\"selected_symbol_workspace\":true,\"native_debug_server\":true,\"debug_attach_launch\":true,\"debug_registers\":true,\"debug_memory\":true,\"software_breakpoints\":true,\"debug_event_stream\":true,\"debug_workspace\":true,\"debug_thread_selection\":true,\"debug_memory_map\":true,\"debug_module_enumeration\":true,\"debug_backtrace\":true,\"debug_live_disassembly\":true,\"dynamic_analysis_layout\":true,\"debug_stop_context\":true,\"debug_module_slide_resolution\":true,\"debug_pc_follow\":true,\"debug_memory_browser\":true,\"python_scripting\":true,\"plugin_api_v1\":true,\"plugin_discovery\":true,\"plugin_capabilities\":true,\"plugin_api_v2\":true,\"plugin_dock_views\":true,\"plugin_persistent_settings\":true,\"plugin_persistent_state\":true,\"plugin_analysis_passes\":true,\"plugin_debug_filters\":true,\"plugin_hotkeys\":true,\"rewplugin_bundles\":true,\"extensions_platform\":true,\"hypervisor_vm\":true,\"apple_soc_virtualization\":true,\"soc_device_graph\":true,\"hypervisor_re_suite\":true,\"guest_memory_inspector\":true,\"guest_disassembly\":true,\"vm_snapshot_diff\":true,\"guest_search\":true,\"vm_trace\":true,\"guest_breakpoints\":true,\"gpa_device_map\":true,\"dyld_cache_browser\":true,\"dyld_cache_bounded_read\":true,\"dyld_legacy_linking_decode\":true,\"soft_linking_decode\":true,\"static_symbolication_decode\":true,\"unified_symbol_resolver\":true,\"apple_symbolication_framework_decode\":true,\"ipsw_catalog\":true,\"ipsw_import\":true,\"public_firmware_key_lookup\":true,\"iphemeral_software_backend\":true,\"firmware_runtime_lab\":true,\"unified_target_model\":true,\"target_capability_discovery\":true,\"address_pivot\":true,\"session_headers\":true,\"analyst_desk\":true,\"workspace_tabs\":true,\"rich_disassembly\":true,\"focus_mode\":true,\"inspector_modes\":true,\"graph_workspace\":true,\"cfg_canvas\":true,\"graph_minimap\":true,\"synchronized_semantics\":true,\"debugger_graph_highlight\":true,\"dockable_analysis_panes\":true,\"binary_workspace\":true,\"synchronized_code_workspace\":true,\"conservative_lifted_view\":true,\"function_browser\":true,\"xref_inspector\":true,\"instruction_data_inspector\":true,\"runtime_correlation_workspace\":true,\"runtime_stop_history\":true,\"register_delta_tracking\":true,\"runtime_static_slide_correlation\":true,\"runtime_memory_observations\":true,\"runtime_execution_heat\":true,\"trace_explorer\":true,\"observed_transition_timeline\":true,\"register_change_band\":true,\"trace_function_summary\":true,\"nearby_memory_evidence\":true,\"evidence_graph_workspace\":true,\"provenance_graph\":true,\"evidence_confidence_edges\":true,\"analyst_note_nodes\":true,\"runtime_evidence_links\":true,\"investigation_ledger\":true,\"persistent_case_state\":true,\"hypothesis_tracking\":true,\"case_bookmarks\":true,\"session_snapshots\":true,\"vgcase_export\":true,\"case_markdown_export\":true,\"case_json_export\":true,\"case_pdf_export\":true,\"binary_diff_workspace\":true,\"semantic_binary_diff\":true,\"cfg_diff\":true,\"xref_diff\":true,\"layout_resilient_function_matching\":true,\"analysis_job_status\":true,\"analysis_progress_phases\":true,\"bounded_analysis_deadline\":true,\"nonfatal_status_polling\":true,\"modern_macho_metadata_admission\":true,\"encrypted_metadata_analysis\":true,\"data_in_code_barriers\":true,\"analysis_quality_ledger\":true,\"function_neighborhoods\":true,\"cross_function_tail_candidates\":true,\"project_analysis_cache\":true,\"analysis_cache_identity_binding\":true,\"stable_release_channel\":true,\"analysis_scalability_budgets\":true,\"off_main_analysis_decode\":true,\"off_main_continuity_fingerprinting\":true,\"analysis_stage_progress\":true,\"analysis_progress_percent\":true,\"analysis_progress_diagnostic_separation\":true,\"analysis_cache_patch_compatibility\":true,\"analysis_cache_envelope_v2\":true,\"structured_analysis_navigation\":true,\"analysis_triage_workspace\":true,\"correlated_evidence_navigation\":true,\"persistent_analysis_pins\":true,\"recent_evidence_history\":true,\"analysis_change_ledger\":true,\"launch_context_continuity\":true,\"investigation_collections\":true,\"collection_aware_pins\":true,\"evidence_resume\":true,\"collection_search\":true,\"collection_health\":true,\"collection_rename\":true,\"evidence_history_navigation\":true,\"collection_comparison\":true,\"collection_review_baselines\":true,\"collection_evidence_drift\":true,\"investigation_review_mode\":true,\"collection_review_journal\":true,\"collection_workflow_state\":true,\"investigation_handoff\":true,\"review_timeline\":true,\"review_follow_up_queue\":true,\"follow_up_evidence_binding\":true,\"collection_review_summary\":true,\"handoff_follow_up_context\":true,\"universal_macos_binary\":true,\"intel_macos_host_support\":true,\"host_architecture_feature_gating\":true,\"cross_host_arm64_analysis\":true,\"vector_emu_cross_host\":true,\"typed_evidence_core\":true,\"off_main_evidence_index\":true,\"semantic_evidence_identity\":true,\"indexed_evidence_navigation\":true,\"evidence_core_compat_adapter\":true,\"bounded_resident_analysis_reports\":true,\"debounced_launch_session_persistence\":true,\"worker_native_typed_evidence\":true,\"typed_evidence_sideband_v1\":true,\"analysis_cache_envelope_v3\":true,\"analysis_stage_fingerprints\":true,\"typed_evidence_compatibility_merge\":true,\"aging_priority_dispatch\":true,\"bounded_concurrent_task_lanes\":true,\"resource_aware_queueing\":true,\"disconnect_queue_cancellation\":true}\0";

#[no_mangle]
pub extern "C" fn reworkbench_capabilities_json() -> *const c_char {
    CAPABILITIES.as_ptr().cast()
}
#[no_mangle]
pub extern "C" fn reworkbench_abi_version() -> u32 { 1 }

/// Adds an offset, returning false without modifying output on overflow.
/// # Safety
/// If non-null, output must point to a valid aligned writable u64 for the call.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_checked_address_add(base: u64, offset: u64, output: *mut u64) -> bool {
    if output.is_null() { return false; }
    match ImageOffset(base).checked_add(offset) {
        Some(value) => {
            // SAFETY: required by caller contract; null checked above.
            unsafe { output.write(value.0); }
            true
        }
        None => false,
    }
}

use reworkbench_scheduler::{Governor, LeaseId};
use std::ffi::c_void;
#[no_mangle]
pub extern "C" fn reworkbench_governor_new(budget_mib: u64, workers: usize) -> *mut c_void {
    Box::into_raw(Box::new(Governor::new(budget_mib, workers))).cast()
}
/// # Safety
/// Pointer must be a live allocation from governor_new, freed exactly once; no concurrent calls.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_governor_free(governor: *mut c_void) {
    if !governor.is_null() { unsafe { drop(Box::from_raw(governor.cast::<Governor>())); } }
}
/// # Safety
/// Pointer must be a live exclusively accessed governor; zero denotes denied admission.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_governor_admit(governor: *mut c_void, mib: u64) -> u64 {
    unsafe { governor.cast::<Governor>().as_mut() }.and_then(|g|g.admit(mib).ok()).map(|l|l.raw()).unwrap_or(0)
}
/// # Safety
/// Pointer must be a live exclusively accessed governor.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_governor_revoke(governor: *mut c_void, lease: u64) -> bool {
    unsafe { governor.cast::<Governor>().as_mut() }.map(|g|g.revoke(LeaseId::from_raw(lease))).unwrap_or(false)
}
/// # Safety
/// Pointer must be a live exclusively accessed governor.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_governor_may_publish(governor: *mut c_void, lease: u64) -> bool {
    unsafe { governor.cast::<Governor>().as_ref() }.map(|g|g.may_publish(LeaseId::from_raw(lease))).unwrap_or(false)
}

extern "C" {
    fn re_cache_new(bytes: usize, entries: usize) -> *mut c_void;
    fn re_cache_free(cache: *mut c_void);
    fn re_cache_clear(cache: *mut c_void);
    fn re_cache_put(cache: *mut c_void, key: *const u8, key_size: usize, value: *const u8, value_size: usize) -> i32;
    fn re_cache_get(cache: *mut c_void, key: *const u8, key_size: usize, size: *mut usize) -> *const u8;
    fn re_file_stamp(path: *const c_char, out: *mut c_char, capacity: usize) -> i32;
}
#[no_mangle]
pub extern "C" fn reworkbench_cache_new(bytes: usize, entries: usize) -> *mut c_void {
    unsafe { re_cache_new(bytes, entries) }
}
/// # Safety
/// Live queue-confined cache from cache_new; free exactly once.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_cache_free(cache: *mut c_void) { unsafe { re_cache_free(cache) } }
/// # Safety
/// Live queue-confined cache. Invalidates all borrowed values.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_cache_clear(cache: *mut c_void) { unsafe { re_cache_clear(cache) } }
/// # Safety
/// Live exclusive cache and readable key/value buffers of the supplied sizes.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_cache_put(cache: *mut c_void,key: *const u8,key_size: usize,value: *const u8,value_size: usize) -> i32 {
    unsafe { re_cache_put(cache,key,key_size,value,value_size) }
}
/// # Safety
/// Live exclusive cache, readable key and writable size. Copy return bytes before mutation/free.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_cache_get(cache: *mut c_void,key: *const u8,key_size: usize,size: *mut usize) -> *const u8 {
    unsafe { re_cache_get(cache,key,key_size,size) }
}
/// # Safety
/// Path is a terminated C string; output is writable for capacity bytes.
#[no_mangle]
pub unsafe extern "C" fn reworkbench_file_stamp(path: *const c_char,out: *mut c_char,capacity: usize) -> i32 {
    unsafe { re_file_stamp(path,out,capacity) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn overflow_does_not_modify_output() {
        let mut output = 42;
        assert!(!unsafe { reworkbench_checked_address_add(u64::MAX, 1, &mut output) });
        assert_eq!(output, 42);
    }
    #[test] fn null_output_is_rejected() {
        assert!(!unsafe { reworkbench_checked_address_add(1, 2, std::ptr::null_mut()) });
    }
    #[test] fn bridge_string_is_valid_c_utf8() {
        let text = unsafe { std::ffi::CStr::from_ptr(reworkbench_capabilities_json()) };
        assert!(text.to_str().unwrap().contains("\"analysis\":true"));
    }
}
