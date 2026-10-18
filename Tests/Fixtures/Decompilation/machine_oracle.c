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
