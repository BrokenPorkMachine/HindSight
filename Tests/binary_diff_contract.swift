import Foundation

func report(functions: String, cfg: String, ir: String, disassembly: String, xrefs: String = "", calls: String = "") -> AnalysisReport {
    AnalysisReport(protocol: 1, kind: "analysis_result", request_id: "test", source_sha256: "digest", function_count: 2, symbol_count: 0, basic_block_count: 2, analyzed_instructions: 4, truncated: false,
        functions: functions, cfg: cfg, ir: ir, strings: "", xrefs: xrefs, call_graph: calls, typed_call_graph: "", call_effect_summaries: "", prototype_conflicts: "", dataflow: "", ssa: "", dominators: "", phi_nodes: "", dominance_frontiers: "", relocations: "", jump_tables: "", metadata: "", runtime_records: "", annotated_disassembly: disassembly, ssa_renamed: "", runtime_graph: "", swift_types: "", selector_xrefs: "", phi_operands: "", objc_classes: "", swift_contexts: "", call_values: "", chained_fixups: "", chained_starts: "", objc_protocols: "", swift_fields: "", swift_field_records: "", interblock_values: "", predecessor_values: "", chained_pointers: "", chained_multistarts: "", arm64e_pointers: "", swift_field_ownership: "", propagated_values: "", indirect_calls: "", dyld_imports: "", dyld_linking: "", softlinking: "", symbolication: "", symbol_resolver: "", apple_symbolication: "", arm64e_bind_symbols: "", dylib_ordinals: "", resolved_imports: "", authenticated_bind_targets: "", indirect_symbols: "", pointer_slots: "", export_trie: "", symbol_provenance: "", linkage_records: "", symbol_callers: "", got_auth_slots: "", import_export_collisions: "", function_intelligence: "", data_references: "", tail_call_candidates: "", function_neighborhoods: "", analysis_quality: "", analysis_insights: "", summary: "")
}

@main
struct BinaryDiffContract {
    static func main() {
        let baseline = report(
            functions: "0x1000 0x1008 8 symbol foo\n0x2000 0x2008 8 symbol old_only",
            cfg: "block B0 0x1000..0x1008\nblock B1 0x2000..0x2008",
            ir: "0x1000 op mov x0, x1\n0x1004 term ret\n0x2000 op nop\n0x2004 term ret",
            disassembly: "0000000000001000 0000000000000000 aa0103e0 mov x0, x1\n0000000000001004 0000000000000004 d65f03c0 ret\n0000000000002000 0000000000000008 d503201f nop\n0000000000002004 000000000000000c d65f03c0 ret"
        )
        let candidate = report(
            functions: "0x3000 0x3008 8 symbol foo\n0x4000 0x4008 8 symbol new_only",
            cfg: "block C0 0x3000..0x3008\nblock C1 0x4000..0x4008",
            ir: "0x3000 op mov x0, x2\n0x3004 term ret\n0x4000 op mov x3, x3\n0x4004 term ret",
            disassembly: "0000000000003000 0000000000000000 aa0203e0 mov x0, x2\n0000000000003004 0000000000000004 d65f03c0 ret\n0000000000004000 0000000000000008 aa0303e3 mov x3, x3\n0000000000004004 000000000000000c d65f03c0 ret"
        )
        let diff = VGBinaryDiffEngine.compare(baselineName: "A", baseline: baseline, candidateName: "B", candidate: candidate)
        precondition(diff.changedCount == 1)
        precondition(diff.addedCount == 1)
        precondition(diff.removedCount == 1)
        let foo = diff.functions.first { $0.displayName == "foo" }!
        precondition(foo.matchReason == "unique symbol")
        precondition(foo.codeChanged)
        precondition(foo.semanticsChanged)
        precondition(foo.baselineAddress == "0x0000000000001000")
        precondition(foo.candidateAddress == "0x0000000000003000")
        print("Binary Diff contract passed: changed=\(diff.changedCount) added=\(diff.addedCount) removed=\(diff.removedCount)")
    }
}
