#include "decode.h"
#include <stdio.h>
#include <inttypes.h>
#include <stdarg.h>
static int text(char*out,size_t cap,int known,const char*fmt,...){
 va_list ap;va_start(ap,fmt);int n=vsnprintf(out,cap,fmt,ap);va_end(ap);
 return n<0 || (size_t)n>=cap ? -1:known;
}
static int64_t signed_bits(uint32_t value,unsigned bits){
 uint32_t sign=1u<<(bits-1);return (int64_t)(value&(sign-1))-(int64_t)(value&sign);
}
static int target(uint64_t pc,int64_t offset,uint64_t*out){
 if(offset<0){uint64_t n=(uint64_t)(-offset);if(pc<n)return 0;*out=pc-n;}
 else{if((uint64_t)offset>UINT64_MAX-pc)return 0;*out=pc+(uint64_t)offset;}return 1;
}
static const char*reg(char*out,unsigned number,int wide,int stack){
 if(number==31)return stack?(wide?"sp":"wsp"):(wide?"xzr":"wzr");
 snprintf(out,8,"%c%u",wide?'x':'w',number);return out;
}
/* Return NULL for prefetch/reserved encodings; SIMD has a separate encoding space. */
static const char*memory_op(unsigned size,unsigned opc,int unscaled,int*wide){
 *wide=size==3 || opc==2;
 if(opc==0){static const char*normal[]={"strb","strh","str","str"};static const char*raw[]={"sturb","sturh","stur","stur"};return unscaled?raw[size]:normal[size];}
 if(opc==1){static const char*normal[]={"ldrb","ldrh","ldr","ldr"};static const char*raw[]={"ldurb","ldurh","ldur","ldur"};return unscaled?raw[size]:normal[size];}
 if(opc==2 && size<=2){static const char*normal[]={"ldrsb","ldrsh","ldrsw"};static const char*raw[]={"ldursb","ldursh","ldursw"};return unscaled?raw[size]:normal[size];}
 if(opc==3 && size<=1){*wide=0;return size==0?(unscaled?"ldursb":"ldrsb"):(unscaled?"ldursh":"ldrsh");}
 return NULL;
}
int re_arm64_decode(uint32_t w,uint64_t pc,char*out,size_t cap){
 if(!out||!cap)return -1;
 out[0]=0;if(pc%4)return -1;
 char a[8],b[8],c[8];unsigned rd=w&31,rn=(w>>5)&31;int wide=(w>>31)!=0;uint64_t dest;
 if(w==0xd503201f)return text(out,cap,1,"nop");
 if((w&0xffe0001f)==0xd4200000)return text(out,cap,1,"brk #0x%x",(w>>5)&0xffff);
 if((w&0xffe0001f)==0xd4400000)return text(out,cap,1,"hlt #0x%x",(w>>5)&0xffff);
 if((w&0xffe0001f)==0xd4000001)return text(out,cap,1,"svc #0x%x",(w>>5)&0xffff);
 if((w&0xfffffc1f)==0xd65f0000 || (w&0xfffffc1f)==0xd61f0000 || (w&0xfffffc1f)==0xd63f0000){
  const char*op=(w&0xfffffc1f)==0xd65f0000?"ret":(w&0xfffffc1f)==0xd61f0000?"br":"blr";
  return text(out,cap,1,"%s %s",op,reg(a,rn,1,0));
 }
 if((w&0x7c000000)==0x14000000){
  if(target(pc,signed_bits(w&0x03ffffff,26)*4,&dest))return text(out,cap,1,"%s 0x%016" PRIx64,wide?"bl":"b",dest);
 }else if((w&0xff000010)==0x54000000){
  static const char*cond[]={"eq","ne","hs","lo","mi","pl","vs","vc","hi","ls","ge","lt","gt","le","al","nv"};
  if(target(pc,signed_bits((w>>5)&0x7ffff,19)*4,&dest))return text(out,cap,1,"b.%s 0x%016" PRIx64,cond[w&15],dest);
 }else if((w&0x7e000000)==0x34000000){
  if(target(pc,signed_bits((w>>5)&0x7ffff,19)*4,&dest))return text(out,cap,1,"%s %s, 0x%016" PRIx64,(w&(1u<<24))?"cbnz":"cbz",reg(a,rd,wide,0),dest);
 }else if((w&0x7e000000)==0x36000000){
  unsigned bit=((w>>19)&31)+(wide?32:0);
  if(target(pc,signed_bits((w>>5)&0x3fff,14)*4,&dest))return text(out,cap,1,"%s %s, #%u, 0x%016" PRIx64,(w&(1u<<24))?"tbnz":"tbz",reg(a,rd,wide,0),bit,dest);
 }else if((w&0x1f000000)==0x10000000){
  int64_t offset=signed_bits(((w>>5)&0x7ffff)<<2|((w>>29)&3),21);
  if(target(wide?pc&~(uint64_t)4095:pc,wide?offset*4096:offset,&dest))return text(out,cap,1,"%s %s, 0x%016" PRIx64,wide?"adrp":"adr",reg(a,rd,1,0),dest);
 }else if((w&0x1f800000)==0x12800000){
  unsigned op=(w>>29)&3,shift=((w>>21)&3)*16;
  if(op!=1 && (wide || shift<32))return text(out,cap,1,"%s %s, #0x%x, lsl #%u",op==0?"movn":op==2?"movz":"movk",reg(a,rd,wide,0),(w>>5)&65535,shift);
 }else if((w&0x1f800000)==0x11000000){
  int sub=(w&(1u<<30))!=0,flags=(w&(1u<<29))!=0;
  return text(out,cap,1,"%s%s %s, %s, #0x%x, lsl #%u",sub?"sub":"add",flags?"s":"",reg(a,rd,wide,!flags),reg(b,rn,wide,1),(w>>10)&4095,(w&(1u<<22))?12:0);
 }
 /* Integer register arithmetic. Register 31 means ZR, not SP, in shifted forms. */
 if((w&0x1f200000)==0x0b000000){
  unsigned shift=(w>>22)&3,amount=(w>>10)&63,rm=(w>>16)&31;
  if(shift<3 && (wide || amount<32)){
   static const char*shifts[]={"lsl","lsr","asr"};
   return text(out,cap,1,"%s%s %s, %s, %s, %s #%u",(w&(1u<<30))?"sub":"add",(w&(1u<<29))?"s":"",reg(a,rd,wide,0),reg(b,rn,wide,0),reg(c,rm,wide,0),shifts[shift],amount);
  }
 }else if((w&0x1f000000)==0x0a000000){
  unsigned opc=(w>>29)&3,invert=(w>>21)&1,shift=(w>>22)&3,amount=(w>>10)&63,rm=(w>>16)&31;
  if(wide || amount<32){
   static const char*ops[]={"and","orr","eor","ands"};static const char*inv[]={"bic","orn","eon","bics"};static const char*shifts[]={"lsl","lsr","asr","ror"};
   return text(out,cap,1,"%s %s, %s, %s, %s #%u",invert?inv[opc]:ops[opc],reg(a,rd,wide,0),reg(b,rn,wide,0),reg(c,rm,wide,0),shifts[shift],amount);
  }
 }else if((w&0x3f000000)==0x39000000){
  unsigned size=w>>30,opc=(w>>22)&3;int data_wide;
  const char*op=memory_op(size,opc,0,&data_wide);
  if(op)return text(out,cap,1,"%s %s, [%s, #%u]",op,reg(a,rd,data_wide,0),reg(b,rn,1,1),((w>>10)&4095)<<size);
 }else if((w&0x3f200000)==0x38000000){
  unsigned size=w>>30,opc=(w>>22)&3,mode=(w>>10)&3;int data_wide;
  const char*op=memory_op(size,opc,mode==0,&data_wide);
  /* Mode 2 is unprivileged. Overlapping writeback registers are constrained-unpredictable. */
  if(op && mode!=2 && !(mode && rn!=31 && rn==rd)){
   int64_t offset=signed_bits((w>>12)&511,9);
   if(mode==1)return text(out,cap,1,"%s %s, [%s], #%" PRId64,op,reg(a,rd,data_wide,0),reg(b,rn,1,1),offset);
   return text(out,cap,1,"%s %s, [%s, #%" PRId64 "]%s",op,reg(a,rd,data_wide,0),reg(b,rn,1,1),offset,mode==3?"!":"");
  }
 }else if((w&0x3e000000)==0x28000000){
  unsigned opc=w>>30,mode=(w>>23)&3,load=(w>>22)&1,rt2=(w>>10)&31;
  /* LDPSW, SIMD pairs and constrained-unpredictable overlaps stay unsupported. */
  if((opc==0 || opc==2) && !(load && rd==rt2) && !((mode==1 || mode==3) && rn!=31 && (rn==rd || rn==rt2))){
   int data_wide=opc==2;int64_t offset=signed_bits((w>>15)&127,7)*(data_wide?8:4);
   const char*op=mode==0?(load?"ldnp":"stnp"):(load?"ldp":"stp");
   if(mode==1)return text(out,cap,1,"%s %s, %s, [%s], #%" PRId64,op,reg(a,rd,data_wide,0),reg(b,rt2,data_wide,0),reg(c,rn,1,1),offset);
   return text(out,cap,1,"%s %s, %s, [%s, #%" PRId64 "]%s",op,reg(a,rd,data_wide,0),reg(b,rt2,data_wide,0),reg(c,rn,1,1),offset,mode==3?"!":"");
  }
 }else if((w&0x3f000000)==0x18000000){
  unsigned opc=w>>30;
  if(opc<3 && target(pc,signed_bits((w>>5)&0x7ffff,19)*4,&dest))return text(out,cap,1,"%s %s, 0x%016" PRIx64,opc==2?"ldrsw":"ldr",reg(a,rd,opc!=0,0),dest);
 }
 return text(out,cap,0,".inst 0x%08" PRIx32 " ; unsupported or target overflow",w);
}

int re_page_range(uint64_t offset,uint64_t address,uint64_t bytes,uint64_t start,
                  uint64_t*position,uint64_t*pc,uint32_t*count){
 if(!position||!pc||!count)return -1;
 *position=0;*pc=0;*count=0;
 if(!bytes||bytes%4||offset%4||address%4||bytes>UINT64_MAX-offset||bytes>UINT64_MAX-address||start>=bytes/4)return -1;
 uint64_t delta=start*4,remaining=bytes/4-start;
 *position=offset+delta;*pc=address+delta;*count=(uint32_t)(remaining<256?remaining:256);return 0;
}
