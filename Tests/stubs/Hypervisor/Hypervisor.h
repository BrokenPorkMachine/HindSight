#ifndef VECTORGLASS_TEST_HYPERVISOR_H
#define VECTORGLASS_TEST_HYPERVISOR_H
#include <stddef.h>
#include <stdint.h>

typedef int hv_return_t;
typedef uint64_t hv_vcpu_t;
typedef uint32_t hv_reg_t;
typedef uint32_t hv_sys_reg_t;
typedef uint64_t hv_memory_flags_t;
typedef void *hv_vm_config_t;
typedef void *hv_vcpu_config_t;

typedef struct hv_vcpu_exit {
    uint32_t reason;
} hv_vcpu_exit_t;

enum { HV_SUCCESS = 0 };
enum {
    HV_MEMORY_READ = 1u << 0,
    HV_MEMORY_WRITE = 1u << 1,
    HV_MEMORY_EXEC = 1u << 2
};
enum {
    HV_REG_CPSR = 0,
    HV_REG_PC = 1
};
enum {
    HV_SYS_REG_SP_EL0 = 10,
    HV_SYS_REG_SP_EL1 = 11
};

hv_return_t hv_vm_create(hv_vm_config_t config);
hv_return_t hv_vm_destroy(void);
hv_return_t hv_vm_map(void *uva, uint64_t gpa, size_t size, hv_memory_flags_t flags);
hv_return_t hv_vm_unmap(uint64_t gpa, size_t size);
hv_return_t hv_vcpu_create(hv_vcpu_t *vcpu, hv_vcpu_exit_t **exit_info, hv_vcpu_config_t config);
hv_return_t hv_vcpu_destroy(hv_vcpu_t vcpu);
hv_return_t hv_vcpu_get_reg(hv_vcpu_t vcpu, hv_reg_t reg, uint64_t *value);
hv_return_t hv_vcpu_set_reg(hv_vcpu_t vcpu, hv_reg_t reg, uint64_t value);
hv_return_t hv_vcpu_get_sys_reg(hv_vcpu_t vcpu, hv_sys_reg_t reg, uint64_t *value);
hv_return_t hv_vcpu_run(hv_vcpu_t vcpu);

#endif
