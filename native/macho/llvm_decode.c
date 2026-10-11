#include "llvm_decode.h"
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
/* LLVM's documented C disassembler ABI. Do not bind to LLVM C++ internals. */
typedef void *(*Create)(const char *, void *, int, void *, void *);
typedef void *(*CreateCPU)(const char *,const char *,const char *,void *,int,void *,void *);
typedef size_t (*Decode)(void *, uint8_t *, uint64_t, uint64_t, char *, size_t);
typedef void (*Dispose)(void *);
typedef int (*Options)(void *, uint64_t);
typedef struct { void *decoder; Decode decode; Dispose dispose; } Context;
static void *library;
static pthread_once_t initialized = PTHREAD_ONCE_INIT;
static void initialize(void) {
 const char *paths[] = {
#ifdef __APPLE__
  "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/libLTO.dylib",
  "/Library/Developer/CommandLineTools/usr/lib/libLTO.dylib",
  "/opt/homebrew/opt/llvm/lib/libLLVM.dylib", "/usr/local/opt/llvm/lib/libLLVM.dylib",
#else
  "libLLVM.so", "libLLVM-20.so", "libLLVM-19.so", "libLLVM-18.so", "libLLVM-17.so", "libLLVM-16.so", "libLLVM-15.so", "libLLVM-14.so",
#endif
  NULL
 };
 for (unsigned i=0; paths[i]; ++i) {
  void *candidate=dlopen(paths[i], RTLD_LOCAL|RTLD_NOW);
  if (!candidate) continue;
  if (!dlsym(candidate,"LLVMCreateDisasm") || !dlsym(candidate,"LLVMDisasmInstruction") || !dlsym(candidate,"LLVMDisasmDispose")) { dlclose(candidate); continue; }
  library=candidate; break;
 }
 if (!library) return;
 void (*lto_init)(void)=(void (*)(void))dlsym(library,"lto_initialize_disassembler");
 if (lto_init) { lto_init(); return; }
 const char *targets[]={"AArch64","X86","ARM"};
 const char *stages[]={"TargetInfo","Target","TargetMC","Disassembler","AsmPrinter"};
 for(unsigned t=0;t<3;t++) for(unsigned s=0;s<5;s++) {
  char name[128]; size_t n=0;
  const char *pieces[]={"LLVMInitialize",targets[t],stages[s]};
  for(unsigned p=0;p<3;p++){size_t k=strlen(pieces[p]);memcpy(name+n,pieces[p],k);n+=k;}name[n]=0;
  void (*init)(void)=(void (*)(void))dlsym(library,name);if(init)init();
 }
}
void *hs_decoder_new(unsigned architecture) {
 pthread_once(&initialized,initialize); if(!library || architecture<1 || architecture>5)return NULL;
 const char *triples[]={"","aarch64-apple-darwin","i386-apple-darwin","x86_64-apple-darwin","armv7-apple-darwin","thumbv7-apple-darwin"};
 Create create=(Create)dlsym(library,"LLVMCreateDisasm");
 Context *c=calloc(1,sizeof(*c));if(!c)return NULL;
 CreateCPU create_cpu=(CreateCPU)dlsym(library,"LLVMCreateDisasmCPUFeatures");
 const char *features=architecture==1?"+v8.5a,+pauth,+mte,+bti":architecture>=4?"+v7,+thumb2":"";
 c->decoder=create_cpu?create_cpu(triples[architecture],"generic",features,NULL,0,NULL,NULL):create(triples[architecture],NULL,0,NULL,NULL);
 if(!c->decoder){free(c);return NULL;}
 c->decode=(Decode)dlsym(library,"LLVMDisasmInstruction");c->dispose=(Dispose)dlsym(library,"LLVMDisasmDispose");
 Options options=(Options)dlsym(library,"LLVMSetDisasmOptions");if(options)options(c->decoder,2);
 return c;
}
size_t hs_decoder_instruction(void *context,const uint8_t *bytes,size_t length,uint64_t address,char *output,size_t capacity) {
 if(!context||!bytes||!length||!output||capacity<2)return 0;
 Context *c=context;output[0]=0;
 size_t n=c->decode(c->decoder,(uint8_t *)bytes,length,address,output,capacity);
 output[capacity-1]=0;return n<=length?n:0;
}
void hs_decoder_free(void *context) {if(context){Context *c=context;c->dispose(c->decoder);free(c);}}
