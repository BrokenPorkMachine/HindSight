// Original HindSight qualification fixture; CC0 / public domain.
.text
.p2align 2
.globl _hs_copy_chain
_hs_copy_chain:
    mov x4, x0
    mov x5, x4
    mov x0, x5
    ret
.globl _hs_frame_spill
_hs_frame_spill:
    sub sp, sp, #16
    str x0, [sp, #8]
    mov x0, #0
    ldr x2, [sp, #8]
    mov x0, x2
    add sp, sp, #16
    ret
.globl _hs_pair_spill
_hs_pair_spill:
    stp x0, x1, [sp, #-16]!
    mov x0, #0
    mov x1, #0
    ldp x2, x3, [sp], #16
    add x0, x2, x3
    ret
.globl _hs_fp_frame
_hs_fp_frame:
    sub sp, sp, #16
    str d0, [sp, #8]
    fmov d0, #1.0
    ldr d1, [sp, #8]
    fmov d0, d1
    add sp, sp, #16
    ret
.globl _hs_preindexed
_hs_preindexed:
    ldr x1, [x0, #8]!
    mov x0, x1
    ret
.globl _hs_load_overlap
_hs_load_overlap:
    ldp x0, x1, [x0]
    add x0, x0, x1
    ret
.globl _hs_csel_same
_hs_csel_same:
    mov x1, x0
    mov x2, x0
    csel x4, x1, x2, eq
    mov x0, x4
    ret
.globl _hs_fcsel_same
_hs_fcsel_same:
    fmov d1, d0
    fcsel d2, d0, d1, eq
    fmov d0, d2
    ret
.globl _hs_xor_zero
_hs_xor_zero:
    eor x4, x0, xzr
    mov x0, x4
    ret
.globl _hs_bic_zero
_hs_bic_zero:
    bic x4, x0, xzr
    mov x0, x4
    ret
.globl _hs_w_overwrite
_hs_w_overwrite:
    mov w0, #1
    ret
.globl _hs_add_sub_zero
_hs_add_sub_zero:
    add x4, x0, xzr
    sub x5, x4, xzr
    add x0, xzr, x5
    ret
.globl _hs_shifted_zero_ops
_hs_shifted_zero_ops:
    eor x4, x0, xzr, lsr #3
    bic x5, x4, xzr, lsl #2
    orr x6, x5, xzr, lsl #2
    mov x0, x6
    ret
.globl _hs_zero_distance_shift
_hs_zero_distance_shift:
    .inst 0xd340fc04 // UBFM x4,x0,#0,#63 (LSR x4,x0,#0)
    .inst 0x9340fc05 // SBFM x5,x0,#0,#63 (ASR x5,x0,#0)
    mov x0, x5
    ret
.globl _hs_w_add_sub_zero
_hs_w_add_sub_zero:
    add w4, w0, wzr
    sub w5, w4, wzr
    orr w6, w5, wzr
    mov w0, w6
    ret
.globl _hs_call_preserved
_hs_call_preserved:
    stp x19, x30, [sp, #-16]!
    mov x19, x0
    bl _hs_call_clobber
    mov x0, x19
    ldp x19, x30, [sp], #16
    ret
.globl _hs_call_clobber
_hs_call_clobber:
    mov x0, #0
    ret
.globl _hs_call_preserved_fp
_hs_call_preserved_fp:
    stp x29, x30, [sp, #-16]!
    mov x29, sp
    str d8, [sp, #-16]!
    fmov d8, d0
    bl _hs_call_clobber
    fmov d0, d8
    ldr d8, [sp], #16
    ldp x29, x30, [sp], #16
    ret
