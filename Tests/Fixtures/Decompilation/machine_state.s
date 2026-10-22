// CC0 / public-domain qualification source, assembled by Apple Clang.
.text
.p2align 2
.globl _hs_c_leaf_helper
_hs_c_leaf_helper:
    add x0, x0, #5
    ret
.globl _hs_m_signed_div
_hs_m_signed_div:
    sdiv x0, x0, x1
    ret
.globl _hs_m_word
_hs_m_word:
    add w0, w0, w1
    asr w0, w0, #7
    ret
.globl _hs_m_rotate
_hs_m_rotate:
    ror x0, x0, x1
    ret
.globl _hs_m_loop
_hs_m_loop:
    and x1, x1, #15
    cbz x1, Ldone
Lagain:
    add x0, x0, x0, lsl #1
    eor x0, x0, x1
    sub x1, x1, #1
    cbnz x1, Lagain
Ldone:
    mov x1, #2
    cbz x1, Ldone2
Lagain2:
    add x0, x0, #1
    sub x1, x1, #1
    cbnz x1, Lagain2
Ldone2:
    ret
.globl _hs_m_memory
_hs_m_memory:
    str x1, [x0]
    ldrsb x0, [x0]
    ret
.globl _hs_m_alias
_hs_m_alias:
    stp x1, x2, [x0]
    ldp x0, x1, [x0]
    eor x0, x0, x1
    ret
.globl _hs_m_pair
_hs_m_pair:
    stp x0, x1, [sp, #-16]!
    ldp x2, x3, [sp], #16
    eor x0, x2, x3
    ret
.globl _hs_m_keep
_hs_m_keep:
    movk x0, #0xface, lsl #32
    ubfx x0, x0, #7, #41
    ret
.globl _hs_m_spin
_hs_m_spin:
    b _hs_m_spin
.globl _hs_m_tail
_hs_m_tail:
    b _hs_m_word
.globl _hs_m_call
_hs_m_call:
    bl _hs_m_word
    ret
.globl _hs_m_flagwrite
_hs_m_flagwrite:
    adds x0, x0, #1
    ret
.globl _hs_m_addflags
_hs_m_addflags:
    adds x2, x0, x1
    cset x3, cs
    cset x4, vs
    eor x0, x2, x3
    eor x0, x0, x4, lsl #1
    ret
.globl _hs_m_addcarry
_hs_m_addcarry:
    cmp x3, #1
    adcs x0, x1, x2
    cset x5, cs
    cset x6, eq
    eor x0, x0, x5
    eor x0, x0, x6, lsl #1
    ret
.globl _hs_m_subcarry
_hs_m_subcarry:
    cmp x3, #1
    sbcs x0, x1, x2
    cset x5, cs
    cset x6, vs
    eor x0, x0, x5
    eor x0, x0, x6, lsl #1
    ret
.globl _hs_m_nzcv_roundtrip
_hs_m_nzcv_roundtrip:
    cmp x2, #1
    mrs x3, nzcv
    msr nzcv, xzr
    cset x4, cs
    eor x0, x3, x4
    ret
.globl _hs_m_andflags
_hs_m_andflags:
    ands w0, w0, w1, asr #3
    cset w2, mi
    cset w3, eq
    eor w0, w0, w2
    eor w0, w0, w3, lsl #1
    ret
.globl _hs_m_opaque
_hs_m_opaque:
    .long 0xffffffff
    ret
.globl _hs_m_unpredictable
_hs_m_unpredictable:
    // LDR X0,[X0],#8: writeback destination aliases base.
    .long 0xf8408400
    ret
.globl _hs_c_loop_volatile_store
_hs_c_loop_volatile_store:
    cbz x1, Lvolatile_done
Lvolatile_loop:
    str x1, [x0]
    sub x1, x1, #1
    cbnz x1, Lvolatile_loop
Lvolatile_done:
    mov x0, x1
    ret
.globl _hs_c_loop_volatile_accumulate
_hs_c_loop_volatile_accumulate:
    mov x2, xzr
    mov x3, xzr
    cbz x1, Laccumulate_done
Laccumulate_loop:
    ldr x3, [x0]
    add x3, x3, x3
    str x3, [x0]
    mov x2, x3
    sub x1, x1, #1
    cbnz x1, Laccumulate_loop
Laccumulate_done:
    mov x0, x2
    ret
.globl _hs_c_loop_two_volatile_pointers
_hs_c_loop_two_volatile_pointers:
    mov x3, xzr
    mov x4, xzr
    mov x5, xzr
    cbz x2, Ltwo_pointers_done
Ltwo_pointers_loop:
    ldr x4, [x0]
    ldr x5, [x1]
    add x3, x4, x5
    str x3, [x0]
    str x5, [x1]
    sub x2, x2, #1
    cbnz x2, Ltwo_pointers_loop
Ltwo_pointers_done:
    mov x0, x3
    ret
