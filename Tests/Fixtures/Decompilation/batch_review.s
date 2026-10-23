// CC0: test-owned sequential app reconstruction fixture.
.text
.p2align 2
.globl _hs_batch_identity
_hs_batch_identity:
    mov x0, x1
    ret
.globl _hs_batch_sum
_hs_batch_sum:
    add x0, x0, x1
    ret
.globl _hs_batch_zero
_hs_batch_zero:
    mov x0, #0
    ret
