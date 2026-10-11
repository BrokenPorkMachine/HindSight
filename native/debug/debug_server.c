#define _DARWIN_C_SOURCE 1
#include "debug_server.h"

#if !defined(__APPLE__) || !defined(__arm64__)
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void unsupported(char *err, size_t cap) {
    if (err && cap) snprintf(err, cap, "Native ARM64 debugging requires the arm64 HindSight slice on Apple Silicon.");
}
static int unsupported_text(char **out, char *err, size_t cap) {
    if (out) *out = NULL;
    unsupported(err, cap);
    return -1;
}
int rw_debug_attach(int pid, char *err, size_t cap){(void)pid;unsupported(err,cap);return -1;}
int rw_debug_launch(const char *path,int *pid_out,char *err,size_t cap){(void)path;if(pid_out)*pid_out=0;unsupported(err,cap);return -1;}
int rw_debug_detach(char *err,size_t cap){unsupported(err,cap);return -1;}
int rw_debug_continue(char *err,size_t cap){unsupported(err,cap);return -1;}
int rw_debug_interrupt(char *err,size_t cap){unsupported(err,cap);return -1;}
int rw_debug_step(char *err,size_t cap){unsupported(err,cap);return -1;}
int rw_debug_set_breakpoint(unsigned long long address,char *err,size_t cap){(void)address;unsupported(err,cap);return -1;}
int rw_debug_remove_breakpoint(unsigned long long address,char *err,size_t cap){(void)address;unsupported(err,cap);return -1;}
int rw_debug_read_memory(unsigned long long address,size_t length,char **out,char *err,size_t cap){(void)address;(void)length;return unsupported_text(out,err,cap);}
int rw_debug_snapshot(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
int rw_debug_poll_event(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
int rw_debug_threads(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
int rw_debug_select_thread(unsigned long long thread_id,char *err,size_t cap){(void)thread_id;unsupported(err,cap);return -1;}
int rw_debug_memory_map(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
int rw_debug_modules(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
int rw_debug_backtrace(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
int rw_debug_disassembly(char **out,char *err,size_t cap){return unsupported_text(out,err,cap);}
void rw_debug_free_string(char *p){free(p);}

#else
#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <mach/arm/thread_status.h>
#include <sys/ptrace.h>
#include <sys/wait.h>
#include <signal.h>
#include <spawn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <stdarg.h>
#include <libgen.h>
#include <mach-o/dyld_images.h>
#include <mach-o/loader.h>
#include "../macho/decode.h"

extern char **environ;

typedef struct { mach_vm_address_t address; uint32_t original; int active; } rw_breakpoint_t;
typedef struct {
    pid_t pid;
    mach_port_t task;
    int attached;
    int stopped;
    rw_breakpoint_t breakpoints[256];
    size_t breakpoint_count;
    uint64_t selected_thread_id;
} rw_debug_state_t;
static rw_debug_state_t g = {0};

static void seterr(char *err, size_t cap, const char *fmt, ...) {
    if (!err || cap == 0) return;
    va_list ap; va_start(ap, fmt); vsnprintf(err, cap, fmt, ap); va_end(ap);
}
static void reset_state(void) {
    if (g.task != MACH_PORT_NULL) mach_port_deallocate(mach_task_self(), g.task);
    memset(&g, 0, sizeof(g));
}
static int refresh_task(char *err, size_t cap) {
    if (!g.attached || g.pid <= 0) { seterr(err, cap, "No debug session is attached."); return -1; }
    if (g.task != MACH_PORT_NULL) return 0;
    kern_return_t kr = task_for_pid(mach_task_self(), g.pid, &g.task);
    if (kr != KERN_SUCCESS) {
        seterr(err, cap, "task_for_pid(%d) failed: %s. The target may require Developer Mode/get-task-allow or elevated authorization.", g.pid, mach_error_string(kr));
        return -1;
    }
    return 0;
}
static int wait_stop(pid_t pid, char *err, size_t cap) {
    int status = 0;
    pid_t r;
    do { r = waitpid(pid, &status, WUNTRACED); } while (r < 0 && errno == EINTR);
    if (r != pid) { seterr(err, cap, "waitpid failed: %s", strerror(errno)); return -1; }
    if (WIFEXITED(status) || WIFSIGNALED(status)) { seterr(err, cap, "Target exited while attaching."); return -1; }
    g.stopped = WIFSTOPPED(status) ? 1 : 0;
    return 0;
}
int rw_debug_attach(int pid, char *err, size_t cap) {
    if (pid <= 1) { seterr(err, cap, "Refusing to attach to PID %d.", pid); return -1; }
    if (g.attached) { seterr(err, cap, "A debug session is already active."); return -1; }
    if (ptrace(PT_ATTACHEXC, pid, (caddr_t)0, 0) != 0) { seterr(err, cap, "ptrace attach failed: %s", strerror(errno)); return -1; }
    g.pid = pid; g.attached = 1;
    if (wait_stop(pid, err, cap) != 0) { reset_state(); return -1; }
    /* task_for_pid can legitimately fail while ptrace control remains usable. Snapshot/memory calls report it precisely. */
    (void)refresh_task(NULL, 0);
    return 0;
}
int rw_debug_launch(const char *path, int *pid_out, char *err, size_t cap) {
    if (!path || path[0] != '/') { seterr(err, cap, "Launch path must be absolute."); return -1; }
    if (g.attached) { seterr(err, cap, "A debug session is already active."); return -1; }
    posix_spawnattr_t attr; posix_spawnattr_init(&attr);
    short flags = 0;
#ifdef POSIX_SPAWN_START_SUSPENDED
    flags |= POSIX_SPAWN_START_SUSPENDED;
#endif
    posix_spawnattr_setflags(&attr, flags);
    char *const argv[] = {(char *)path, NULL};
    pid_t pid = 0;
    int rc = posix_spawn(&pid, path, NULL, &attr, argv, environ);
    posix_spawnattr_destroy(&attr);
    if (rc != 0) { seterr(err, cap, "posix_spawn failed: %s", strerror(rc)); return -1; }
    if (ptrace(PT_ATTACHEXC, pid, (caddr_t)0, 0) != 0) {
        kill(pid, SIGKILL); seterr(err, cap, "Launched PID %d but debugger attach failed: %s", pid, strerror(errno)); return -1;
    }
    g.pid = pid; g.attached = 1;
    if (wait_stop(pid, err, cap) != 0) { kill(pid, SIGKILL); reset_state(); return -1; }
    (void)refresh_task(NULL, 0);
    if (pid_out) *pid_out = pid;
    return 0;
}
int rw_debug_detach(char *err, size_t cap) {
    if (!g.attached) { reset_state(); return 0; }
    for (size_t i=0;i<g.breakpoint_count;i++) if (g.breakpoints[i].active) {
        char tmp[256]={0}; (void)rw_debug_remove_breakpoint(g.breakpoints[i].address, tmp, sizeof(tmp));
    }
    if (ptrace(PT_DETACH, g.pid, (caddr_t)1, 0) != 0 && errno != ESRCH) { seterr(err, cap, "ptrace detach failed: %s", strerror(errno)); return -1; }
    reset_state(); return 0;
}
int rw_debug_continue(char *err, size_t cap) {
    if (!g.attached) { seterr(err, cap, "No debug session."); return -1; }
    if (ptrace(PT_CONTINUE, g.pid, (caddr_t)1, 0) != 0) { seterr(err, cap, "continue failed: %s", strerror(errno)); return -1; }
    g.stopped = 0; return 0;
}
int rw_debug_interrupt(char *err, size_t cap) {
    if (!g.attached) { seterr(err, cap, "No debug session."); return -1; }
    if (kill(g.pid, SIGSTOP) != 0) { seterr(err, cap, "SIGSTOP failed: %s", strerror(errno)); return -1; }
    return wait_stop(g.pid, err, cap);
}
int rw_debug_step(char *err, size_t cap) {
    if (!g.attached) { seterr(err, cap, "No debug session."); return -1; }
    if (ptrace(PT_STEP, g.pid, (caddr_t)1, 0) != 0) { seterr(err, cap, "single-step failed: %s", strerror(errno)); return -1; }
    g.stopped = 0; return 0;
}
static int read_u32(mach_vm_address_t address, uint32_t *out, char *err, size_t cap) {
    if (refresh_task(err,cap)!=0) return -1;
    mach_vm_size_t got=0; kern_return_t kr=mach_vm_read_overwrite(g.task,address,4,(mach_vm_address_t)out,&got);
    if (kr!=KERN_SUCCESS || got!=4) { seterr(err,cap,"read @0x%llx failed: %s",address,mach_error_string(kr)); return -1; }
    return 0;
}
static int write_u32(mach_vm_address_t address, uint32_t value, char *err, size_t cap) {
    if (refresh_task(err,cap)!=0) return -1;
    vm_size_t ps=(vm_size_t)getpagesize(); mach_vm_address_t page=address & ~((mach_vm_address_t)ps-1);
    kern_return_t kr=mach_vm_protect(g.task,page,ps,FALSE,VM_PROT_READ|VM_PROT_WRITE|VM_PROT_COPY);
    if (kr!=KERN_SUCCESS) { seterr(err,cap,"protect @0x%llx failed: %s",page,mach_error_string(kr)); return -1; }
    kr=mach_vm_write(g.task,address,(vm_offset_t)&value,4);
    kern_return_t kr2=mach_vm_protect(g.task,page,ps,FALSE,VM_PROT_READ|VM_PROT_EXECUTE);
    if (kr!=KERN_SUCCESS) { seterr(err,cap,"write @0x%llx failed: %s",address,mach_error_string(kr)); return -1; }
    if (kr2!=KERN_SUCCESS) { seterr(err,cap,"breakpoint written but RX protection restore failed: %s",mach_error_string(kr2)); return -1; }
    return 0;
}
int rw_debug_set_breakpoint(unsigned long long address, char *err, size_t cap) {
    if ((address & 3u)!=0) { seterr(err,cap,"ARM64 software breakpoints require 4-byte alignment."); return -1; }
    for(size_t i=0;i<g.breakpoint_count;i++) if(g.breakpoints[i].active&&g.breakpoints[i].address==address) return 0;
    if(g.breakpoint_count>=256){seterr(err,cap,"Breakpoint limit reached (256).");return -1;}
    uint32_t original=0;if(read_u32(address,&original,err,cap)!=0)return -1;
    const uint32_t brk=0xD4200000u;if(write_u32(address,brk,err,cap)!=0)return -1;
    g.breakpoints[g.breakpoint_count++] = (rw_breakpoint_t){address,original,1}; return 0;
}
int rw_debug_remove_breakpoint(unsigned long long address, char *err, size_t cap) {
    for(size_t i=0;i<g.breakpoint_count;i++) if(g.breakpoints[i].active&&g.breakpoints[i].address==address){
        if(write_u32(address,g.breakpoints[i].original,err,cap)!=0)return -1;g.breakpoints[i].active=0;return 0;
    }
    seterr(err,cap,"No breakpoint exists at 0x%llx.",address);return -1;
}
static char *hex_encode(const uint8_t *p,size_t n){
    static const char h[]="0123456789abcdef"; if(n>4096)n=4096; char *s=malloc(n*2+1);if(!s)return NULL;
    for(size_t i=0;i<n;i++){s[i*2]=h[p[i]>>4];s[i*2+1]=h[p[i]&15];}s[n*2]=0;return s;
}
int rw_debug_read_memory(unsigned long long address,size_t length,char **json_out,char *err,size_t cap){
    if(!json_out){seterr(err,cap,"Missing output pointer.");return -1;}*json_out=NULL;if(length==0||length>4096){seterr(err,cap,"Memory reads are bounded to 1...4096 bytes.");return -1;}
    if(refresh_task(err,cap)!=0)return -1;uint8_t *buf=calloc(1,length);if(!buf){seterr(err,cap,"Out of memory.");return -1;}
    mach_vm_size_t got=0;kern_return_t kr=mach_vm_read_overwrite(g.task,address,length,(mach_vm_address_t)buf,&got);
    if(kr!=KERN_SUCCESS){free(buf);seterr(err,cap,"memory read failed: %s",mach_error_string(kr));return -1;}
    char *hex=hex_encode(buf,(size_t)got);free(buf);if(!hex){seterr(err,cap,"Out of memory.");return -1;}
    size_t need=strlen(hex)+160;char *j=malloc(need);if(!j){free(hex);seterr(err,cap,"Out of memory.");return -1;}
    snprintf(j,need,"{\"address\":\"0x%llx\",\"length\":%llu,\"hex\":\"%s\"}",address,(unsigned long long)got,hex);free(hex);*json_out=j;return 0;
}

static int thread_id_for_port(thread_t thread, uint64_t *out) {
    thread_identifier_info_data_t info; mach_msg_type_number_t count=THREAD_IDENTIFIER_INFO_COUNT;
    kern_return_t kr=thread_info(thread,THREAD_IDENTIFIER_INFO,(thread_info_t)&info,&count);
    if(kr!=KERN_SUCCESS)return -1; *out=info.thread_id; return 0;
}
static int selected_thread(thread_act_array_t threads,mach_msg_type_number_t count,thread_t *out,uint64_t *tid_out){
    if(count==0)return -1; thread_t choice=threads[0]; uint64_t choice_id=0; (void)thread_id_for_port(choice,&choice_id);
    if(g.selected_thread_id){for(mach_msg_type_number_t i=0;i<count;i++){uint64_t tid=0;if(thread_id_for_port(threads[i],&tid)==0&&tid==g.selected_thread_id){choice=threads[i];choice_id=tid;break;}}}
    else g.selected_thread_id=choice_id;
    *out=choice;if(tid_out)*tid_out=choice_id;return 0;
}
static int read_remote(mach_vm_address_t address,void *buf,size_t length){
    mach_vm_size_t got=0;kern_return_t kr=mach_vm_read_overwrite(g.task,address,(mach_vm_size_t)length,(mach_vm_address_t)buf,&got);return (kr==KERN_SUCCESS&&got==length)?0:-1;
}
static int read_remote_cstr(mach_vm_address_t address,char *out,size_t cap){
    if(!out||cap<2)return -1;size_t n=0;while(n+1<cap){char ch=0;if(read_remote(address+n,&ch,1)!=0)return -1;out[n++]=ch;if(ch==0)return 0;}out[cap-1]=0;return 0;
}
static char *alloc_text(size_t cap,char *err,size_t errcap){char *p=calloc(1,cap);if(!p)seterr(err,errcap,"Out of memory.");return p;}

static const char *rw_signal_name(int sig) {
    switch (sig) {
        case SIGTRAP: return "SIGTRAP";
        case SIGSTOP: return "SIGSTOP";
        case SIGSEGV: return "SIGSEGV";
        case SIGBUS: return "SIGBUS";
        case SIGILL: return "SIGILL";
        case SIGABRT: return "SIGABRT";
        case SIGFPE: return "SIGFPE";
        default: return "signal";
    }
}

static int rw_selected_pc(uint64_t *pc_out) {
    if (!pc_out || !g.stopped || refresh_task(NULL, 0) != 0) return -1;
    thread_act_array_t threads=NULL; mach_msg_type_number_t count=0;
    kern_return_t kr=task_threads(g.task,&threads,&count);
    if(kr!=KERN_SUCCESS||count==0)return -1;
    thread_t th=MACH_PORT_NULL; uint64_t tid=0; selected_thread(threads,count,&th,&tid);
    arm_thread_state64_t st; mach_msg_type_number_t sc=ARM_THREAD_STATE64_COUNT; memset(&st,0,sizeof(st));
    kr=thread_get_state(th,ARM_THREAD_STATE64,(thread_state_t)&st,&sc);
    for(mach_msg_type_number_t i=0;i<count;i++)mach_port_deallocate(mach_task_self(),threads[i]);
    vm_deallocate(mach_task_self(),(vm_address_t)threads,(vm_size_t)(count*sizeof(thread_t)));
    if(kr!=KERN_SUCCESS)return -1; *pc_out=st.__pc; return 0;
}

typedef struct { uint64_t load_address; uint64_t text_vmaddr; int64_t slide; char path[2048]; } rw_image_t;
static int rw_read_text_vmaddr(uint64_t load, uint64_t *vmaddr_out) {
    struct mach_header_64 mh; if(read_remote(load,&mh,sizeof(mh))!=0 || mh.magic!=MH_MAGIC_64) return -1;
    uint64_t cursor=load+sizeof(mh);
    for(uint32_t i=0;i<mh.ncmds && i<4096;i++){
        struct load_command lc; if(read_remote(cursor,&lc,sizeof(lc))!=0 || lc.cmdsize<sizeof(lc) || lc.cmdsize>(1u<<20)) return -1;
        if(lc.cmd==LC_SEGMENT_64 && lc.cmdsize>=sizeof(struct segment_command_64)){
            struct segment_command_64 seg; if(read_remote(cursor,&seg,sizeof(seg))!=0) return -1;
            if(strncmp(seg.segname,"__TEXT",16)==0){ *vmaddr_out=seg.vmaddr; return 0; }
        }
        cursor += lc.cmdsize;
    }
    return -1;
}
static int rw_collect_images(rw_image_t **images_out, uint32_t *count_out) {
    if(!images_out||!count_out)return -1; *images_out=NULL; *count_out=0;
    task_dyld_info_data_t di; mach_msg_type_number_t dc=TASK_DYLD_INFO_COUNT;
    if(task_info(g.task,TASK_DYLD_INFO,(task_info_t)&di,&dc)!=KERN_SUCCESS)return -1;
    struct dyld_all_image_infos infos; memset(&infos,0,sizeof(infos));
    if(read_remote(di.all_image_info_addr,&infos,sizeof(infos))!=0)return -1;
    uint32_t count=infos.infoArrayCount; if(count>512)count=512;
    struct dyld_image_info *arr=calloc(count?count:1,sizeof(*arr)); if(!arr)return -1;
    if(count&&read_remote((mach_vm_address_t)infos.infoArray,arr,(size_t)count*sizeof(*arr))!=0){free(arr);return -1;}
    rw_image_t *images=calloc(count?count:1,sizeof(*images)); if(!images){free(arr);return -1;}
    for(uint32_t i=0;i<count;i++){
        images[i].load_address=(uint64_t)(uintptr_t)arr[i].imageLoadAddress;
        if(arr[i].imageFilePath)(void)read_remote_cstr((mach_vm_address_t)arr[i].imageFilePath,images[i].path,sizeof(images[i].path));
        uint64_t vmaddr=0; if(rw_read_text_vmaddr(images[i].load_address,&vmaddr)==0){images[i].text_vmaddr=vmaddr; images[i].slide=(int64_t)images[i].load_address-(int64_t)vmaddr;}
    }
    free(arr); *images_out=images; *count_out=count; return 0;
}
static const rw_image_t *rw_image_for_pc(const rw_image_t *images,uint32_t count,uint64_t pc){
    const rw_image_t *best=NULL; for(uint32_t i=0;i<count;i++){if(images[i].load_address<=pc && (!best || images[i].load_address>best->load_address))best=&images[i];} return best;
}
static const char *rw_basename_const(const char *path){ const char *p=strrchr(path,'/'); return p?p+1:path; }

int rw_debug_snapshot(char **json_out,char *err,size_t cap){
    if(!json_out){seterr(err,cap,"Missing output pointer.");return -1;}*json_out=NULL;if(!g.stopped){seterr(err,cap,"Target is running; interrupt it before reading register state.");return -1;}if(refresh_task(err,cap)!=0)return -1;
    thread_act_array_t threads=NULL;mach_msg_type_number_t count=0;kern_return_t kr=task_threads(g.task,&threads,&count);
    if(kr!=KERN_SUCCESS||count==0){seterr(err,cap,"task_threads failed: %s",mach_error_string(kr));return -1;}
    thread_t selected=MACH_PORT_NULL;uint64_t selected_id=0;if(selected_thread(threads,count,&selected,&selected_id)!=0){seterr(err,cap,"No selectable thread.");return -1;}
    arm_thread_state64_t st;mach_msg_type_number_t sc=ARM_THREAD_STATE64_COUNT;memset(&st,0,sizeof(st));
    kr=thread_get_state(selected,ARM_THREAD_STATE64,(thread_state_t)&st,&sc);
    for(mach_msg_type_number_t i=0;i<count;i++)mach_port_deallocate(mach_task_self(),threads[i]);
    vm_deallocate(mach_task_self(),(vm_address_t)threads,(vm_size_t)(count*sizeof(thread_t)));
    if(kr!=KERN_SUCCESS){seterr(err,cap,"thread_get_state failed: %s",mach_error_string(kr));return -1;}
#if defined(__arm64__) || defined(__aarch64__)
    uint64_t regs[29]; memcpy(regs,&st.__x,sizeof(regs));
    char *j=calloc(1,8192);if(!j){seterr(err,cap,"Out of memory.");return -1;}size_t off=0;
    off+=snprintf(j+off,8192-off,"{\"pid\":%d,\"stopped\":%s,\"thread_count\":%u,\"selected_thread_id\":\"0x%llx\",\"pc\":\"0x%llx\",\"sp\":\"0x%llx\",\"fp\":\"0x%llx\",\"lr\":\"0x%llx\",\"cpsr\":%u,\"registers\":{",g.pid,g.stopped?"true":"false",count,selected_id,st.__pc,st.__sp,st.__fp,st.__lr,st.__cpsr);
    for(int i=0;i<29;i++)off+=snprintf(j+off,8192-off,"%s\"x%d\":\"0x%llx\"",i?",":"",i,regs[i]);
    off+=snprintf(j+off,8192-off,"},\"breakpoints\":[");int first=1;for(size_t i=0;i<g.breakpoint_count;i++)if(g.breakpoints[i].active){off+=snprintf(j+off,8192-off,"%s\"0x%llx\"",first?"":",",g.breakpoints[i].address);first=0;}snprintf(j+off,8192-off,"]}");*json_out=j;return 0;
#else
    seterr(err,cap,"Native register decoding currently requires an arm64 host.");return -1;
#endif
}

int rw_debug_poll_event(char **json_out,char *err,size_t cap){
    if(!json_out){seterr(err,cap,"Missing output pointer.");return -1;}*json_out=NULL;
    if(!g.attached){seterr(err,cap,"No debug session.");return -1;}
    int status=0;pid_t r=waitpid(g.pid,&status,WNOHANG|WUNTRACED|WCONTINUED);
    if(r<0){if(errno==ECHILD||errno==ESRCH){reset_state();seterr(err,cap,"Target is no longer traceable.");return -1;}seterr(err,cap,"waitpid event poll failed: %s",strerror(errno));return -1;}
    if(r==0){char *j=strdup("{\"event\":\"none\"}");if(!j){seterr(err,cap,"Out of memory.");return -1;}*json_out=j;return 0;}
    char tmp[512];
    if(WIFSTOPPED(status)){
        g.stopped=1; int sig=WSTOPSIG(status); uint64_t pc=0; int haspc=(rw_selected_pc(&pc)==0);
        const char *reason = sig==SIGTRAP ? "breakpoint-or-single-step" : (sig==SIGSTOP ? "interrupt" : rw_signal_name(sig));
        if(haspc)snprintf(tmp,sizeof(tmp),"{\"event\":\"stopped\",\"signal\":%d,\"reason\":\"%s\",\"pc\":\"0x%llx\"}",sig,reason,pc);
        else snprintf(tmp,sizeof(tmp),"{\"event\":\"stopped\",\"signal\":%d,\"reason\":\"%s\"}",sig,reason);
    }
    else if(WIFCONTINUED(status)){g.stopped=0;snprintf(tmp,sizeof(tmp),"{\"event\":\"continued\",\"reason\":\"execution resumed\"}");}
    else if(WIFEXITED(status)){int code=WEXITSTATUS(status);snprintf(tmp,sizeof(tmp),"{\"event\":\"exited\",\"code\":%d,\"reason\":\"process exited\"}",code);reset_state();}
    else if(WIFSIGNALED(status)){int sig=WTERMSIG(status);snprintf(tmp,sizeof(tmp),"{\"event\":\"signaled\",\"signal\":%d,\"reason\":\"%s\"}",sig,rw_signal_name(sig));reset_state();}
    else snprintf(tmp,sizeof(tmp),"{\"event\":\"changed\",\"reason\":\"process state changed\"}");
    char *j=strdup(tmp);if(!j){seterr(err,cap,"Out of memory.");return -1;}*json_out=j;return 0;
}


int rw_debug_threads(char **out,char *err,size_t cap){
    if(!out){seterr(err,cap,"Missing output pointer.");return -1;}*out=NULL;if(!g.stopped){seterr(err,cap,"Target is running; interrupt it before enumerating threads.");return -1;}if(refresh_task(err,cap)!=0)return -1;
    thread_act_array_t threads=NULL;mach_msg_type_number_t count=0;kern_return_t kr=task_threads(g.task,&threads,&count);if(kr!=KERN_SUCCESS){seterr(err,cap,"task_threads failed: %s",mach_error_string(kr));return -1;}
    char *text=alloc_text(65536,err,cap);if(!text)return -1;size_t off=0;off+=snprintf(text+off,65536-off,"THREADS (%u)\\n",count);
    for(mach_msg_type_number_t i=0;i<count&&off<65000;i++){uint64_t tid=0;arm_thread_state64_t st;mach_msg_type_number_t sc=ARM_THREAD_STATE64_COUNT;memset(&st,0,sizeof(st));int idok=thread_id_for_port(threads[i],&tid)==0;kr=thread_get_state(threads[i],ARM_THREAD_STATE64,(thread_state_t)&st,&sc);if(idok&&kr==KERN_SUCCESS)off+=snprintf(text+off,65536-off,"%c thread 0x%llx  pc=0x%llx sp=0x%llx\\n",tid==g.selected_thread_id?'*':' ',tid,st.__pc,st.__sp);}
    for(mach_msg_type_number_t i=0;i<count;i++)mach_port_deallocate(mach_task_self(),threads[i]);vm_deallocate(mach_task_self(),(vm_address_t)threads,(vm_size_t)(count*sizeof(thread_t)));*out=text;return 0;
}
int rw_debug_select_thread(unsigned long long thread_id,char *err,size_t cap){
    if(!g.stopped){seterr(err,cap,"Target is running; interrupt it before selecting a thread.");return -1;}if(refresh_task(err,cap)!=0)return -1;thread_act_array_t threads=NULL;mach_msg_type_number_t count=0;kern_return_t kr=task_threads(g.task,&threads,&count);if(kr!=KERN_SUCCESS){seterr(err,cap,"task_threads failed: %s",mach_error_string(kr));return -1;}int found=0;for(mach_msg_type_number_t i=0;i<count;i++){uint64_t tid=0;if(thread_id_for_port(threads[i],&tid)==0&&tid==thread_id){found=1;break;}}for(mach_msg_type_number_t i=0;i<count;i++)mach_port_deallocate(mach_task_self(),threads[i]);vm_deallocate(mach_task_self(),(vm_address_t)threads,(vm_size_t)(count*sizeof(thread_t)));if(!found){seterr(err,cap,"Thread 0x%llx is no longer present.",thread_id);return -1;}g.selected_thread_id=thread_id;return 0;
}
static const char *prot_text(vm_prot_t p,char out[4]){out[0]=(p&VM_PROT_READ)?'r':'-';out[1]=(p&VM_PROT_WRITE)?'w':'-';out[2]=(p&VM_PROT_EXECUTE)?'x':'-';out[3]=0;return out;}
int rw_debug_memory_map(char **out,char *err,size_t cap){
    if(!out){seterr(err,cap,"Missing output pointer.");return -1;}*out=NULL;if(refresh_task(err,cap)!=0)return -1;char *text=alloc_text(131072,err,cap);if(!text)return -1;size_t off=0;mach_vm_address_t addr=0;natural_t depth=0;unsigned rows=0;
    while(rows<512&&off<130000){mach_vm_size_t size=0;vm_region_submap_info_data_64_t info;mach_msg_type_number_t ic=VM_REGION_SUBMAP_INFO_COUNT_64;kern_return_t kr=mach_vm_region_recurse(g.task,&addr,&size,&depth,(vm_region_recurse_info_t)&info,&ic);if(kr!=KERN_SUCCESS)break;if(info.is_submap){depth++;continue;}char pr[4],mx[4];off+=snprintf(text+off,131072-off,"0x%016llx-0x%016llx %s max=%s size=0x%llx tag=%u\\n",addr,addr+size,prot_text(info.protection,pr),prot_text(info.max_protection,mx),size,info.user_tag);rows++;if(size==0||addr+size<=addr)break;addr+=size;}
    if(rows==512)snprintf(text+off,131072-off,"[truncated at 512 regions]\\n");*out=text;return 0;
}
int rw_debug_modules(char **out,char *err,size_t cap){
    if(!out){seterr(err,cap,"Missing output pointer.");return -1;}*out=NULL;if(refresh_task(err,cap)!=0)return -1;
    rw_image_t *images=NULL;uint32_t count=0;if(rw_collect_images(&images,&count)!=0){seterr(err,cap,"Failed to enumerate dyld images.");return -1;}
    char *text=alloc_text(131072,err,cap);if(!text){free(images);return -1;}size_t off=0;
    off+=snprintf(text+off,131072-off,"MODULES (%u)\n",count);
    for(uint32_t i=0;i<count&&off<130000;i++){
        off+=snprintf(text+off,131072-off,"0x%016llx slide=%+lld preferred=0x%016llx  %s\n",
            (unsigned long long)images[i].load_address,(long long)images[i].slide,(unsigned long long)images[i].text_vmaddr,images[i].path[0]?images[i].path:"<unreadable path>");
    }
    free(images);*out=text;return 0;
}

int rw_debug_backtrace(char **out,char *err,size_t cap){
    if(!out){seterr(err,cap,"Missing output pointer.");return -1;}*out=NULL;if(!g.stopped){seterr(err,cap,"Target is running; interrupt it before unwinding.");return -1;}if(refresh_task(err,cap)!=0)return -1;
    thread_act_array_t threads=NULL;mach_msg_type_number_t count=0;kern_return_t kr=task_threads(g.task,&threads,&count);if(kr!=KERN_SUCCESS||count==0){seterr(err,cap,"task_threads failed: %s",mach_error_string(kr));return -1;}
    thread_t th;uint64_t tid=0;selected_thread(threads,count,&th,&tid);arm_thread_state64_t st;mach_msg_type_number_t sc=ARM_THREAD_STATE64_COUNT;memset(&st,0,sizeof(st));kr=thread_get_state(th,ARM_THREAD_STATE64,(thread_state_t)&st,&sc);
    for(mach_msg_type_number_t i=0;i<count;i++)mach_port_deallocate(mach_task_self(),threads[i]);vm_deallocate(mach_task_self(),(vm_address_t)threads,(vm_size_t)(count*sizeof(thread_t)));if(kr!=KERN_SUCCESS){seterr(err,cap,"thread_get_state failed: %s",mach_error_string(kr));return -1;}
    rw_image_t *images=NULL;uint32_t image_count=0;(void)rw_collect_images(&images,&image_count);
    char *text=alloc_text(32768,err,cap);if(!text){free(images);return -1;}size_t off=0;off+=snprintf(text+off,32768-off,"BACKTRACE thread=0x%llx\n",tid);
    uint64_t pcs[64]={0};uint64_t fps[64]={0};unsigned frames=1;pcs[0]=st.__pc;fps[0]=st.__fp;uint64_t fp=st.__fp;
    for(unsigned frame=1;frame<64&&fp;frame++){uint64_t pair[2]={0};if(read_remote(fp,pair,sizeof(pair))!=0)break;uint64_t prev=pair[0],lr=pair[1];pcs[frames]=lr;fps[frames]=fp;frames++;if(prev<=fp||prev-fp>(16ull<<20))break;fp=prev;}
    for(unsigned i=0;i<frames&&off<32000;i++){const rw_image_t *im=rw_image_for_pc(images,image_count,pcs[i]);if(im){uint64_t rel=pcs[i]-im->load_address;uint64_t unslid=(uint64_t)((int64_t)pcs[i]-im->slide);off+=snprintf(text+off,32768-off,"#%u  0x%016llx  %s+0x%llx  unslid=0x%llx%s\n",i,pcs[i],rw_basename_const(im->path),rel,unslid,i?"":"  <PC>");}else{off+=snprintf(text+off,32768-off,"#%u  0x%016llx%s\n",i,pcs[i],i?"":"  <PC>");}}
    free(images);*out=text;return 0;
}

int rw_debug_disassembly(char **out,char *err,size_t cap){
    if(!out){seterr(err,cap,"Missing output pointer.");return -1;}*out=NULL;if(!g.stopped){seterr(err,cap,"Target is running; interrupt it before live disassembly.");return -1;}if(refresh_task(err,cap)!=0)return -1;thread_act_array_t threads=NULL;mach_msg_type_number_t count=0;kern_return_t kr=task_threads(g.task,&threads,&count);if(kr!=KERN_SUCCESS||count==0){seterr(err,cap,"task_threads failed: %s",mach_error_string(kr));return -1;}thread_t th;uint64_t tid=0;selected_thread(threads,count,&th,&tid);arm_thread_state64_t st;mach_msg_type_number_t sc=ARM_THREAD_STATE64_COUNT;memset(&st,0,sizeof(st));kr=thread_get_state(th,ARM_THREAD_STATE64,(thread_state_t)&st,&sc);for(mach_msg_type_number_t i=0;i<count;i++)mach_port_deallocate(mach_task_self(),threads[i]);vm_deallocate(mach_task_self(),(vm_address_t)threads,(vm_size_t)(count*sizeof(thread_t)));if(kr!=KERN_SUCCESS){seterr(err,cap,"thread_get_state failed: %s",mach_error_string(kr));return -1;}uint64_t start=st.__pc>=32?st.__pc-32:st.__pc;start&=~3ull;uint32_t words[32]={0};mach_vm_size_t got=0;kr=mach_vm_read_overwrite(g.task,start,sizeof(words),(mach_vm_address_t)words,&got);if(kr!=KERN_SUCCESS){seterr(err,cap,"live code read failed: %s",mach_error_string(kr));return -1;}char *text=alloc_text(32768,err,cap);if(!text)return -1;size_t off=0;off+=snprintf(text+off,32768-off,"LIVE DISASSEMBLY thread=0x%llx pc=0x%llx\\n",tid,st.__pc);size_t n=(size_t)got/4;for(size_t i=0;i<n&&off<32000;i++){uint64_t pc=start+i*4;char asmtext[192]={0};(void)re_arm64_decode(words[i],pc,asmtext,sizeof(asmtext));off+=snprintf(text+off,32768-off,"%s 0x%016llx  %08x  %s\\n",pc==st.__pc?">":" ",pc,words[i],asmtext[0]?asmtext:".inst");}*out=text;return 0;
}


void rw_debug_free_string(char *p){free(p);}

#endif
