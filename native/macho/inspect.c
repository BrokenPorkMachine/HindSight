#include "inspect.h"
#include <stdio.h>
#include <stdarg.h>
#include <string.h>
#include <inttypes.h>

/* Mach-O load commands used by the bounded mapper. Keep these named: a prior
   alpha/beta regression confused LC_FUNCTION_STARTS (0x26) with
   LC_DATA_IN_CODE (0x29) and rejected essentially every modern binary. */
enum {
 LC_ENCRYPTION_INFO = 0x21,
 LC_FUNCTION_STARTS = 0x26,
 LC_DATA_IN_CODE = 0x29,
 LC_ENCRYPTION_INFO_64 = 0x2c
};

typedef struct { char *out; size_t capacity,used; int full; } Writer;
static void emit(Writer *w,const char *fmt,...) {
 if(w->full || w->used>=w->capacity)return;
 va_list args;va_start(args,fmt);int n=vsnprintf(w->out+w->used,w->capacity-w->used,fmt,args);va_end(args);
 if(n<0 || (size_t)n>=w->capacity-w->used){w->full=1;return;}w->used+=(size_t)n;
}
static int fail(Writer *w,const char *message){emit(w,"Rejected: %s\n",message);return -1;}
static uint32_t u32(const uint8_t *p,int be){
 if(be)return ((uint32_t)p[0]<<24)|((uint32_t)p[1]<<16)|((uint32_t)p[2]<<8)|p[3];
 return ((uint32_t)p[3]<<24)|((uint32_t)p[2]<<16)|((uint32_t)p[1]<<8)|p[0];
}
static uint64_t u64(const uint8_t*p,int be){return be?((uint64_t)u32(p,be)<<32)|u32(p+4,be):((uint64_t)u32(p+4,be)<<32)|u32(p,be);}
static int range(uint64_t offset,uint64_t size,uint64_t total){return offset<=total && size<=total-offset;}
static void name16(char *out,const uint8_t *p){size_t i;for(i=0;i<16 && p[i];i++)out[i]=(p[i]>=32 && p[i]<127)?(char)p[i]:'?';out[i]=0;}
int re_macho_inspect(const uint8_t *d,size_t n,uint64_t file_size,char *out,size_t capacity){
 if(!out || capacity==0)return -1;
 out[0]=0;Writer w={out,capacity,0,0};
 if(!d || n<4){emit(&w,"Unsupported: raw or short input.\n");return 1;}
 int be;
 if(!memcmp(d,"\xcf\xfa\xed\xfe",4))be=0;
 else if(!memcmp(d,"\xfe\xed\xfa\xcf",4))be=1;
 else {emit(&w,"Unsupported: only thin 64-bit Mach-O layout is implemented; universal/raw/ELF bytes remain captured.\n");return 1;}
 if(n<32 || file_size<32)return fail(&w,"truncated header");
 uint32_t count=u32(d+16,be),commands=u32(d+20,be),file_type=u32(d+12,be);
 if(count>8192 || commands>16u*1024u*1024u)return fail(&w,"load-command budget");
 if(!range(32,commands,n) || !range(32,commands,file_size))return fail(&w,"load commands outside file or input window");
 size_t end=32+(size_t)commands,at=32;unsigned segments=0,sections=0;
 emit(&w,"Thin 64-bit Mach-O; CPU 0x%08" PRIx32 "; subtype 0x%08" PRIx32 "; filetype %" PRIu32 "; %" PRIu32 " commands\n",u32(d+4,be),u32(d+8,be),file_type,count);
 for(uint32_t index=0;index<count;index++){
  if(!range(at,8,end))return fail(&w,"truncated load command");
  uint32_t cmd=u32(d+at,be),size=u32(d+at+4,be);
  if(size<8 || size%8!=0 || !range(at,size,end))return fail(&w,"invalid command size/alignment");
  const uint8_t *p=d+at;
  if(cmd==0x19){
   if(size<72)return fail(&w,"short segment command");
   uint32_t ns=u32(p+64,be);uint64_t va=u64(p+24,be),vs=u64(p+32,be),fo=u64(p+40,be),fs=u64(p+48,be);
   if(++segments>64 || ns>256-sections)return fail(&w,"segment/section output budget");
   if((uint64_t)72+(uint64_t)ns*80!=size)return fail(&w,"segment section table length");
   if(!range(fo,fs,file_size) || vs>UINT64_MAX-va || fs>vs)return fail(&w,"segment range");
   char segment[17];name16(segment,p+8);
   emit(&w,"Segment %s VA 0x%016" PRIx64 " size 0x%" PRIx64 " file 0x%" PRIx64 "+0x%" PRIx64 " prot 0x%" PRIx32 "\n",segment,va,vs,fo,fs,u32(p+60,be));
   for(uint32_t j=0;j<ns;j++){
    const uint8_t *s=p+72+(size_t)j*80;
    uint64_t address=u64(s+32,be),bytes=u64(s+40,be);uint32_t offset=u32(s+48,be),align=u32(s+52,be),reloc=u32(s+56,be),nr=u32(s+60,be),flags=u32(s+64,be);
    uint32_t type=flags&255;int zero=type==1 || type==0xc || type==0x12;
    if(address<va || !range(address-va,bytes,vs) || align>63)return fail(&w,"section virtual range/alignment");
    if(!zero && (!range(offset,bytes,file_size) || offset<fo || !range(offset-fo,bytes,fs)))return fail(&w,"section file range");
    if(!range(reloc,(uint64_t)nr*8,file_size))return fail(&w,"section relocations outside file");
    char name[17];name16(name,s);
    emit(&w,"  Section %s VA 0x%016" PRIx64 " size 0x%" PRIx64 " %s offset 0x%" PRIx32 "\n",name,address,bytes,zero?"zero-fill":"file-backed",offset);sections++;
   }
  }else if(cmd==2){
   if(size!=24)return fail(&w,"symbol command length");
   if(!range(u32(p+8,be),(uint64_t)u32(p+12,be)*16,file_size) || !range(u32(p+16,be),u32(p+20,be),file_size))return fail(&w,"symbol/string table range");
   emit(&w,"Symbol table: %" PRIu32 " entries (range checked; names not decoded)\n",u32(p+12,be));
  }else if(cmd==LC_ENCRYPTION_INFO_64){
   if(size!=24 || !range(u32(p+8,be),u32(p+12,be),file_size))return fail(&w,"encryption command range");
   emit(&w,"Encryption ID: %" PRIu32 " (nonzero means code bytes may be unavailable for static decode)\n",u32(p+16,be));
  }else if(cmd==LC_ENCRYPTION_INFO){
   if(size<20 || !range(u32(p+8,be),u32(p+12,be),file_size))return fail(&w,"encryption command range");
   emit(&w,"Encryption ID: %" PRIu32 " (nonzero means code bytes may be unavailable for static decode)\n",u32(p+16,be));
  }else if(cmd==0x1d || cmd==LC_FUNCTION_STARTS || cmd==LC_DATA_IN_CODE || cmd==0x80000033 || cmd==0x80000034){
   if(size!=16 || !range(u32(p+8,be),u32(p+12,be),file_size))return fail(&w,"linkedit payload range");
  }
  at+=size;
 }
 if(at!=end)return fail(&w,"command count does not consume declared region");
 emit(&w,"Scope: command framing and selected segment/section/linkedit ranges checked. Unknown command payloads, overlaps, signatures, fixups and instruction semantics are NOT validated.\n");
 if(w.full)return -1;
 return 0;
}

int re_macho_code_range(const uint8_t*d,size_t n,uint64_t file_size,uint64_t*offset,uint64_t*address,uint64_t*bytes,char*reason,size_t capacity){
 if(!offset||!address||!bytes||!reason||!capacity)return -1;
 *offset=0;*address=0;*bytes=0;
 char checked[65536];
 if(re_macho_inspect(d,n,file_size,checked,sizeof checked)!=0){snprintf(reason,capacity,"Layout did not pass the supported checks.");return 1;}
 if(memcmp(d,"\xcf\xfa\xed\xfe",4)||u32(d+4,0)!=0x0100000c){snprintf(reason,capacity,"Preview requires little-endian ARM64.");return 1;}
 uint32_t type=u32(d+12,0);
 if(type!=2 && type!=6 && type!=8){snprintf(reason,capacity,"Preview requires an executable, dylib or bundle; relocatable objects are unsupported.");return 1;}
 uint64_t fo=0,va=0,sz=0;size_t owner=0,chosen=0;unsigned found=0;int encrypted=0;
 size_t at=32;
 for(uint32_t i=0;i<u32(d+16,0);i++){
  const uint8_t*p=d+at;uint32_t cmd=u32(p,0);uint32_t cmdsize=u32(p+4,0);
  if((cmd==LC_ENCRYPTION_INFO && cmdsize>=20 && u32(p+16,0)!=0) ||
     (cmd==LC_ENCRYPTION_INFO_64 && cmdsize>=24 && u32(p+16,0)!=0)) encrypted=1;
  /* LC_FUNCTION_STARTS and LC_DATA_IN_CODE are ordinary analysis metadata.
     They must never veto mapping of a valid __text section. */
  if(cmd==0x19 && (u32(p+60,0)&4)){
   for(uint32_t j=0;j<u32(p+64,0);j++){
    const uint8_t*s=p+72+(size_t)j*80;uint32_t flags=u32(s+64,0);
    if(memcmp(s,"__text\0",7) || (flags&255)!=0 || !(flags&0x80000400) || u64(s+40,0)==0)continue;
    if(++found>1){snprintf(reason,capacity,"Multiple executable __text sections are ambiguous.");return 1;}
    fo=u32(s+48,0);va=u64(s+32,0);sz=u64(s+40,0);owner=at;chosen=(size_t)(s-d);
    if(fo<32+(uint64_t)u32(d+20,0) || va-u64(p+24,0)!=fo-u64(p+40,0) || va%4 || fo%4 || sz%4){snprintf(reason,capacity,"Code mapping or four-byte alignment is inconsistent.");return 1;}
   }
  }
  at+=u32(p+4,0);
 }
 if(!found){snprintf(reason,capacity,"No supported executable __text section.");return 1;}
 at=32;
 for(uint32_t i=0;i<u32(d+16,0);i++){
  const uint8_t*p=d+at;
  if(u32(p,0)==0x19){
   uint64_t other=u64(p+24,0),length=u64(p+32,0),file=u64(p+40,0),length_file=u64(p+48,0);
   if(at!=owner && ((length && va<other+length && other<va+sz)||(length_file && fo<file+length_file && file<fo+sz))){snprintf(reason,capacity,"Overlapping segment mapping is ambiguous.");return 1;}
   for(uint32_t j=0;j<u32(p+64,0);j++){
    const uint8_t*s=p+72+(size_t)j*80;if((size_t)(s-d)==chosen)continue;
    other=u64(s+32,0);length=u64(s+40,0);file=u32(s+48,0);uint32_t kind=u32(s+64,0)&255;
    if(length && ((va<other+length && other<va+sz)||((kind!=1 && kind!=0xc && kind!=0x12)&&fo<file+length && file<fo+sz))){snprintf(reason,capacity,"Overlapping section mapping is ambiguous.");return 1;}
   }
  }
  at+=u32(p+4,0);
 }
 *offset=fo;*address=va;*bytes=sz;
 if(encrypted){snprintf(reason,capacity,"Mapped __text is declared encrypted; static instruction decode is unavailable, but metadata analysis remains supported.");return 2;}
 snprintf(reason,capacity,"Single checked __text mapping; LC_FUNCTION_STARTS/LC_DATA_IN_CODE metadata does not veto analysis.");return 0;
}

/* Read planning only: oversized declarations need no payload to be rejected by inspect. */
size_t re_macho_window_size(const uint8_t*header,size_t length,uint64_t file_size){
 size_t small=(size_t)(file_size<32?file_size:32);
 if(!header||length<32||file_size<32)return small;
 int be;if(!memcmp(header,"\xcf\xfa\xed\xfe",4))be=0;
 else if(!memcmp(header,"\xfe\xed\xfa\xcf",4))be=1;else return small;
 uint32_t commands=u32(header+20,be);
 if(commands>16u*1024u*1024u||u32(header+16,be)>8192)return 32;
 uint64_t needed=32+(uint64_t)commands;return (size_t)(needed<file_size?needed:file_size);
}
