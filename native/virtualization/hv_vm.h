#ifndef REWORKBENCH_HV_VM_H
#define REWORKBENCH_HV_VM_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rw_hv_vm rw_hv_vm_t;
rw_hv_vm_t *rw_hv_vm_create(uint64_t memory_size, char *error, size_t error_size);
void rw_hv_vm_destroy(rw_hv_vm_t *vm);
int rw_hv_vm_reset(rw_hv_vm_t *vm, char *error, size_t error_size);
uint64_t rw_hv_vm_memory_size(const rw_hv_vm_t *vm);
int rw_hv_vm_is_ready(const rw_hv_vm_t *vm);
int rw_hv_vm_read_memory(const rw_hv_vm_t *vm, uint64_t gpa, void *out, size_t size, char *error, size_t error_size);
int rw_hv_vm_write_memory(rw_hv_vm_t *vm, uint64_t gpa, const void *data, size_t size, char *error, size_t error_size);
int rw_hv_vm_get_pc_sp(const rw_hv_vm_t *vm, uint64_t *pc, uint64_t *sp, char *error, size_t error_size);
int rw_hv_vm_set_pc(rw_hv_vm_t *vm, uint64_t pc, char *error, size_t error_size);
int rw_hv_vm_run_once(rw_hv_vm_t *vm, uint32_t *exit_reason, uint64_t *pc, char *error, size_t error_size);
#ifdef __cplusplus
}
#endif
#endif
