#include "hv_vm.h"
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#if defined(__APPLE__) && defined(__arm64__)
#include <Hypervisor/Hypervisor.h>

struct rw_hv_vm {
    void *memory;
    uint64_t memory_size;
    hv_vcpu_t vcpu;
    hv_vcpu_exit_t *exit_info;
    int vcpu_created;
    int ready;
};

static void fail(char *error, size_t n, const char *what, hv_return_t rc) {
    if (error && n) snprintf(error, n, "%s (Hypervisor.framework rc=%d)", what, (int)rc);
}
static int bounds_ok(const rw_hv_vm_t *vm, uint64_t gpa, size_t size) {
    if (!vm || !vm->ready) return 0;
    if (gpa > vm->memory_size) return 0;
    return size <= vm->memory_size - gpa;
}

rw_hv_vm_t *rw_hv_vm_create(uint64_t memory_size, char *error, size_t error_size) {
    if (memory_size < (16u << 20) || memory_size > (16ull << 30)) {
        if (error && error_size) snprintf(error, error_size, "VM RAM must be between 16 MiB and 16 GiB.");
        return NULL;
    }
    const uint64_t page = 0x4000;
    memory_size = (memory_size + page - 1) & ~(page - 1);
    rw_hv_vm_t *vm = calloc(1, sizeof(*vm));
    if (!vm) return NULL;
    hv_return_t rc = hv_vm_create(NULL);
    if (rc != HV_SUCCESS) { fail(error,error_size,"hv_vm_create failed",rc); free(vm); return NULL; }
    if (posix_memalign(&vm->memory, page, (size_t)memory_size) != 0) {
        if (error && error_size) snprintf(error,error_size,"Unable to allocate aligned guest RAM.");
        hv_vm_destroy(); free(vm); return NULL;
    }
    memset(vm->memory, 0, (size_t)memory_size);
    rc = hv_vm_map(vm->memory, 0, (size_t)memory_size, HV_MEMORY_READ | HV_MEMORY_WRITE | HV_MEMORY_EXEC);
    if (rc != HV_SUCCESS) { fail(error,error_size,"hv_vm_map failed",rc); free(vm->memory); hv_vm_destroy(); free(vm); return NULL; }
    vm->memory_size = memory_size;
    rc = hv_vcpu_create(&vm->vcpu, &vm->exit_info, NULL);
    if (rc != HV_SUCCESS) { fail(error,error_size,"hv_vcpu_create failed",rc); hv_vm_unmap(0,(size_t)memory_size); free(vm->memory); hv_vm_destroy(); free(vm); return NULL; }
    vm->vcpu_created = 1;
    vm->ready = 1;
    return vm;
}

void rw_hv_vm_destroy(rw_hv_vm_t *vm) {
    if (!vm) return;
    if (vm->vcpu_created) hv_vcpu_destroy(vm->vcpu);
    if (vm->memory) { hv_vm_unmap(0, (size_t)vm->memory_size); free(vm->memory); }
    hv_vm_destroy();
    free(vm);
}

int rw_hv_vm_reset(rw_hv_vm_t *vm, char *error, size_t error_size) {
    if (!vm || !vm->ready) { if (error && error_size) snprintf(error,error_size,"VM is not ready."); return -1; }
    memset(vm->memory, 0, (size_t)vm->memory_size);
    hv_return_t rc = hv_vcpu_set_reg(vm->vcpu, HV_REG_PC, 0);
    if (rc != HV_SUCCESS) { fail(error,error_size,"Unable to reset vCPU PC",rc); return -1; }
    return 0;
}
uint64_t rw_hv_vm_memory_size(const rw_hv_vm_t *vm) { return vm ? vm->memory_size : 0; }
int rw_hv_vm_is_ready(const rw_hv_vm_t *vm) { return vm && vm->ready; }

int rw_hv_vm_read_memory(const rw_hv_vm_t *vm, uint64_t gpa, void *out, size_t size, char *error, size_t error_size) {
    if (!out || !bounds_ok(vm,gpa,size)) { if(error&&error_size) snprintf(error,error_size,"Guest memory read is out of range."); return -1; }
    memcpy(out, (const uint8_t *)vm->memory + gpa, size);
    return 0;
}
int rw_hv_vm_write_memory(rw_hv_vm_t *vm, uint64_t gpa, const void *data, size_t size, char *error, size_t error_size) {
    if ((!data && size) || !bounds_ok(vm,gpa,size)) { if(error&&error_size) snprintf(error,error_size,"Guest memory write is out of range."); return -1; }
    if (size) memcpy((uint8_t *)vm->memory + gpa, data, size);
    return 0;
}
int rw_hv_vm_get_pc_sp(const rw_hv_vm_t *vm, uint64_t *pc, uint64_t *sp, char *error, size_t error_size) {
    if (!vm || !vm->ready || !pc || !sp) { if(error&&error_size) snprintf(error,error_size,"VM/register output is unavailable."); return -1; }
    hv_return_t rc = hv_vcpu_get_reg(vm->vcpu, HV_REG_PC, pc);
    if (rc != HV_SUCCESS) { fail(error,error_size,"Unable to read vCPU PC",rc); return -1; }
    uint64_t cpsr = 0;
    rc = hv_vcpu_get_reg(vm->vcpu, HV_REG_CPSR, &cpsr);
    if (rc != HV_SUCCESS) { fail(error,error_size,"Unable to read vCPU CPSR for stack selection",rc); return -1; }
    /*
     * On Apple silicon Hypervisor.framework, SP is not a general hv_reg_t.
     * The guest-visible stack pointers are system registers. PSTATE.M[0]
     * selects the current-EL stack (EL1h => SP_EL1) versus SP_EL0
     * (EL0t/EL1t). Hypervisor guests execute at EL0/EL1, so this maps the
     * active AArch64 stack pointer without relying on the removed HV_REG_SP.
     */
    const hv_sys_reg_t sp_reg = (cpsr & 1u) ? HV_SYS_REG_SP_EL1 : HV_SYS_REG_SP_EL0;
    rc = hv_vcpu_get_sys_reg(vm->vcpu, sp_reg, sp);
    if (rc != HV_SUCCESS) { fail(error,error_size,"Unable to read active vCPU SP",rc); return -1; }
    return 0;
}
int rw_hv_vm_set_pc(rw_hv_vm_t *vm, uint64_t pc, char *error, size_t error_size) {
    if (!vm || !vm->ready || (pc & 3u)) { if(error&&error_size) snprintf(error,error_size,"PC must be 4-byte aligned in an active VM."); return -1; }
    hv_return_t rc = hv_vcpu_set_reg(vm->vcpu, HV_REG_PC, pc);
    if (rc != HV_SUCCESS) { fail(error,error_size,"Unable to set vCPU PC",rc); return -1; }
    return 0;
}
int rw_hv_vm_run_once(rw_hv_vm_t *vm, uint32_t *exit_reason, uint64_t *pc, char *error, size_t error_size) {
    if (!vm || !vm->ready || !exit_reason || !pc) { if(error&&error_size) snprintf(error,error_size,"VM/run output is unavailable."); return -1; }
    hv_return_t rc = hv_vcpu_run(vm->vcpu);
    if (rc != HV_SUCCESS) { fail(error,error_size,"hv_vcpu_run failed",rc); return -1; }
    *exit_reason = vm->exit_info ? (uint32_t)vm->exit_info->reason : UINT32_MAX;
    rc = hv_vcpu_get_reg(vm->vcpu, HV_REG_PC, pc);
    if (rc != HV_SUCCESS) { fail(error,error_size,"Unable to read PC after VM exit",rc); return -1; }
    return 0;
}

#else
struct rw_hv_vm { int unused; };
rw_hv_vm_t *rw_hv_vm_create(uint64_t memory_size, char *error, size_t error_size) { (void)memory_size; if(error&&error_size) snprintf(error,error_size,"Apple Silicon Hypervisor.framework host required."); return NULL; }
void rw_hv_vm_destroy(rw_hv_vm_t *vm) { free(vm); }
int rw_hv_vm_reset(rw_hv_vm_t *vm, char *error, size_t error_size) { (void)vm; if(error&&error_size) snprintf(error,error_size,"Unsupported host."); return -1; }
uint64_t rw_hv_vm_memory_size(const rw_hv_vm_t *vm) { (void)vm; return 0; }
int rw_hv_vm_is_ready(const rw_hv_vm_t *vm) { (void)vm; return 0; }
int rw_hv_vm_read_memory(const rw_hv_vm_t *vm,uint64_t gpa,void*out,size_t size,char*error,size_t n){(void)vm;(void)gpa;(void)out;(void)size;if(error&&n)snprintf(error,n,"Unsupported host.");return -1;}
int rw_hv_vm_write_memory(rw_hv_vm_t *vm,uint64_t gpa,const void*data,size_t size,char*error,size_t n){(void)vm;(void)gpa;(void)data;(void)size;if(error&&n)snprintf(error,n,"Unsupported host.");return -1;}
int rw_hv_vm_get_pc_sp(const rw_hv_vm_t *vm,uint64_t*pc,uint64_t*sp,char*error,size_t n){(void)vm;(void)pc;(void)sp;if(error&&n)snprintf(error,n,"Unsupported host.");return -1;}
int rw_hv_vm_set_pc(rw_hv_vm_t *vm,uint64_t pc,char*error,size_t n){(void)vm;(void)pc;if(error&&n)snprintf(error,n,"Unsupported host.");return -1;}
int rw_hv_vm_run_once(rw_hv_vm_t *vm,uint32_t*reason,uint64_t*pc,char*error,size_t n){(void)vm;(void)reason;(void)pc;if(error&&n)snprintf(error,n,"Unsupported host.");return -1;}
#endif
