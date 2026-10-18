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
