/* CC0. Execute only the test-owned fixtures, never user-imported binaries. */
#include <dlfcn.h>
#include <inttypes.h>
#include <objc/runtime.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef uint64_t (*function)(uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t);
int main(int argc,char **argv) {
    if(argc!=3)return 2;
    void *image=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL);if(!image)return 3;
    function target;
    if(!strcmp(argv[2],"objc")) {
        Method m=class_getInstanceMethod(objc_getClass("HSMachineStateFixture"),sel_registerName("mix:with:"));
        if(!m)return 4;target=(function)method_getImplementation(m);
    } else target=(function)dlsym(image,argv[2]);
    if(!target)return 5;
    uint64_t x[8];
    while(scanf("%"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64,
          &x[0],&x[1],&x[2],&x[3],&x[4],&x[5],&x[6],&x[7])==8) {
        unsigned char memory[4096]={0};
        if(!strcmp(argv[2],"hs_c_load")||!strcmp(argv[2],"hs_c_load_volatile_u32")||!strcmp(argv[2],"hs_c_load_volatile_twice")||!strcmp(argv[2],"hs_c_load_volatile_branch")||!strcmp(argv[2],"hs_c_volatile_load_then_call")||!strcmp(argv[2],"hs_c_call_then_volatile_load")||!strcmp(argv[2],"hs_c_load_signed16")||!strcmp(argv[2],"hs_c_load_signed32")||!strcmp(argv[2],"hs_c_load_signed64")||!strcmp(argv[2],"hs_c_load_unsigned8")||!strcmp(argv[2],"hs_c_load_unsigned16")||!strcmp(argv[2],"hs_c_load_signed_offset")||!strcmp(argv[2],"hs_c_load_word_offset")) {
            uint64_t values[2]={x[1],x[2]};
            x[0]=(uint64_t)(uintptr_t)&values[0];
            uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" \n",result);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_load_dynamic")||!strcmp(argv[2],"hs_c_load_dynamic_s16")||!strcmp(argv[2],"hs_c_load_dynamic_u32")) {
            uint64_t values[4]={x[2],x[3],x[4],x[5]};
            x[0]=(uint64_t)(uintptr_t)(const uint8_t *)&values[0];
            uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64"\n",result);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_recursive_store")) {
            uint64_t value=0;x[1]%=16;x[0]=(uint64_t)(uintptr_t)&value;
            uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" %016"PRIx64"\n",result,value);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_call_then_load")) {
            uint64_t value=x[1];x[0]=(uint64_t)(uintptr_t)&value;
            printf("%016"PRIx64"\n",target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]));
            continue;
        }
        if(!strcmp(argv[2],"hs_c_loop_two_volatile_pointers")) {
            uint64_t values[2]={x[3],x[4]};
            uint64_t *left=&values[0], *right=(x[0]&1)?&values[0]:&values[1];
            uint64_t result=target((uint64_t)(uintptr_t)left,(uint64_t)(uintptr_t)right,x[2]%8,x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" %016"PRIx64" %016"PRIx64"\n",result,values[0],values[1]);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_store")||!strcmp(argv[2],"hs_c_store_volatile")||!strcmp(argv[2],"hs_c_store_volatile_twice")||!strcmp(argv[2],"hs_c_store_u16")||!strcmp(argv[2],"hs_c_store_u32")||!strcmp(argv[2],"hs_c_store_s16")||!strcmp(argv[2],"hs_c_store_s32")||!strcmp(argv[2],"hs_c_store_twice")||!strcmp(argv[2],"hs_c_store_conditional")||!strcmp(argv[2],"hs_c_store_then_call")||!strcmp(argv[2],"hs_c_volatile_store_then_call")||!strcmp(argv[2],"hs_c_call_then_volatile_store")||!strcmp(argv[2],"hs_c_volatile_store_call_load")||!strcmp(argv[2],"hs_c_loop_volatile_store")||!strcmp(argv[2],"hs_c_loop_volatile_accumulate")||!strcmp(argv[2],"hs_c_branch_store_call")||!strcmp(argv[2],"hs_c_call_then_store")||!strcmp(argv[2],"hs_c_call_then_store_branch")||!strcmp(argv[2],"hs_c_conditional_call_store")) {
            uint64_t values[4]={x[2],0,x[3],0};
            if(!strcmp(argv[2],"hs_c_loop_volatile_store")||!strcmp(argv[2],"hs_c_loop_volatile_accumulate"))x[1]%=8;
            x[0]=(uint64_t)(uintptr_t)&values[0];
            uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            if(!strcmp(argv[2],"hs_c_store_twice"))printf("%016"PRIx64" %016"PRIx64" %016"PRIx64"\n",result,values[0],values[2]);
            else printf("%016"PRIx64" %016"PRIx64"\n",result,values[0]);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_store_dynamic")) {
            uint8_t memory[16];for(unsigned i=0;i<16;i++)memory[i]=(uint8_t)(x[3]+i*13);
            x[0]=(uint64_t)(uintptr_t)memory;
            uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" ",result);for(unsigned i=0;i<16;i++)printf("%02x",memory[i]);puts("");
            continue;
        }
        if(!strcmp(argv[2],"hs_c_store_dynamic_u32")) {
            uint32_t values[4]={(uint32_t)x[1],(uint32_t)x[2],(uint32_t)x[3],(uint32_t)x[4]};
            uint32_t result=target((uint64_t)(uintptr_t)&values[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" %08"PRIx32" %08"PRIx32" %08"PRIx32" %08"PRIx32"\n",(uint64_t)result,values[0],values[1],values[2],values[3]);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_store_dynamic_s16")) {
            int16_t values[4]={(int16_t)x[1],(int16_t)x[2],(int16_t)x[3],(int16_t)x[4]};
            int16_t result=target((uint64_t)(uintptr_t)&values[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" %04"PRIx16" %04"PRIx16" %04"PRIx16" %04"PRIx16"\n",(uint64_t)result,(uint16_t)values[0],(uint16_t)values[1],(uint16_t)values[2],(uint16_t)values[3]);
            continue;
        }
        if(!strcmp(argv[2],"hs_c_store_then_load")||!strcmp(argv[2],"hs_c_load_then_store")||!strcmp(argv[2],"hs_c_conditional_store_then_load")||!strcmp(argv[2],"hs_c_alternate_store_then_load")||!strcmp(argv[2],"hs_c_store_load_then_call")||!strcmp(argv[2],"hs_c_call_load_store")) {
            uint64_t values[2]={x[3],x[4]};
            uint64_t *target_ptr=&values[0];
            const uint64_t *source=(x[0]&1)?&values[0]:&values[1];
            x[0]=(uint64_t)(uintptr_t)target_ptr;
            x[1]=(uint64_t)(uintptr_t)source;
            uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
            printf("%016"PRIx64" %016"PRIx64" %016"PRIx64"\n",result,values[0],values[1]);
            continue;
        }
        if(!strcmp(argv[2],"hs_m_memory")||!strcmp(argv[2],"hs_m_alias")) {
            if(x[0]<0x100000 || x[0]>0x100ff0)return 6;
            x[0]=(uint64_t)(uintptr_t)(memory+x[0]-0x100000);
        }
        uint64_t result=target(x[0],x[1],x[2],x[3],x[4],x[5],x[6],x[7]);
        printf("%016"PRIx64" ",result);
        if(!strcmp(argv[2],"hs_m_memory")||!strcmp(argv[2],"hs_m_alias"))for(unsigned i=0;i<4096;i++)printf("%02x",memory[i]);
        printf("\n");
    }
    return 0;
}
