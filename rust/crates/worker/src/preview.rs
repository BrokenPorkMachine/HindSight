use std::{ffi::{CStr,CString},fs::{File,Metadata},io::{self,Read,Seek,SeekFrom},os::{raw::c_char,fd::{AsRawFd,FromRawFd},unix::fs::MetadataExt}};
use super::semantic::{
    CALLEE_SAVED_GPR, CALLEE_SAVED_VECTOR, CALLER_SAVED_GPR, CALLER_SAVED_VECTOR,
    PRESERVED_SPECIAL,
};
extern "C" {
 fn re_macho_window_size(header:*const u8,length:usize,file_size:u64)->usize;
 fn re_macho_code_range(data:*const u8,length:usize,file_size:u64,offset:*mut u64,address:*mut u64,size:*mut u64,reason:*mut c_char,capacity:usize)->i32;
 fn re_arm64_decode(word:u32,pc:u64,out:*mut c_char,capacity:usize)->i32;
 fn re_page_range(offset:u64,address:u64,bytes:u64,start:u64,position:*mut u64,pc:*mut u64,count:*mut u32)->i32;
 fn re_open_source(path:*const c_char)->i32;
 fn re_verify_fd(fd:i32,digest:*const c_char,size:u64,error:*mut c_char,capacity:usize)->i32;
}
fn analysis_progress(percent:u8,stage:&str,detail:&str){
 eprintln!("VG_PROGRESS\t{percent}\t{stage}\t{detail}");
}
fn evidence_address(line:&str)->Option<String>{
 let bytes=line.as_bytes();let mut i=0usize;
 while i+3<=bytes.len(){
  if bytes[i]==b'0'&&i+2<bytes.len()&&(bytes[i+1]==b'x'||bytes[i+1]==b'X'){
   let start=i;i+=2;let digits=i;while i<bytes.len()&&bytes[i].is_ascii_hexdigit()&&i-digits<16{i+=1}
   if i>digits{return Some(line[start..i].to_ascii_lowercase())}
  }else{i+=1}
 }
 None
}
fn evidence_symbol(line:&str)->Option<String>{
 for key in ["symbol=","name="]{
  if let Some(pos)=line.to_ascii_lowercase().find(key){let tail=&line[pos+key.len()..];let value=tail.split(|c:char|c.is_whitespace()||c==','||c==';'||c==']'||c==')').next().unwrap_or("");if !value.is_empty()&&value.len()<=1024{return Some(value.to_string())}}
 }
 None
}
fn stage_fingerprint(parts:&[&str])->String{
 let mut hash:u64=0xcbf29ce484222325;
 for part in parts{for b in part.as_bytes(){hash^=*b as u64;hash=hash.wrapping_mul(0x100000001b3)}hash^=0xff;hash=hash.wrapping_mul(0x100000001b3)}
 format!("fnv1a64:{hash:016x}")
}
fn emit_evidence_source(source:&str,workspace:&str,kind:&str,text:&str,priority:u16,total:&mut usize,cap:usize)->bool{
 let mut truncated=false;
 for (ordinal,raw) in text.lines().enumerate(){
  if *total>=cap{truncated=true;break}
  let line=raw.trim();if line.is_empty()||line.len()>4096{continue}
  let address=evidence_address(line).map(|x|super::quote(&x)).unwrap_or_else(||"null".into());
  let symbol=evidence_symbol(line).map(|x|super::quote(&x)).unwrap_or_else(||"null".into());
  eprintln!("VG_EVIDENCE\t{{\"source\":{},\"workspace\":{},\"kind\":{},\"text\":{},\"address\":{},\"symbol\":{},\"ordinal\":{},\"focus_priority\":{}}}",super::quote(source),super::quote(workspace),super::quote(kind),super::quote(line),address,symbol,ordinal,priority);
  *total+=1;
 }
 truncated
}
pub fn read_window(file:&mut File,size:u64)->io::Result<Vec<u8>>{
 file.seek(SeekFrom::Start(0))?;
 let mut header=vec![0u8;size.min(32) as usize];file.read_exact(&mut header)?;
 let needed=unsafe{re_macho_window_size(header.as_ptr(),header.len(),size)};
 if needed<header.len() || needed>16*1024*1024+32{return Err(io::Error::new(io::ErrorKind::InvalidData,"Invalid layout read budget"));}
 let prefix=header.len();header.resize(needed,0);file.read_exact(&mut header[prefix..])?;
 Ok(header)
}
struct Mapping {offset:u64,address:u64,bytes:u64,encrypted:bool,reason:String}
fn mapping(window:&[u8],size:u64)->Result<Mapping,String>{
 let(mut offset,mut address,mut bytes)=(0,0,0);let mut reason=[0 as c_char;512];
 let code=unsafe{re_macho_code_range(window.as_ptr(),window.len(),size,&mut offset,&mut address,&mut bytes,reason.as_mut_ptr(),reason.len())};
 let detail=unsafe{CStr::from_ptr(reason.as_ptr())}.to_string_lossy().into_owned();
 if code!=0&&code!=2{return Err(detail);}
 Ok(Mapping{offset,address,bytes,encrypted:code==2,reason:detail})
}
fn page(file:&mut File,map:&Mapping,start:u64,data_ranges:&[(u64,u64,u16)])->io::Result<(String,u32)>{
 let(mut position,mut address,mut count)=(0,0,0);
 let code=unsafe{re_page_range(map.offset,map.address,map.bytes,start,&mut position,&mut address,&mut count)};
 if code!=0{return Err(io::Error::new(io::ErrorKind::InvalidInput,"Page starts outside the supported code section"));}
 file.seek(SeekFrom::Start(position))?;
 let mut code=vec![0u8;count as usize*4];file.read_exact(&mut code)?;
 let mut report=String::from("VA (unslid)        File offset       Word      Instruction\n");let mut known=0;
 for (index,chunk) in code.chunks_exact(4).enumerate(){
  // Native range planning has checked the complete mapped extent before these additions.
  let delta=(index*4) as u64;let pc=address+delta;let file_position=position+delta;
  let word=u32::from_le_bytes(chunk.try_into().unwrap());
  if let Some((_,_,kind))=data_ranges.iter().find(|(lo,hi,_)|file_position<*hi&&file_position.saturating_add(4)>*lo){
   report.push_str(&format!("{pc:016x}  {file_position:016x}  {word:08x}  .data-in-code kind={kind}\n"));continue;
  }
  let mut output=[0 as c_char;192];let result=unsafe{re_arm64_decode(word,pc,output.as_mut_ptr(),output.len())};
  if result<0{return Err(io::Error::new(io::ErrorKind::InvalidData,"Instruction output bound exceeded"));}
  if result==1{known+=1;}
  let instruction=unsafe{CStr::from_ptr(output.as_ptr())}.to_string_lossy();
  report.push_str(&format!("{pc:016x}  {file_position:016x}  {word:08x}  {instruction}\n"));
 }
 report.push_str(&format!("\n{count} of {} words shown; {known} decoded, {} unsupported. Start word: {start}.\n",map.bytes/4,count-known));
 report.push_str("A64 subset v2 (integer memory/arithmetic); .inst does not mean invalid. No relocations, ASLR slide, code/data proof, CFG or function boundaries.\n");
 Ok((report,count))
}
pub fn render(file:&mut File,window:&[u8],size:u64)->io::Result<(String,String)>{
 let map=match mapping(window,size){Ok(map)=>map,Err(reason)=>return Ok(("unavailable".into(),reason))};
 if map.encrypted{return Ok(("metadata-only".into(),map.reason));}
 let data_ranges=data_in_code_ranges(file,window,size)?;
 Ok(("preview".into(),page(file,&map,0,&data_ranges)?.0))
}
fn fingerprint(m:&Metadata)->(u64,u64,u64,i64,i64,i64,i64){(m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())}
pub fn request(path:&str,digest:&str,size:&str,start:&str,id:&str)->io::Result<String>{
 if path.len()>16384||!std::path::Path::new(path).is_absolute()||digest.len()!=64||!digest.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))||id.len()!=36||!id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-'){
  return Err(io::Error::new(io::ErrorKind::InvalidInput,"Invalid page identity"));
 }
 let size:u64=size.parse().map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,"Invalid source size"))?;
 let start:u64=start.parse().map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,"Invalid start word"))?;
 let path=CString::new(path)?;let hash=CString::new(digest)?;
 let fd=unsafe{re_open_source(path.as_ptr())};
 if fd<0{return Err(io::Error::new(io::ErrorKind::InvalidInput,"Cannot open captured regular file without following symlinks"));}
 // Ownership transfers from the native opener into File exactly once.
 let mut file=unsafe{File::from_raw_fd(fd)};let before=file.metadata()?;
 let mut error=[0 as c_char;512];
 if unsafe{re_verify_fd(file.as_raw_fd(),hash.as_ptr(),size,error.as_mut_ptr(),error.len())}!=0{
  return Err(io::Error::new(io::ErrorKind::InvalidData,unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy().into_owned()));
 }
 let window=read_window(&mut file,size)?;
 let map=mapping(&window,size).map_err(|e|io::Error::new(io::ErrorKind::InvalidData,e))?;
 if map.encrypted{return Err(io::Error::new(io::ErrorKind::Unsupported,"Instruction pages are unavailable because __text is declared encrypted; metadata analysis is still supported."));}
 let data_ranges=data_in_code_ranges(&mut file,&window,size)?;
 let (report,count)=page(&mut file,&map,start,&data_ranges)?;
 if fingerprint(&before)!=fingerprint(&file.metadata()?){return Err(io::Error::new(io::ErrorKind::InvalidData,"Captured object changed during page request"));}
 Ok(format!("{{\"protocol\":1,\"kind\":\"page_result\",\"request_id\":{},\"source_sha256\":{},\"start_word\":{},\"total_words\":{},\"section_address\":{},\"section_offset\":{},\"word_count\":{count},\"text\":{}}}",super::quote(id),super::quote(digest),super::quote(&start.to_string()),super::quote(&(map.bytes/4).to_string()),super::quote(&map.address.to_string()),super::quote(&map.offset.to_string()),super::quote(&report)))
}


fn read_exact_range(file:&mut File, offset:u64, size:usize)->io::Result<Vec<u8>>{
 file.seek(SeekFrom::Start(offset))?;let mut data=vec![0u8;size];file.read_exact(&mut data)?;Ok(data)
}
fn le32(p:&[u8])->u32{u32::from_le_bytes(p[..4].try_into().unwrap())}
fn le64(p:&[u8])->u64{u64::from_le_bytes(p[..8].try_into().unwrap())}
fn data_in_code_ranges(file:&mut File,window:&[u8],size:u64)->io::Result<Vec<(u64,u64,u16)>>{
 let mut ranges=Vec::new();if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]{return Ok(ranges)}let ncmd=le32(&window[16..20]);let mut at=32usize;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x29&&cs>=16{let off=le32(&window[at+8..at+12]) as u64;let len=le32(&window[at+12..at+16]) as usize;if len<=8*1024*1024&&len%8==0&&off<=size&&len as u64<=size-off{let data=read_exact_range(file,off,len)?;for e in data.chunks_exact(8).take(200_000){let start=le32(&e[0..4]) as u64;let bytes=u16::from_le_bytes(e[4..6].try_into().unwrap()) as u64;let kind=u16::from_le_bytes(e[6..8].try_into().unwrap());if bytes>0&&start<size&&bytes<=size-start{ranges.push((start,start+bytes,kind));}}}}at+=cs;}
 ranges.sort_unstable_by_key(|r|r.0);ranges.dedup();Ok(ranges)
}
fn branch_target(w:u32,pc:u64)->Option<(u64,&'static str,bool)>{
 let signed=|value:u32,bits:u32|->i64{let shift=64-bits;((value as i64)<<shift)>>shift};
 let add=|off:i64|->Option<u64>{if off<0{pc.checked_sub((-off) as u64)}else{pc.checked_add(off as u64)}};
 if w&0x7c000000==0x14000000{return add(signed(w&0x03ffffff,26)*4).map(|x|(x,if w>>31!=0{"call"}else{"branch"},w>>31==0));}
 if w&0xff000010==0x54000000{return add(signed((w>>5)&0x7ffff,19)*4).map(|x|(x,"cond",true));}
 if w&0x7e000000==0x34000000{return add(signed((w>>5)&0x7ffff,19)*4).map(|x|(x,"cond",true));}
 if w&0x7e000000==0x36000000{return add(signed((w>>5)&0x3fff,14)*4).map(|x|(x,"cond",true));}
 None
}
fn is_terminal(w:u32)->bool{(w&0xfffffc1f)==0xd65f0000||(w&0xfffffc1f)==0xd61f0000||(w&0x7c000000)==0x14000000&&w>>31==0}
fn is_conditional(w:u32)->bool{(w&0xff000010)==0x54000000||(w&0x7e000000)==0x34000000||(w&0x7e000000)==0x36000000}
fn parse_metadata(file:&mut File,window:&[u8],map:&Mapping,size:u64)->io::Result<(Vec<(u64,String)>,Vec<u64>,Vec<u64>)>{
 if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]{return Ok((vec![],vec![],vec![]));}
 let ncmd=le32(&window[16..20]);let mut at=32usize;let mut text_vm=0u64;let mut fstarts=None;let mut symtab=None;
 for _ in 0..ncmd{
  if at+8>window.len(){break} let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;
  if cs<8||at+cs>window.len(){break}
  if cmd==0x19&&cs>=72{let name=&window[at+8..at+24];if name.starts_with(b"__TEXT\0"){text_vm=le64(&window[at+24..at+32]);}}
  else if cmd==0x26&&cs>=16{fstarts=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize));}
  else if cmd==0x2&&cs>=24{symtab=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize,le32(&window[at+16..at+20]) as u64,le32(&window[at+20..at+24]) as usize));}
  at+=cs;
 }
 let mut starts=Vec::new();let mut function_starts=Vec::new();
 if let Some((off,len))=fstarts{if len<=8*1024*1024&&off<=size&&len as u64<=size-off{
  let data=read_exact_range(file,off,len)?;let mut i=0usize;let mut addr=text_vm;
  while i<data.len()&&starts.len()<200_000{let mut shift=0u32;let mut delta=0u64;loop{if i>=data.len()||shift>=64{break}let b=data[i];i+=1;delta|=((b&0x7f)as u64)<<shift;if b&0x80==0{break}shift+=7;}if delta==0{break}addr=addr.saturating_add(delta);if addr>=map.address&&addr<map.address+map.bytes&&(addr-map.address)%4==0{starts.push(addr);function_starts.push(addr);}}
 }}
 let mut symbols=Vec::new();
 if let Some((so,n,stro,strlen))=symtab{let n=n.min(200_000);let bytes=n.saturating_mul(16);if bytes<=16*1024*1024&&strlen<=16*1024*1024&&so<=size&&bytes as u64<=size-so&&stro<=size&&strlen as u64<=size-stro{
  let syms=read_exact_range(file,so,bytes)?;let strs=read_exact_range(file,stro,strlen)?;
  for i in 0..n{let p=&syms[i*16..i*16+16];let strx=le32(p) as usize;let typ=p[4];let val=le64(&p[8..16]);if typ&0xe0!=0||(typ&0x0e)!=0x0e||val<map.address||val>=map.address+map.bytes||(val-map.address)%4!=0||strx==0||strx>=strs.len(){continue}let end=strs[strx..].iter().position(|&b|b==0).map(|x|strx+x).unwrap_or(strs.len());if end<=strx{continue}let name=String::from_utf8_lossy(&strs[strx..end]).into_owned();if name.len()<=256{symbols.push((val,name));starts.push(val);}}
 }}
 starts.push(map.address);starts.sort_unstable();starts.dedup();function_starts.sort_unstable();function_starts.dedup();symbols.sort_by_key(|x|x.0);symbols.dedup_by(|a,b|a.0==b.0&&a.1==b.1);Ok((symbols,starts,function_starts))
}
fn symbol_name_at(symbols:&[(u64,String)],address:u64)->Option<&str>{
 let index=symbols.partition_point(|(value,_)|*value<address);
 symbols.get(index).filter(|(value,_)|*value==address).map(|(_,name)|name.as_str())
}
fn objc_imp_names(records:&str)->std::collections::BTreeMap<u64,String>{
 let mut names=std::collections::BTreeMap::new();
 for line in records.lines().filter(|line|line.contains(" method ")&&line.contains("confidence=validated")){
  let Some(address)=line.split(" imp=").nth(1).and_then(|tail|tail.split_whitespace().next()).and_then(|value|u64::from_str_radix(value.trim_start_matches("0x"),16).ok()) else{continue};
  let Some(name)=line.split(" resolved=").nth(1).and_then(|tail|tail.split(" confidence=").next()).map(str::trim) else{continue};
  if address!=0&&name.len()<=512&&(name.starts_with("-[")||name.starts_with("+["))&&name.ends_with(']'){names.entry(address).or_insert_with(||name.to_string());}
 }
 names
}
fn export_function_names(exports:&str,window:&[u8],starts:&[u64],scan_end:u64)->std::collections::BTreeMap<u64,String>{
 let Some(base)=dyld_segments(window).into_iter().find(|segment|segment.name=="__TEXT").map(|segment|segment.vmaddr) else{return Default::default()};
 let admitted=starts.iter().copied().collect::<std::collections::BTreeSet<_>>();let mut names=std::collections::BTreeMap::new();
 for line in exports.lines().filter_map(|line|line.strip_prefix("export ")){
  let Some((name,fields))=line.split_once(" address=") else{continue};
  if name.is_empty()||name.len()>1024||name.chars().any(char::is_whitespace){continue}
  let Some(offset)=fields.split_whitespace().next().and_then(|value|u64::from_str_radix(value.trim_start_matches("0x"),16).ok()) else{continue};
  let Some(flags)=fields.split(" flags=").nth(1).and_then(|value|value.split_whitespace().next()).and_then(|value|u64::from_str_radix(value.trim_start_matches("0x"),16).ok()) else{continue};
  if flags&8!=0{continue}
  let address=if flags&3==2{offset}else if flags&3==0{let Some(value)=base.checked_add(offset) else{continue};value}else{continue};
  if address<scan_end&&admitted.contains(&address){names.entry(address).or_insert_with(||name.to_string());}
 }
 names
}
fn export_function_starts(exports:&str,window:&[u8],code_start:u64,scan_end:u64)->Vec<u64>{
 let Some(base)=dyld_segments(window).into_iter().find(|segment|segment.name=="__TEXT").map(|segment|segment.vmaddr) else{return Vec::new()};
 exports.lines().filter_map(|line|{
  let (_,fields)=line.strip_prefix("export ")?.split_once(" address=")?;
  let offset=fields.split_whitespace().next().and_then(|value|u64::from_str_radix(value.trim_start_matches("0x"),16).ok())?;
  let flags=fields.split(" flags=").nth(1).and_then(|value|value.split_whitespace().next()).and_then(|value|u64::from_str_radix(value.trim_start_matches("0x"),16).ok())?;
  if flags&8!=0{return None}
  let address=if flags&3==2{offset}else if flags&3==0{base.checked_add(offset)?}else{return None};
  (address>=code_start&&address<scan_end&&(address-code_start)%4==0).then_some(address)
 }).collect::<std::collections::BTreeSet<_>>().into_iter().collect()
}
fn resolved_function_name<'a>(address:u64,symbols:&'a[(u64,String)],objc:&'a std::collections::BTreeMap<u64,String>,exports:&'a std::collections::BTreeMap<u64,String>,stubs:&'a std::collections::BTreeMap<u64,String>)->Option<&'a str>{
 symbol_name_at(symbols,address).or_else(||objc.get(&address).map(String::as_str)).or_else(||exports.get(&address).map(String::as_str)).or_else(||stubs.get(&address).map(String::as_str))
}
fn function_for(starts:&[u64], address:u64)->u64{
 let idx=starts.partition_point(|&x|x<=address);if idx==0{starts[0]}else{starts[idx-1]}
}
fn admitted_function_for(starts:&[u64], address:u64, scan_end:u64)->Option<u64>{
 if starts.is_empty()||address<starts[0]||address>=scan_end{return None}Some(function_for(starts,address))
}
fn reg_name(n:u32,wide:bool)->String{if n==31{if wide{"xzr".into()}else{"wzr".into()}}else{format!("{}{}",if wide{"x"}else{"w"},n)}}
fn flow_hint(w:u32)->Option<String>{
 let rd=w&31;let rn=(w>>5)&31;let rm=(w>>16)&31;let wide=w>>31!=0;
 if (w&0x1f800000)==0x11000000{return Some(format!("def {} <- use {} + immediate",reg_name(rd,wide),reg_name(rn,wide)));}
 if (w&0x1f200000)==0x0b000000{return Some(format!("def {} <- use {}, {}",reg_name(rd,wide),reg_name(rn,wide),reg_name(rm,wide)));}
 if (w&0x1f000000)==0x0a000000{return Some(format!("def {} <- use {}, {}",reg_name(rd,wide),reg_name(rn,wide),reg_name(rm,wide)));}
 if (w&0x1f800000)==0x12800000{return Some(format!("def {} <- immediate",reg_name(rd,wide)));}
 if (w&0x1f000000)==0x10000000{return Some(format!("def {} <- pc-relative address",reg_name(rd,true)));}
 if (w&0x3f000000)==0x39000000{let load=((w>>22)&3)!=0;return Some(if load{format!("def {} <- memory[use x{}]",reg_name(rd,(w>>30)==3),rn)}else{format!("memory[use x{}] <- use {}",rn,reg_name(rd,(w>>30)==3))});}
 None
}
fn printable_strings(file:&mut File,size:u64,cfstring_ranges:&[(u64,u64)])->io::Result<String>{
 let budget=size.min(4*1024*1024) as usize;let data=read_exact_range(file,0,budget)?;let mut out=String::new();let mut start=0usize;let mut i=0usize;
 while i<=data.len(){let printable=i<data.len() && (data[i]==b'\t'||(0x20..=0x7e).contains(&data[i]));if printable{i+=1;continue}
  let overlaps_cfstring=|begin:usize,end:usize|cfstring_ranges.iter().any(|(base,length)|(begin as u64)<base.saturating_add(*length)&&(end as u64)>*base);
  if i.saturating_sub(start)>=5&&!overlaps_cfstring(start,i){let end=(start+512).min(i);let text=String::from_utf8_lossy(&data[start..end]);if out.len()<60_000{out.push_str(&format!("file+0x{start:08x}  {text}{}\n",if end<i{" …"}else{""}));}}
  i+=1;start=i;
 }
 Ok(out)
}

fn object_detections(_file:&mut File,window:&[u8],size:u64,strings:&str,dyld_imports:&str)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut cstrings=0usize;let mut cfstring_records=0usize;let mut cdata=0usize;let mut cf=0usize;let mut foundation=0usize;
 for sec in &secs{
  if sec.sect.starts_with("__objc_"){out.push_str(&format!("0x{:016x} section={} classification=Objective-C-runtime-object confidence=section-backed size={} bytes\n",sec.addr,sec.sect,sec.size));if out.len()>50_000{break}}
  if sec.sect=="__cstring"||sec.sect=="__cfstring"{if sec.offset>size||sec.size>size-sec.offset||sec.size>4*1024*1024{continue}if sec.sect=="__cfstring"{out.push_str(&format!("0x{:016x} section={} classification=cfstring-constant-record confidence=section-backed size={} bytes payload=unresolved\n",sec.addr,sec.sect,sec.size));cfstring_records+=1;}else{out.push_str(&format!("0x{:016x} section={} classification=c-string confidence=section-backed size={} bytes\n",sec.addr,sec.sect,sec.size));cstrings+=1;}
  }else if ["__const","__data","__data_const","__bss","__common"].contains(&sec.sect.as_str()){out.push_str(&format!("0x{:016x} section={} classification=c-data-object confidence=section-backed size={} bytes\n",sec.addr,sec.sect,sec.size));cdata+=1;}
  if out.len()>50_000{break}
 }
 let classify=|name:&str|->Option<&'static str>{let n=name.trim_matches('_').to_ascii_lowercase();if n.contains("cfstring")||n.contains("cfdata")||n.contains("cfarray")||n.contains("cfdictionary")||n.contains("cfset")||n.contains("cfurl")||n.contains("cfuuid")||n.contains("cfrunloop")||n.contains("cfnotification")||n.contains("cfallocator"){Some("CoreFoundation object")}else if n.contains("nsstring")||n.contains("nsdata")||n.contains("nsarray")||n.contains("nsdictionary")||n.contains("nsset")||n.contains("nsurl")||n.contains("nsnumber")||n.contains("nsdate")||n.contains("nsvalue")||n.contains("nserror")||n.contains("nsobject"){Some("Foundation object")}else{None}};
 for line in dyld_imports.lines().chain(strings.lines()){let lower=line.to_ascii_lowercase();let Some(kind)=classify(&lower) else{continue};let confidence=if line.contains("symbol=")||line.contains("import"){"symbol-backed"}else{"string-backed heuristic"};out.push_str(&format!("{} classification={} confidence={}\n",line,kind,confidence));if kind=="CoreFoundation object"{cf+=1}else{foundation+=1}if out.len()>60_000{break}}
 out.push_str(&format!("\nDetection totals: C strings={} CFString constant-record sections={} C data sections={} CoreFoundation={} Foundation={}\nCFString section records are not C strings; payload decoding requires validated pointer/fixup and record-layout evidence. Object identity is not inferred from arbitrary bytes; Apple object records require matching runtime names or imports.\n",cstrings,cfstring_records,cdata,cf,foundation));Ok(out)
}

fn parse_text_relocations(file:&mut File,window:&[u8],size:u64)->io::Result<Vec<(u64,String,u32,bool,u32,Option<u64>)>>{
 if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]{return Ok(vec![])}
 let ncmd=le32(&window[16..20]);let mut at=32usize;let mut text_addr=0u64;let mut reloff=0u64;let mut nreloc=0usize;let mut symtab=None;
 for _ in 0..ncmd{
  if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}
  if cmd==0x19&&cs>=72{
   let nsects=le32(&window[at+64..at+68]) as usize;let mut sp=at+72;
   for _ in 0..nsects{if sp+80>at+cs{break}let sect=&window[sp..sp+16];let seg=&window[sp+16..sp+32];if sect.starts_with(b"__text\0")&&seg.starts_with(b"__TEXT\0"){text_addr=le64(&window[sp+32..sp+40]);reloff=le32(&window[sp+56..sp+60]) as u64;nreloc=le32(&window[sp+60..sp+64]) as usize;}sp+=80;}
  }else if cmd==0x2&&cs>=24{symtab=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize,le32(&window[at+16..at+20]) as u64,le32(&window[at+20..at+24]) as usize));}
  at+=cs;
 }
 if text_addr==0||nreloc==0||reloff==0{return Ok(vec![])}
 let (so,nsyms,stro,strlen)=match symtab{Some(v)=>v,None=>return Ok(vec![])};let nsyms=nsyms.min(200_000);let symbytes=nsyms.saturating_mul(16);
 if symbytes>16*1024*1024||strlen>16*1024*1024||so>size||symbytes as u64>size-so||stro>size||strlen as u64>size-stro{return Ok(vec![])}
 let syms=read_exact_range(file,so,symbytes)?;let strs=read_exact_range(file,stro,strlen)?;let mut names=Vec::with_capacity(nsyms);let mut values=Vec::with_capacity(nsyms);
 for i in 0..nsyms{let p=&syms[i*16..i*16+16];let strx=le32(p) as usize;values.push(le64(&p[8..16]));if strx==0||strx>=strs.len(){names.push(String::new());continue}let end=strs[strx..].iter().position(|&b|b==0).map(|x|strx+x).unwrap_or(strs.len());names.push(String::from_utf8_lossy(&strs[strx..end]).into_owned());}
 let count=nreloc.min(200_000);let bytes=count.saturating_mul(8);if bytes>8*1024*1024||reloff>size||bytes as u64>size-reloff{return Ok(vec![])}
 let rel=read_exact_range(file,reloff,bytes)?;let mut out=Vec::new();
 for i in 0..count{let p=&rel[i*8..i*8+8];let addr=i32::from_le_bytes(p[..4].try_into().unwrap());if addr<0{continue}let info=le32(&p[4..8]);let symbol=(info&0x00ff_ffff) as usize;let pcrel=((info>>24)&1)!=0;let rtype=(info>>28)&0xf;let external=((info>>27)&1)!=0;let name=if external&&symbol<names.len()&&!names[symbol].is_empty(){names[symbol].clone()}else{format!("<section-or-local:{}>",symbol)};let target=if external&&symbol<values.len()&&values[symbol]!=0{Some(values[symbol])}else{None};out.push((text_addr+addr as u64,name,rtype,pcrel,((info>>25)&3),target));}
 Ok(out)
}
fn ssa_hint(w:u32,versions:&mut [u32;32])->Option<String>{
 let rd=(w&31) as usize;let rn=((w>>5)&31) as usize;let rm=((w>>16)&31) as usize;let wide=w>>31!=0;let reg=|n:usize,v:u32|->String{format!("{}{}#{}",if wide{"x"}else{"w"},n,v)};
 let mut define=|dst:usize,uses:Vec<usize>,rhs:&str|->String{let use_text=uses.into_iter().map(|r|reg(r,versions[r])).collect::<Vec<_>>().join(", ");versions[dst]=versions[dst].saturating_add(1);format!("{} = {}{}{}",reg(dst,versions[dst]),rhs,if use_text.is_empty(){""}else{" "},use_text)};
 if rd==31{return None}
 if (w&0x1f800000)==0x11000000{return Some(define(rd,vec![rn],"add/sub imm <-"));}
 if (w&0x1f200000)==0x0b000000{return Some(define(rd,vec![rn,rm],"add/sub reg <-"));}
 if (w&0x1f000000)==0x0a000000{return Some(define(rd,vec![rn,rm],"logic reg <-"));}
 if (w&0x1f800000)==0x12800000{return Some(define(rd,vec![],"wide-immediate"));}
 if (w&0x1f000000)==0x10000000{return Some(define(rd,vec![],"pc-relative"));}
 if (w&0x3f000000)==0x39000000&&((w>>22)&3)!=0{return Some(define(rd,vec![rn],"load <- mem via"));}
 None
}
fn indirect_branch_reg(w:u32)->Option<u32>{if (w&0xfffffc1f)==0xd61f0000{Some((w>>5)&31)}else{None}}

fn cstr16(bytes:&[u8])->String{let end=bytes.iter().position(|&b|b==0).unwrap_or(bytes.len());String::from_utf8_lossy(&bytes[..end]).into_owned()}
fn platform_metadata(window:&[u8])->String{
 if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]{return "Metadata recovery unavailable for this container.".into()}
 let ncmd=le32(&window[16..20]);let mut at=32usize;let mut out=String::new();let mut objc=0usize;let mut swift=0usize;
 for _ in 0..ncmd{
  if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}
  if cmd==0x19&&cs>=72{let nsects=le32(&window[at+64..at+68]) as usize;let mut sp=at+72;for _ in 0..nsects{if sp+80>at+cs{break}let sect=cstr16(&window[sp..sp+16]);let seg=cstr16(&window[sp+16..sp+32]);let addr=le64(&window[sp+32..sp+40]);let size=le64(&window[sp+40..sp+48]);let interesting=sect.starts_with("__objc_")||sect.starts_with("__swift5_")||sect=="__cstring"||sect=="__const";if interesting&&out.len()<60_000{out.push_str(&format!("{seg},{sect:<20} {addr:#018x}  {size:>10} bytes"));if sect.starts_with("__objc_"){objc+=1;out.push_str("  Objective-C metadata");}else if sect.starts_with("__swift5_"){swift+=1;out.push_str("  Swift reflection metadata");}out.push('\n');}sp+=80;}}
  at+=cs;
 }
 if out.is_empty(){out.push_str("No Objective-C/Swift metadata sections were found in the parsed load-command window.\n");}
 out.push_str(&format!("\nObjective-C metadata sections: {objc}\nSwift reflection sections: {swift}\nSection presence is factual; class/type reconstruction remains conservative until pointer/relative-offset records are individually validated.\n"));out
}
fn block_index(leaders:&[u64], address:u64)->Option<usize>{let i=leaders.partition_point(|&x|x<=address);if i==0{None}else{Some(i-1)}}
fn modeled_def_reg(w:u32)->Option<usize>{let rd=(w&31) as usize;if rd==31{return None}if (w&0x1f800000)==0x11000000||(w&0x1f200000)==0x0b000000||(w&0x1f000000)==0x0a000000||(w&0x1f800000)==0x12800000||(w&0x1f000000)==0x10000000||(w&0x1f800000)==0x13000000||((w&0x3f000000)==0x39000000&&((w>>22)&3)!=0){Some(rd)}else{None}}
fn structural_analysis(leaders:&[u64], starts:&[u64], edges:&[(u64,u64,&str)], code:&[u8], base:u64, end:u64)->(String,String,String){
 use std::collections::{BTreeMap,BTreeSet};
 const MAX_STRUCTURAL_BLOCKS:usize=20_000;
 let n=leaders.len();if n==0||starts.is_empty(){return (String::new(),String::new(),String::new())}
 if n>MAX_STRUCTURAL_BLOCKS{
  let note=format!("CFG contains {n} basic blocks; global dominator/phi/frontier detail omitted by the {MAX_STRUCTURAL_BLOCKS}-block structural-analysis budget. Direct CFG edges and other bounded evidence remain available.\n");
  return (note.clone(),note.clone(),note);
 }
 let mut pred=vec![BTreeSet::<usize>::new();n];
 for &(from,to,kind) in edges{if kind=="call"{continue}if let(Some(a),Some(b))=(block_index(leaders,from),block_index(leaders,to)){if a<n&&b<n&&a!=b&&function_for(starts,leaders[a])==function_for(starts,leaders[b]){pred[b].insert(a);}}}
 let function_entries:BTreeSet<u64>=starts.iter().copied().collect();
 for i in 0..n.saturating_sub(1){if function_entries.contains(&leaders[i+1]){continue}let block_end=leaders[i+1];if block_end<=base||block_end>end{continue}let last=block_end.saturating_sub(4);let off=last.saturating_sub(base) as usize;if off+4<=code.len(){let w=le32(&code[off..off+4]);if !is_terminal(w){pred[i+1].insert(i);}}}

 // Beta.13 scalability rule: dominators are solved per function instead of
 // allocating an all-program dominator set for every basic block. The previous
 // formulation was O(B^2) memory across the complete CFG and could exhaust an
 // 8 GB host on branch-heavy firmware. A single pathological function is
 // bounded explicitly rather than being allowed to monopolize memory.
 const MAX_DOMINATOR_BLOCKS_PER_FUNCTION:usize=512;
 const MAX_TOTAL_DOMINATOR_BLOCKS:usize=8_192;
 const MAX_DOMINATOR_PASSES:usize=64;
 let mut groups=BTreeMap::<u64,Vec<usize>>::new();for b in 0..n{groups.entry(function_for(starts,leaders[b])).or_default().push(b);}
 let mut idom=vec![None;n];let mut dominance=String::new();let mut skipped_functions=0usize;let mut solved_functions=0usize;let mut solved_blocks=0usize;
 for (function,blocks) in groups{
  if blocks.is_empty(){continue}
  if blocks.len()>MAX_DOMINATOR_BLOCKS_PER_FUNCTION||solved_blocks.saturating_add(blocks.len())>MAX_TOTAL_DOMINATOR_BLOCKS{
   skipped_functions+=1;
   if dominance.len()<58_000{dominance.push_str(&format!("function {function:#018x}: {} basic blocks; dominator detail omitted by bounded per-function/total work budgets (per-function={}, total-solved-blocks={}). CFG/edges remain available.\n",blocks.len(),MAX_DOMINATOR_BLOCKS_PER_FUNCTION,MAX_TOTAL_DOMINATOR_BLOCKS));}
   continue
  }
  solved_functions+=1;solved_blocks=solved_blocks.saturating_add(blocks.len());
  let all:BTreeSet<usize>=blocks.iter().copied().collect();let entry=blocks.iter().copied().find(|&b|leaders[b]==function).unwrap_or(blocks[0]);
  let mut dom=BTreeMap::<usize,BTreeSet<usize>>::new();for &b in &blocks{dom.insert(b,if b==entry{[b].into_iter().collect()}else{all.clone()});}
  let pass_budget=blocks.len().saturating_mul(2).max(1).min(MAX_DOMINATOR_PASSES);let mut converged=false;
  for _ in 0..pass_budget{let mut changed=false;for &b in &blocks{if b==entry{continue}let local_preds=pred[b].iter().copied().filter(|p|all.contains(p)).collect::<Vec<_>>();let mut next=if local_preds.is_empty(){BTreeSet::new()}else{all.clone()};for p in local_preds{if let Some(pd)=dom.get(&p){next=next.intersection(pd).copied().collect();}}next.insert(b);let same=dom.get(&b).map(|current|current==&next).unwrap_or(false);if !same{dom.insert(b,next);changed=true;}}if !changed{converged=true;break}}
  if !converged{if dominance.len()<58_000{dominance.push_str(&format!("function {function:#018x}: dominator fixed-point did not converge within {pass_budget} passes; detail withheld rather than publishing partial dominance facts.\n"));}continue}
  for &b in &blocks{if b==entry{continue}let Some(ds)=dom.get(&b) else{continue};let mut best=None;let mut best_depth=0usize;for &d in ds{if d==b{continue}let depth=dom.get(&d).map(|v|v.len()).unwrap_or(0);if depth>best_depth{best=Some(d);best_depth=depth;}}idom[b]=best;}
  idom[entry]=Some(entry);
  for &b in blocks.iter().filter(|&&b|b<4000){if dominance.len()>60_000{break}let preds=pred[b].iter().map(|p|format!("B{p:04}")).collect::<Vec<_>>().join(",");let ds=dom.get(&b).map(|set|set.iter().map(|d|format!("B{d:04}")).collect::<Vec<_>>().join(",")).unwrap_or_else(||"budgeted".into());dominance.push_str(&format!("B{b:04} {:#018x}  preds=[{}]  dom=[{}]\n",leaders[b],preds,ds));}
 }
 if dominance.is_empty(){dominance.push_str("No bounded dominator detail was produced for the current CFG.\n");}
 if dominance.len()<59_000{dominance.push_str(&format!("\nDominator solver: bounded mode; solved functions={solved_functions}, solved blocks={solved_blocks}, budget-skipped functions={skipped_functions}, max blocks/function={MAX_DOMINATOR_BLOCKS_PER_FUNCTION}, max solved blocks={MAX_TOTAL_DOMINATOR_BLOCKS}, max passes/function={MAX_DOMINATOR_PASSES}.\n"));}

 const MAX_STRUCTURAL_PHI_BLOCKS:usize=8_192;let mut phi=String::new();
 if n>MAX_STRUCTURAL_PHI_BLOCKS{phi.push_str(&format!("Structural phi-site detail omitted because the CFG has {n} blocks, above the {MAX_STRUCTURAL_PHI_BLOCKS}-block phi budget.\n"));}
 else{for b in 0..n{if pred[b].len()<2{continue}let mut counts=[0u16;32];for &pb in &pred[b]{let ps=leaders[pb];let pe=leaders.get(pb+1).copied().unwrap_or(end);let lo=ps.saturating_sub(base) as usize;let hi=pe.saturating_sub(base).min(code.len() as u64) as usize;let mut seen=[false;32];for ch in code.get(lo..hi).unwrap_or(&[]).chunks_exact(4){if let Some(r)=modeled_def_reg(le32(ch)){seen[r]=true}}for r in 0..32{if seen[r]{counts[r]=counts[r].saturating_add(1)}}}let regs=(0..31).filter(|&r|counts[r]>=2).map(|r|format!("x{r}")).collect::<Vec<_>>();if regs.is_empty(){continue}let start=leaders[b];let preds_txt=pred[b].iter().map(|p|format!("B{p:04}")).collect::<Vec<_>>().join(", ");if phi.len()<60_000{phi.push_str(&format!("{start:#018x}  B{b:04} join({preds_txt})  phi candidates: {}  (modeled definitions reach at least two predecessor paths; operand versions pending merge)\n",regs.join(", ")));}}}
 if phi.is_empty(){phi.push_str("No multi-predecessor join blocks were found in the bounded CFG; no phi placement sites are required by the current structural model.\n");}
 let mut frontier=vec![BTreeSet::<usize>::new();n];for b in 0..n{if pred[b].len()<2||idom[b].is_none(){continue}let stop=idom[b];for &p in &pred[b]{if idom[p].is_none(){continue}let mut runner=Some(p);let mut guard=0usize;while runner!=stop&&guard<n{let Some(r)=runner else{break};frontier[r].insert(b);let next=idom[r];if next==Some(r){break}runner=next;guard+=1;}}}
 let mut frontiers=String::new();for b in 0..n.min(4000){if frontiers.len()>60_000{break}let fs=frontier[b].iter().map(|f|format!("B{f:04}")).collect::<Vec<_>>().join(",");let id=match idom[b]{Some(d) if d==b=>"entry".into(),Some(d)=>format!("B{d:04}"),None=>"budgeted/unresolved".into()};frontiers.push_str(&format!("B{b:04} {:#018x}  idom={}  DF=[{}]\n",leaders[b],id,fs));}
 (dominance,phi,frontiers)
}
#[derive(Clone)]
#[allow(dead_code)]
struct SectionInfo{
 seg:String,
 sect:String,
 addr:u64,
 size:u64,
 offset:u64,
 flags:u32,
 reserved1:u32,
 reserved2:u32,
}
fn sections(window:&[u8])->Vec<SectionInfo>{
 let mut out=Vec::new();if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]{return out}let ncmd=le32(&window[16..20]);let mut at=32usize;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x19&&cs>=72{let nsects=le32(&window[at+64..at+68]) as usize;let mut sp=at+72;for _ in 0..nsects{if sp+80>at+cs{break}out.push(SectionInfo{sect:cstr16(&window[sp..sp+16]),seg:cstr16(&window[sp+16..sp+32]),addr:le64(&window[sp+32..sp+40]),size:le64(&window[sp+40..sp+48]),offset:le32(&window[sp+48..sp+52]) as u64,flags:le32(&window[sp+64..sp+68]),reserved1:le32(&window[sp+68..sp+72]),reserved2:le32(&window[sp+72..sp+76])});sp+=80;}}at+=cs;}out
}
fn section_for_va<'a>(sections:&'a [SectionInfo],va:u64)->Option<&'a SectionInfo>{sections.iter().find(|s|va>=s.addr&&va<s.addr.saturating_add(s.size))}
fn read_cstr_at(file:&mut File,sec:&SectionInfo,va:u64,max:usize)->io::Result<Option<String>>{if va<sec.addr||va>=sec.addr.saturating_add(sec.size){return Ok(None)}let rel=va-sec.addr;let remain=sec.size-rel;let take=remain.min(max as u64) as usize;let data=read_exact_range(file,sec.offset+rel,take)?;let end=data.iter().position(|&b|b==0).unwrap_or(data.len());if end==0{return Ok(None)}let text=String::from_utf8_lossy(&data[..end]).into_owned();if text.chars().all(|c|!c.is_control()||c=='\t'){Ok(Some(text))}else{Ok(None)}}
/// Resolve a signed PC-relative field without wrapping on malformed inputs.
fn resolve(site:u64,rel:i64)->Option<u64>{if rel<0{site.checked_sub(rel.unsigned_abs())}else{site.checked_add(rel as u64)}}
/// Read a bounded string only when the virtual address is proven to lie in a parsed section.
fn string_at_va(file:&mut File,secs:&[SectionInfo],va:u64,max:usize)->io::Result<Option<String>>{
 let Some(sec)=section_for_va(secs,va) else{return Ok(None)};
 read_cstr_at(file,sec,va,max)
}
fn runtime_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut recovered=0usize;
 for sec in &secs{if !["__objc_classname","__objc_methname","__objc_methtype","__swift5_reflstr"].contains(&sec.sect.as_str()){continue}if sec.offset>size||sec.size>size-sec.offset||sec.size>4*1024*1024{continue}let data=read_exact_range(file,sec.offset,sec.size as usize)?;let mut start=0usize;for i in 0..=data.len(){if i==data.len()||data[i]==0{if i>start{let bytes=&data[start..i];if bytes.len()<=1024{let text=String::from_utf8_lossy(bytes).into_owned();if text.chars().all(|c|!c.is_control()||c=='\t'){out.push_str(&format!("{},{} {:#018x}  {}\n",sec.seg,sec.sect,sec.addr+start as u64,text));recovered+=1;if out.len()>45_000{return Ok(out)}}}}start=i+1;}}}
 for sec in &secs{if !["__objc_selrefs","__objc_classrefs","__objc_superrefs"].contains(&sec.sect.as_str()){continue}if sec.offset>size||sec.size>size-sec.offset||sec.size>2*1024*1024{continue}let bytes=read_exact_range(file,sec.offset,sec.size as usize)?;for (i,p) in bytes.chunks_exact(8).enumerate().take(10000){let target=le64(p);let site=sec.addr+i as u64*8;if target==0{continue}if let Some(target_sec)=section_for_va(&secs,target){let label=if ["__objc_classname","__objc_methname","__cstring","__swift5_reflstr"].contains(&target_sec.sect.as_str()){read_cstr_at(file,target_sec,target,1024)?.unwrap_or_else(||"<non-string>".into())}else{format!("{},{}",target_sec.seg,target_sec.sect)};out.push_str(&format!("{},{} {site:#018x} -> {target:#018x}  {label}\n",sec.seg,sec.sect));recovered+=1;if out.len()>60_000{return Ok(out)}}}}
 if recovered==0{out.push_str("No directly recoverable Objective-C/Swift runtime string records or resolved pointer references were found in the bounded metadata pass.\n");}Ok(out)
}

fn swift_nominal_types(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;
 for sec in secs.iter().filter(|s|s.sect=="__swift5_types"){
  if sec.offset>size||sec.size>size-sec.offset||sec.size>4*1024*1024{continue}
  let data=read_exact_range(file,sec.offset,sec.size as usize)?;
  for (i,p) in data.chunks_exact(4).enumerate().take(100_000){
   let site=sec.addr+i as u64*4;let rel=i32::from_le_bytes(p.try_into().unwrap()) as i64;
   let target=if rel<0{site.checked_sub((-rel) as u64)}else{site.checked_add(rel as u64)};let Some(target)=target else{continue};
   let Some(desc_sec)=section_for_va(&secs,target) else{continue};
   let mut name=String::from("<validated descriptor target; name unresolved>");
   let rel_in=target.saturating_sub(desc_sec.addr);if rel_in+12<=desc_sec.size&&desc_sec.offset+rel_in+12<=size{
    let hdr=read_exact_range(file,desc_sec.offset+rel_in,12)?;let name_rel=i32::from_le_bytes(hdr[8..12].try_into().unwrap()) as i64;
    let name_site=target+8;let name_va=if name_rel<0{name_site.checked_sub((-name_rel) as u64)}else{name_site.checked_add(name_rel as u64)};
    if let Some(name_va)=name_va{if let Some(name_sec)=section_for_va(&secs,name_va){if let Some(text)=read_cstr_at(file,name_sec,name_va,512)?{name=text;}}}
   }
   out.push_str(&format!("{site:#018x} rel={rel:+#x} -> {target:#018x}  {},{}  {}\n",desc_sec.seg,desc_sec.sect,name));count+=1;if out.len()>60_000{return Ok(out)}
  }
 }
 if count==0{out.push_str("No validated Swift nominal-type descriptor references were recovered from __swift5_types in the bounded metadata pass.\n");}
 Ok(out)
}
fn runtime_reference_graph(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut edges=0usize;
 for sec in secs.iter().filter(|s|["__objc_selrefs","__objc_classrefs","__objc_superrefs"].contains(&s.sect.as_str())){
  if sec.offset>size||sec.size>size-sec.offset||sec.size>2*1024*1024{continue}let data=read_exact_range(file,sec.offset,sec.size as usize)?;
  for (i,p) in data.chunks_exact(8).enumerate().take(20_000){let target=le64(p);if target==0{continue}let site=sec.addr+i as u64*8;let Some(ts)=section_for_va(&secs,target) else{continue};let label=read_cstr_at(file,ts,target,512)?.unwrap_or_else(||format!("{},{}",ts.seg,ts.sect));out.push_str(&format!("{},{} {site:#018x} -> {target:#018x}  {label}\n",sec.seg,sec.sect));edges+=1;if out.len()>60_000{return Ok(out)}}
 }
 if edges==0{out.push_str("No validated Objective-C selector/class/super reference edges were recovered.\n");}Ok(out)
}
fn objc_selector_call_evidence(file:&mut File,window:&[u8],size:u64,code:&[u8],base:u64,calls:&[(u64,u64,u64)],symbols:&[(u64,String)])->io::Result<std::collections::BTreeMap<u64,String>>{
 let mut result=std::collections::BTreeMap::new();
 let section_list=sections(window);
 for &(_,site,target) in calls{
  let direct_symbol=symbol_name_at(symbols,target).is_some_and(|name|name.contains("objc_msgSend"));
  let mut selector=None;
  if let Some(stub_section)=section_for_va(&section_list,target).filter(|section|section.sect=="__objc_stubs"){
   let relative=target-stub_section.addr;
   if relative.checked_add(20).is_some_and(|end|end<=stub_section.size){
    if let Some(bytes)=read_va(file,&section_list,size,target,20)?{
     let first=le32(&bytes[0..4]);let second=le32(&bytes[4..8]);let third=le32(&bytes[8..12]);let fourth=le32(&bytes[12..16]);let fifth=le32(&bytes[16..20]);
     let load_fields=|word:u32|->Option<(u32,u32,u64)>{if word&0x3b00_0000!=0x3900_0000||(word>>22)&3!=1||word>>30!=3{return None}Some((word&31,(word>>5)&31,(((word>>10)&0xfff)as u64)*8))};
     if let (Some((base_register,page)),Some((loaded,load_base,offset)),Some((dispatch_register,_dispatch_page)),Some((dispatch_loaded,dispatch_base,_)))=(
      arm64_address_value(first,target),load_fields(second),arm64_address_value(third,target+8),load_fields(fourth)){
      let is_dispatch=fifth&0xffff_fc1f==0xd61f_0000&&(fifth>>5)&31==dispatch_register;
      if base_register==1&&first>>31!=0&&loaded==1&&load_base==1&&dispatch_register==16&&third>>31!=0&&dispatch_loaded==16&&dispatch_base==16&&is_dispatch{
       if let Some(reference)=page.checked_add(offset){selector=objc_selector_name(file,&section_list,size,reference)?;}
      }
     }
    }
   }
  }
  if selector.is_none()&&direct_symbol&&site>=base+4&&(site-base)%4==0{
   let call_index=((site-base)/4)as usize;let load_index=call_index-1;let load_offset=load_index*4;
   if let Some(word_bytes)=code.get(load_offset..load_offset+4){let word=le32(word_bytes);
    if word&0x3b00_0000==0x3900_0000&&(word>>22)&3==1&&word>>30==3&&word&31==1{
     let base_register=(word>>5)&31;let displacement=((word>>10)&0xfff)as u64*8;let mut reference=None;
     for address_index in load_index.saturating_sub(2)..load_index{
      let address_pc=base+address_index as u64*4;let address_word=le32(&code[address_index*4..address_index*4+4]);
      if let Some((register,page))=arm64_address_value(address_word,address_pc){if register==base_register&&address_word>>31!=0{
       let mut address=page.checked_add(displacement);if address_index+1==load_index{reference=address;}
       if address_index+2==load_index{let add_word=le32(&code[(address_index+1)*4..(address_index+2)*4]);if let Some((destination,source,immediate))=arm64_add_immediate(add_word){if destination==base_register&&source==base_register{address=address.and_then(|value|value.checked_add(immediate));reference=address;}}}
      }}
     }
     if let Some(address)=reference{selector=objc_selector_name(file,&section_list,size,address)?;}
    }
   }
  }
  if let Some(selector)=selector{result.insert(site,selector);}
 }
 Ok(result)
}
fn selector_call_xrefs(relocations:&[(u64,String,u32,bool,u32,Option<u64>)],calls:&[(u64,u64,u64)],symbols:&[(u64,String)])->String{
 let mut out=String::new();let mut count=0usize;
 for (site,name,_,_,_,target) in relocations.iter(){let lower=name.to_ascii_lowercase();if !(lower.contains("objc_msgsend")||lower.contains("objc_msgsend$super")||lower.contains("selref")||lower.contains("selector")){continue}out.push_str(&format!("{site:#018x}  relocation -> {name}"));if let Some(t)=target{out.push_str(&format!(" @ {t:#018x}"));}out.push('\n');count+=1;if out.len()>60_000{return out}}
 for &(func,site,target) in calls.iter(){if let Some(name)=symbol_name_at(symbols,target){if name.contains("objc_msgSend"){out.push_str(&format!("{site:#018x}  function {func:#018x} direct call -> {name}\n"));count+=1;if out.len()>60_000{return out}}}}
 if count==0{out.push_str("No selector/message-send xrefs were proven by symbols or Mach-O relocations in the bounded code pass.\n");}out
}
fn ssa_rename_trace(code:&[u8],base:u64,leaders:&[u64],end:u64)->String{
 let mut out=String::new();let mut global=[0u32;32];
 for (bi,&start) in leaders.iter().enumerate().take(4000){let stop=leaders.get(bi+1).copied().unwrap_or(end);let lo=start.saturating_sub(base) as usize;let hi=stop.saturating_sub(base).min(code.len() as u64) as usize;if lo>=hi||hi>code.len(){continue}out.push_str(&format!("B{bi:04} {start:#018x}\n"));let mut local=global;
  for (ii,ch) in code[lo..hi].chunks_exact(4).enumerate(){let pc=start+ii as u64*4;let w=le32(ch);if let Some(line)=ssa_hint(w,&mut local){out.push_str(&format!("  {pc:#018x}  {line}\n"));}else{let mut b=[0 as c_char;192];let known=unsafe{re_arm64_decode(w,pc,b.as_mut_ptr(),b.len())};if known!=1{out.push_str(&format!("  {pc:#018x}  opaque barrier: incoming versions not propagated past unknown semantics\n"));local=[0u32;32];}}if out.len()>60_000{return out}}
  for r in 0..32{global[r]=global[r].max(local[r]);}
 }
 if out.is_empty(){out.push_str("No modeled instructions were available for SSA renaming.\n");}out
}


fn read_va(file:&mut File,secs:&[SectionInfo],size:u64,va:u64,len:usize)->io::Result<Option<Vec<u8>>>{
 let Some(sec)=section_for_va(secs,va) else{return Ok(None)};let rel=va-sec.addr;if rel.saturating_add(len as u64)>sec.size||sec.offset.saturating_add(rel).saturating_add(len as u64)>size{return Ok(None)}Ok(Some(read_exact_range(file,sec.offset+rel,len)?))
}
fn objc_metadata_string(file:&mut File,secs:&[SectionInfo],size:u64,va:u64,max:usize)->io::Result<Option<String>>{
 let Some(section)=section_for_va(secs,va)else{return Ok(None)};
 if matches!(section.flags&255,1|0xc|0x12){return Ok(None)}
 let length=(section.size-(va-section.addr)).min(max as u64+1) as usize;
 let Some(bytes)=read_va(file,secs,size,va,length)?else{return Ok(None)};
 let Some(end)=bytes.iter().position(|byte|*byte==0).filter(|end|*end>0&&*end<=max)else{return Ok(None)};
 let Ok(text)=std::str::from_utf8(&bytes[..end])else{return Ok(None)};
 if text.chars().any(char::is_control){return Ok(None)}
 Ok(Some(text.to_owned()))
}
fn objc_selector_name(file:&mut File,secs:&[SectionInfo],size:u64,va:u64)->io::Result<Option<String>>{
 let Some(section)=section_for_va(secs,va)else{return Ok(None)};
 if ["__objc_methname","__cstring"].contains(&section.sect.as_str()){return read_cstr_at(file,section,va,512)}
 if section.sect!="__objc_selrefs"{return Ok(None)}
 let Some(bytes)=read_va(file,secs,size,va,8)?else{return Ok(None)};let target=le64(&bytes);
 let Some(name_section)=section_for_va(secs,target)else{return Ok(None)};
 if !["__objc_methname","__cstring"].contains(&name_section.sect.as_str()){return Ok(None)}
 read_cstr_at(file,name_section,target,512)
}
fn objc_method_list(file:&mut File,secs:&[SectionInfo],size:u64,va:u64,kind:&str,owner:Option<&str>,extended:&[Option<String>],out:&mut String,limit:&mut usize)->io::Result<Vec<super::objc::MethodLayout>> {
 let mut records=Vec::new();
 if va==0||section_for_va(secs,va).is_none(){return Ok(records)}
 let Some(h)=read_va(file,secs,size,va,8)? else{return Ok(records)};
 let flags=le32(&h[..4]);let count=le32(&h[4..8]).min(4096) as usize;let relative=flags&0x8000_0000!=0;
 let entry_size=(flags&0xffff) as usize;let stride=if relative{entry_size.max(12)}else{entry_size.max(24)};
 if stride>256{return Ok(records)}
 for i in 0..count {let entry=va.saturating_add(8).saturating_add((i*stride) as u64);let need=if relative{12}else{24};let Some(m)=read_va(file,secs,size,entry,need)? else{break};
  let resolve_rel=|off:usize,raw:u32|->Option<u64>{let rel=(raw as i32) as i64;let site=entry.checked_add(off as u64)?;if rel<0{site.checked_sub(rel.unsigned_abs())}else{site.checked_add(rel as u64)}};
  let (sel,ty,imp)=if relative{
   let selector_word=le32(&m[0..4]);let selector_direct=selector_word&1!=0;
   let selector_relative=if selector_direct{selector_word&!1}else{selector_word};
   (resolve_rel(0,selector_relative).map(|address|(address,selector_direct)),resolve_rel(4,le32(&m[4..8])),resolve_rel(8,le32(&m[8..12])).map(|address|(address,false)))
  }else{(Some((le64(&m[0..8]),false)),Some(le64(&m[8..16])),Some((le64(&m[16..24]),false)))};
  let selector=if let Some((address,_direct))=sel{objc_selector_name(file,secs,size,address)?.unwrap_or_else(||"<selector unresolved>".into())}else{"<selector unresolved>".into()};
  let types=extended.get(i).and_then(Clone::clone).or(ty.and_then(|v|section_for_va(secs,v)).map(|s|read_cstr_at(file,s,ty.unwrap(),512)).transpose()?.flatten()).unwrap_or_else(||"<types unresolved>".into());
  let confidence=if selector.starts_with('<'){"candidate"}else{"validated"};
  if confidence=="validated"&&!types.starts_with('<')&&types.is_ascii(){
   if let Some((implementation,_))=imp.filter(|(address,_)|address%4==0&&section_for_va(secs,*address).is_some_and(|section|section.flags&0x8000_0400!=0)){
    records.push(super::objc::MethodLayout{implementation,class_method:matches!(kind,"class"|"category-class")});
   }
  }
  let resolved=owner.filter(|_| confidence=="validated").map(|class|match kind {"class"|"category-class"=>format!("+[{} {}]",class,selector),_=>format!("-[{} {}]",class,selector)});
  out.push_str(&format!("  {kind} method {entry:#018x}  name={selector}  types={types}  imp={:#018x}{} confidence={confidence}\n",imp.map(|(address,_)|address).unwrap_or(0),resolved.map(|name|format!(" resolved={name}")).unwrap_or_default()));
  *limit+=1;if out.len()>60_000{return Ok(records)}
 }
 Ok(records)
}
fn objc_ivar_list(file:&mut File,secs:&[SectionInfo],size:u64,va:u64,out:&mut String,limit:&mut usize)->io::Result<Vec<super::objc::IvarLayout>> {
 let mut records=Vec::new();
 if va==0{return Ok(records)}
 let Some(header)=read_va(file,secs,size,va,8)?else{return Ok(records)};
 let stride=le32(&header[..4]) as usize;let count=le32(&header[4..8]) as usize;
 if !(32..=256).contains(&stride)||stride%8!=0||count>4096{return Ok(records)}
 for index in 0..count {
  let Some(entry)=va.checked_add(8).and_then(|value|value.checked_add(index as u64*stride as u64))else{break};
  let Some(bytes)=read_va(file,secs,size,entry,32)?else{break};
  let offset_pointer=le64(&bytes[..8]);let name_pointer=le64(&bytes[8..16]);let type_pointer=le64(&bytes[16..24]);
  let Some(name)=objc_metadata_string(file,secs,size,name_pointer,512)?else{continue};
  let Some(encoding)=objc_metadata_string(file,secs,size,type_pointer,512)?else{continue};
  if !super::objc::identifier(&name)||encoding.is_empty()||!encoding.is_ascii()||encoding.bytes().any(|byte|byte.is_ascii_control()){continue}
  // objc4 ivar_t.offset points to an int32_t, even in LP64 images.
  let Some(section)=section_for_va(secs,offset_pointer).filter(|section|section.sect=="__objc_ivar"&&section.flags&255==0)else{continue};
  let _=section;
  let Some(offset_bytes)=read_va(file,secs,size,offset_pointer,4)?else{continue};
  let offset=i32::from_le_bytes(offset_bytes[..4].try_into().unwrap());
  let raw_alignment=le32(&bytes[24..28]);let width=le32(&bytes[28..32]);
  let alignment=if raw_alignment==u32::MAX{8}else if raw_alignment<=20{1u32<<raw_alignment}else{continue};
  if offset<0||width==0||offset as u32%alignment!=0{continue}
  out.push_str(&format!("  ivar {entry:#018x} name={name} type={encoding} offset_ptr={offset_pointer:#018x} offset={offset:#x} size={width} alignment={alignment} confidence=validated\n"));*limit+=1;
  records.push(super::objc::IvarLayout{record:entry,offset_pointer,name,encoding,offset:offset as u32,size:width,alignment});
  if out.len()>60_000{break}
 }
 Ok(records)
}
fn objc_protocol_names(file:&mut File,secs:&[SectionInfo],size:u64,va:u64)->io::Result<Vec<String>> {
 let mut names=Vec::new();if va==0||section_for_va(secs,va).is_none(){return Ok(names)}let Some(h)=read_va(file,secs,size,va,8)?else{return Ok(names)};let count=le64(&h[..8]).min(128) as usize;
 for i in 0..count {let slot=va.saturating_add(8).saturating_add((i*8) as u64);let Some(pointer)=read_va(file,secs,size,slot,8)?else{break};let target=le64(&pointer);let Some(protocol)=read_va(file,secs,size,target,16)?else{continue};let name_va=le64(&protocol[8..16]);let Some(section)=section_for_va(secs,name_va)else{continue};let Some(name)=read_cstr_at(file,section,name_va,512)?else{continue};if !name.is_empty()&&name.len()<=256&&name.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'$'|b'.'))&&!names.contains(&name){names.push(name)} }
 Ok(names)
}
fn objc_class_name(file:&mut File,secs:&[SectionInfo],size:u64,class_va:u64)->io::Result<Option<String>> {
 let Some(class)=read_va(file,secs,size,class_va,40)?else{return Ok(None)};
 let Some((_,ro))=objc_ro_record(file,secs,size,le64(&class[32..40]))?else{return Ok(None)};
 objc_metadata_string(file,secs,size,le64(&ro[24..32]),512)
}
fn objc_protocol_object(file:&mut File,secs:&[SectionInfo],size:u64,pva:u64,out:&mut String,limit:&mut usize)->io::Result<()> {
 if pva==0||section_for_va(secs,pva).is_none(){return Ok(())}let Some(ph)=read_va(file,secs,size,pva,72)? else{return Ok(())};let declared_size=le32(&ph[64..68]) as usize;if !(72..=4096).contains(&declared_size)||read_va(file,secs,size,pva,declared_size)?.is_none(){return Ok(())}let name_va=le64(&ph[8..16]);let name=if name_va!=0{if let Some(s)=section_for_va(secs,name_va){read_cstr_at(file,s,name_va,512)?.unwrap_or_else(||"<protocol name unresolved>".into())}else{"<protocol name unresolved>".into()}}else{"<protocol name unresolved>".into()};let confidence=if name.starts_with('<'){"candidate"}else{"validated"};let adopted=objc_protocol_names(file,secs,size,le64(&ph[16..24]))?;let adoption=if adopted.is_empty(){String::new()}else{format!(" adopts={}",adopted.join(","))};out.push_str(&format!("  protocol {pva:#018x} name={name}{adoption} confidence={confidence}\n"));*limit+=1;if out.len()>60_000{return Ok(())}
 // protocol_t method-list fields follow the isa/name/protocol-list pair on 64-bit Mach-O.
 let lists=[("required-instance",24usize),("required-class",32),("optional-instance",40),("optional-class",48)].map(|(label,off)|(label,le64(&ph[off..off+8])));let counts=lists.map(|(_,list)|read_va(file,secs,size,list,8).ok().flatten().map(|header|le32(&header[4..8]).min(4096) as usize).unwrap_or(0));let total=counts.iter().sum::<usize>().min(16_384);let mut extended=Vec::new();if declared_size>=80 {if let Some(full)=read_va(file,secs,size,pva,80)?{let table=le64(&full[72..80]);for index in 0..total {let value=read_va(file,secs,size,table.saturating_add(index as u64*8),8)?.and_then(|slot|{let address=le64(&slot);section_for_va(secs,address).map(|section|(section,address))}).map(|(section,address)|read_cstr_at(file,section,address,1024)).transpose()?.flatten();extended.push(value);}}}
 let mut cursor=0usize;for (index,(label,list)) in lists.into_iter().enumerate(){let end=cursor.saturating_add(counts[index]).min(extended.len());let overrides=if cursor<end{&extended[cursor..end]}else{&[]};objc_method_list(file,secs,size,list,label,None,overrides,out,limit)?;cursor=cursor.saturating_add(counts[index]);}
 Ok(())
}
fn objc_protocol_list(file:&mut File,secs:&[SectionInfo],size:u64,va:u64,out:&mut String,limit:&mut usize)->io::Result<()> {
 if va==0||section_for_va(secs,va).is_none(){return Ok(())}let Some(h)=read_va(file,secs,size,va,8)? else{return Ok(())};let count=le64(&h[..8]).min(4096) as usize;
 for i in 0..count {let slot=va.saturating_add(8).saturating_add((i*8) as u64);let Some(p)=read_va(file,secs,size,slot,8)? else{break};objc_protocol_object(file,secs,size,le64(&p),out,limit)?;if out.len()>60_000{return Ok(())}}
 Ok(())
}
fn objc_property_list(file:&mut File,secs:&[SectionInfo],size:u64,va:u64,out:&mut String,limit:&mut usize)->io::Result<Vec<super::objc::PropertyLayout>> {
 let mut records=Vec::new();
 if va==0{return Ok(records)}
 let Some(header)=read_va(file,secs,size,va,8)?else{return Ok(records)};
 let stride=le32(&header[..4]) as usize;let count=le32(&header[4..8]) as usize;
 if !(16..=256).contains(&stride)||stride%8!=0||count>4096{return Ok(records)}
 for index in 0..count {
  let Some(entry)=va.checked_add(8).and_then(|value|value.checked_add(index as u64*stride as u64))else{break};
  let Some(bytes)=read_va(file,secs,size,entry,16)?else{break};
  let Some(name)=objc_metadata_string(file,secs,size,le64(&bytes[..8]),512)?else{continue};
  let Some(attributes)=objc_metadata_string(file,secs,size,le64(&bytes[8..16]),2048)?else{continue};
  let Some(property)=super::objc::PropertyLayout::parse(entry,name,attributes)else{continue};
  out.push_str(&format!("  property {entry:#018x} name={} attributes={} getter={} setter={} backing_ivar={} confidence=validated\n",
   property.name,property.attributes,property.getter,property.setter.as_deref().unwrap_or("<readonly>"),property.ivar.as_deref().unwrap_or("<unresolved>")));
  *limit+=1;records.push(property);if out.len()>60_000{break}
 }
 Ok(records)
}
// The supported on-disk LP64 class_ro_t has a 16-byte scalar header, then
// ivarLayout/name/methods/protocols/ivars/weakIvarLayout/properties pointers.
// Only a positively marked class_rw_t is followed through its RO pointer.
fn objc_ro_record(file:&mut File,secs:&[SectionInfo],size:u64,bits:u64)->io::Result<Option<(u64,Vec<u8>)>> {
 let mut address=bits&!7u64;
 if let Some(prefix)=read_va(file,secs,size,address,16)?{
  if le32(&prefix[..4])&0x8000_0000!=0{address=le64(&prefix[8..16])&!7u64;}
 }
 let Some(bytes)=read_va(file,secs,size,address,72)?else{return Ok(None)};
 let start=le32(&bytes[4..8]);let extent=le32(&bytes[8..12]);
 if start>extent||extent>16*1024*1024{return Ok(None)}
 let name_address=le64(&bytes[24..32]);
 let Some(name)=objc_metadata_string(file,secs,size,name_address,512)?else{return Ok(None)};
 if name.is_empty()||name.len()>256||!name.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'$'|b'.')){return Ok(None)}
 Ok(Some((address,bytes)))
}
fn objc_class_graph(file:&mut File,window:&[u8],size:u64)->io::Result<(String,Vec<super::objc::ClassLayout>)>{
 let secs=sections(window);let mut out=String::new();let mut layouts=Vec::new();
 let external_classes=objc_external_class_binds(file,window,size)?;
 let mut classes=0usize;let mut methods=0usize;let mut ivars=0usize;let mut protocols=0usize;let mut properties=0usize;
 for list in secs.iter().filter(|section|section.sect=="__objc_classlist"){
  if list.offset>size||list.size>size-list.offset||list.size>2*1024*1024{continue}
  let data=read_exact_range(file,list.offset,list.size as usize)?;
  for (index,pointer) in data.chunks_exact(8).enumerate().take(20_000){
   let class_address=le64(pointer);let Some(class)=read_va(file,&secs,size,class_address,40)?else{continue};
   let super_address=le64(&class[8..16]);
   let Some((ro_address,ro))=objc_ro_record(file,&secs,size,le64(&class[32..40]))?else{
    out.push_str(&format!("candidate class[{index}] {class_address:#018x} super={super_address:#018x} (RO record unresolved)\n"));continue
   };
   let Some(name)=objc_metadata_string(file,&secs,size,le64(&ro[24..32]),512)?else{continue};
   let methods_address=le64(&ro[32..40]);let protocols_address=le64(&ro[40..48]);let ivars_address=le64(&ro[48..56]);let properties_address=le64(&ro[64..72]);
   let instance_start=le32(&ro[4..8]);let instance_size=le32(&ro[8..12]);
   let conformances=objc_protocol_names(file,&secs,size,protocols_address)?;
   let conforms=if conformances.is_empty(){String::new()}else{format!("  conforms={}",conformances.join(","))};
   let local_superclass=objc_class_name(file,&secs,size,super_address)?;
   let imported_superclass=class_address.checked_add(8).and_then(|slot|external_classes.get(&slot));
   let superclass=match (local_superclass,imported_superclass){
    (Some(local),Some((imported,_))) if local!=*imported=>String::new(),
    (_,Some((name,provider)))=>format!("  super_name={name}  super_source=dyld-bind  super_provider={provider}"),
    (Some(local),None)=>format!("  super_name={local}"),
    _=>String::new()
   };
   out.push_str(&format!("reconstructed class {class_address:#018x}  name={name}  super={super_address:#018x}{superclass}  ro={ro_address:#018x}{conforms} instance_start={instance_start} instance_size={instance_size}\n"));classes+=1;
   let mut declared_methods=objc_method_list(file,&secs,size,methods_address,"instance",Some(&name),&[],&mut out,&mut methods)?;
   // The metaclass uses the same RO layout: methods are at byte 32.
   let isa=le64(&class[..8]);
   if let Some(meta)=read_va(file,&secs,size,isa,40)?{
    if let Some((_,meta_ro))=objc_ro_record(file,&secs,size,le64(&meta[32..40]))?{
     declared_methods.extend(objc_method_list(file,&secs,size,le64(&meta_ro[32..40]),"class",Some(&name),&[],&mut out,&mut methods)?);
    }
   }
   let expected_members=if ivars_address==0{Some(0)}else{read_va(file,&secs,size,ivars_address,8)?.map(|header|le32(&header[4..8]) as usize)};
   let members:Vec<_>=objc_ivar_list(file,&secs,size,ivars_address,&mut out,&mut ivars)?.into_iter().filter(|ivar|{
    ivar.offset>=instance_start&&ivar.offset.checked_add(ivar.size).is_some_and(|end|end<=instance_size)
   }).collect();
   objc_protocol_list(file,&secs,size,protocols_address,&mut out,&mut protocols)?;
   let declared_properties=objc_property_list(file,&secs,size,properties_address,&mut out,&mut properties)?;
   let expected_properties=if properties_address==0{Some(0)}else{read_va(file,&secs,size,properties_address,8)?.map(|header|le32(&header[4..8]) as usize)};
   let ivars_complete=expected_members==Some(members.len());let properties_complete=expected_properties==Some(declared_properties.len());
   if !ivars_complete||!properties_complete{out.push_str("  member coverage incomplete; affected source member/type reconstruction withheld\n")}
   layouts.push(super::objc::ClassLayout{address:class_address,name,instance_start,instance_size,ivars:members,properties:declared_properties,methods:declared_methods,ivars_complete,properties_complete});
   if out.len()>60_000{return Ok((out,Vec::new()))}
  }
 }
 if classes==0{out.push_str("No Objective-C class records satisfied the bounded pointer/RO validation rules.\n");}
 else{out.push_str(&format!("\nValidated Objective-C classes: {classes}; methods: {methods}; ivars: {ivars}; protocols: {protocols}; properties: {properties}. Raw/encoded pointers that do not map into verified sections remain unresolved.\n"));}
 Ok((out,layouts))
}
fn swift_context_graph(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;let kind_name=|k:u32|->&'static str{match k{0=>"module",1=>"extension",2=>"anonymous",3=>"protocol",16=>"class",17=>"struct",18=>"enum",_=>"other"}};
 for sec in secs.iter().filter(|s|s.sect=="__swift5_types") {if sec.offset>size||sec.size>size-sec.offset||sec.size>4*1024*1024{continue}let data=read_exact_range(file,sec.offset,sec.size as usize)?;
  for (i,p) in data.chunks_exact(4).enumerate().take(100_000){let site=sec.addr+i as u64*4;let rel=i32::from_le_bytes(p.try_into().unwrap()) as i64;let desc=if rel<0{site.checked_sub((-rel) as u64)}else{site.checked_add(rel as u64)};let Some(desc)=desc else{continue};let Some(h)=read_va(file,&secs,size,desc,12)? else{continue};let flags=le32(&h[..4]);let parent_rel=i32::from_le_bytes(h[4..8].try_into().unwrap()) as i64;let name_rel=i32::from_le_bytes(h[8..12].try_into().unwrap()) as i64;let parent_site=desc+4;let name_site=desc+8;let parent=if parent_rel==0{None}else if parent_rel<0{parent_site.checked_sub((-parent_rel) as u64)}else{parent_site.checked_add(parent_rel as u64)};let name_va=if name_rel<0{name_site.checked_sub((-name_rel) as u64)}else{name_site.checked_add(name_rel as u64)};let name=match name_va{Some(v)=>string_at_va(file,&secs,v,512)?.unwrap_or_else(||"<name unresolved>".into()),None=>"<name unresolved>".into()};let pk=flags&0x1f;let ptxt=match parent{Some(v) if section_for_va(&secs,v).is_some()=>format!("{v:#018x}"),Some(v)=>format!("{v:#018x} (unmapped)"),None=>"none".into()};out.push_str(&format!("descriptor {desc:#018x} kind={}({pk}) name={name} parent={ptxt} provenance=relative-pointer+mapped-cstring confidence=validated-record\n",kind_name(pk)));count+=1;if out.len()>60_000{return Ok(out)}}
 }
 if count==0{out.push_str("No Swift context descriptors passed bounded relative-pointer validation.\n");}Ok(out)
}
fn last_defs_for_block(code:&[u8],base:u64,start:u64,end:u64)->[Option<u64>;32]{let mut defs=[None;32];let lo=start.saturating_sub(base) as usize;let hi=end.saturating_sub(base).min(code.len() as u64) as usize;if lo>=hi||hi>code.len(){return defs}for (i,ch) in code[lo..hi].chunks_exact(4).enumerate(){if let Some(r)=modeled_def_reg(le32(ch)){defs[r]=Some(start+i as u64*4)}}defs}
fn phi_operand_trace(leaders:&[u64],starts:&[u64],edges:&[(u64,u64,&str)],code:&[u8],base:u64,end:u64)->String{
 use std::collections::BTreeSet;const MAX_PHI_OPERAND_BLOCKS:usize=20_000;let n=leaders.len();
 if n>MAX_PHI_OPERAND_BLOCKS{return format!("Phi operand provenance omitted because the CFG has {n} blocks, above the {MAX_PHI_OPERAND_BLOCKS}-block bounded operand-analysis budget.\n")}
 let mut pred=vec![BTreeSet::<usize>::new();n];for &(from,to,kind) in edges{if kind=="call"{continue}if let(Some(a),Some(b))=(block_index(leaders,from),block_index(leaders,to)){if a<n&&b<n&&a!=b&&function_for(starts,leaders[a])==function_for(starts,leaders[b]){pred[b].insert(a);}}}
 let local=(0..n).map(|b|{let pe=leaders.get(b+1).copied().unwrap_or(end);last_defs_for_block(code,base,leaders[b],pe)}).collect::<Vec<_>>();
 let mut out=String::new();for b in 0..n{if pred[b].len()<2{continue}for r in 0..31{let mut ops=Vec::new();for &pb in &pred[b]{if let Some(pc)=local[pb][r]{ops.push(format!("x{r}@B{pb:04}[def {pc:#018x}]"));}}if ops.len()>=2{out.push_str(&format!("B{b:04} {:#018x}  x{r}.phi <- {}\n",leaders[b],ops.join(", ")));if out.len()>60_000{return out}}}}if out.is_empty(){out.push_str("No phi site had two or more validated modeled predecessor definitions.\n");}out
}

fn explicit_integer_cast(w:u32)->Option<(usize,usize,&'static str,&'static str)> {
 // UBFM/SBFM aliases with immr=0 encode the architectural UXTB/UXTH/UXTW
 // and SXTB/SXTH/SXTW operations. Requiring N==sf excludes reserved forms.
 if (w&0x1f800000)!=0x13000000||((w>>16)&0x3f)!=0{return None}
 let sf=(w>>31)&1;let n=(w>>22)&1;if n!=sf{return None}
 let opc=(w>>29)&3;let imms=(w>>10)&0x3f;let rd=(w&31) as usize;let rn=((w>>5)&31) as usize;
 let (operation,result)=match (opc,imms,sf){
  (0,7,0)=>("sign-extend-i8-to-i32","signed-i32"),(0,15,0)=>("sign-extend-i16-to-i32","signed-i32"),
  (0,7,1)=>("sign-extend-i8-to-i64","signed-i64"),(0,15,1)=>("sign-extend-i16-to-i64","signed-i64"),(0,31,1)=>("sign-extend-i32-to-i64","signed-i64"),
  (2,7,0)=>("zero-extend-u8-to-u32","unsigned-u32"),(2,15,0)=>("zero-extend-u16-to-u32","unsigned-u32"),
  (2,7,1)=>("zero-extend-u8-to-u64","unsigned-u64"),(2,15,1)=>("zero-extend-u16-to-u64","unsigned-u64"),(2,31,1)=>("zero-extend-u32-to-u64","unsigned-u64"),
  _=>return None,
 };
 Some((rd,rn,operation,result))
}

fn definition_value_shape(w:u32)->&'static str{
 if let Some((_,_,_,shape))=explicit_integer_cast(w){return shape}
 if (w&0x1f000000)==0x10000000{return "pointer-width-address-candidate"}
 if (w&0x1f800000)==0x12800000{return if w>>31!=0{"integer-u64"}else{"integer-u32-zero-extended"}}
 if (w&0x1f800000)==0x11000000||(w&0x1f200000)==0x0b000000||(w&0x1f000000)==0x0a000000{return if w>>31!=0{"u64-or-pointer-width"}else{"integer-u32-zero-extended"}}
 if (w&0x3f000000)==0x39000000&&((w>>22)&3)!=0{return match w>>30{0=>"loaded-8-bit-value",1=>"loaded-16-bit-value",2=>"loaded-32-bit-value",_=>"loaded-64-bit-or-pointer-width"}}
 "unknown"
}

fn modeled_argument_uses(w:u32)->u8{
 let mut uses=0u8;let mut add=|r:u32|{if r<8{uses|=1u8<<r}};let rn=(w>>5)&31;let rm=(w>>16)&31;let rt=w&31;
 if (w&0x1f800000)==0x11000000{add(rn)}
 else if (w&0x1f200000)==0x0b000000||(w&0x1f000000)==0x0a000000{add(rn);add(rm)}
 else if (w&0x1f800000)==0x13000000{add(rn)}
 else if (w&0x3b000000)==0x39000000{add(rn);if ((w>>22)&3)==0{add(rt)}}
 else if (w&0x3a000000)==0x28000000{add(rn);if ((w>>22)&1)==0{add(rt);add((w>>10)&31)}}
 else if (w&0x7e000000)==0x34000000||(w&0x7e000000)==0x36000000{add(rt)}
 else if (w&0xfffffc1f)==0xd61f0000||(w&0xfffffc1f)==0xd63f0000||(w&0xfffffc1f)==0xd65f0000{add(rn)}
 uses
}

fn function_parameter_candidates(starts:&[u64],code:&[u8],base:u64,symbols:&[(u64,String)])->String{
 let mut out=String::new();let code_end=base.saturating_add(code.len() as u64);let mut found=0usize;
 for (index,&entry) in starts.iter().take(2000).enumerate(){
  if entry<base||entry>=code_end{continue}
  let end=starts.get(index+1).copied().unwrap_or(code_end).min(code_end).min(entry.saturating_add(128));
  let lo=(entry-base) as usize;let hi=(end-base) as usize;let mut defined=0u8;let mut live_in=0u8;
  for chunk in code[lo..hi].chunks_exact(4){let w=le32(chunk);if (w&0x7c000000)==0x14000000||is_conditional(w)||is_terminal(w){break}live_in|=modeled_argument_uses(w)&!defined;if let Some(r)=modeled_def_reg(w){if r<8{defined|=1u8<<r}}}
  if live_in==0{continue}
  let registers=(0..8).filter(|r|live_in&(1u8<<r)!=0).map(|r|format!("x{r}")).collect::<Vec<_>>();
  let highest=8-live_in.leading_zeros() as usize;let name=symbol_name_at(symbols,entry).unwrap_or("<discovered>");
  out.push_str(&format!("function {entry:#018x} {name} parameter-registers=[{}] highest-abi-position={highest} classification=incoming-parameter-candidate provenance=entry-block-use-before-modeled-definition\n",registers.join(", ")));
  found+=1;if out.len()>18_000{break}
 }
 if found==0{out.push_str("No entry-block uses of x0-x7 before a modeled definition were recovered.\n")}
 out
}

fn explicit_cast_evidence(code:&[u8],base:u64)->String{
 let mut out=String::new();let mut found=0usize;
 for (index,chunk) in code.chunks_exact(4).enumerate(){let w=le32(chunk);let Some((rd,rn,operation,result))=explicit_integer_cast(w) else{continue};let pc=base+index as u64*4;out.push_str(&format!("{pc:#018x} x{rd}<-x{rn} cast={operation} result-shape={result} classification=explicit-integer-cast provenance=decoded-bitfield-alias\n"));found+=1;if out.len()>18_000{break}}
 if found==0{out.push_str("No explicit UXTB/UXTH/UXTW/SXTB/SXTH/SXTW encodings were recovered.\n")}
 out
}

/// Build the caller and callee claims for every direct call and iterate them
/// to a fixed point.
///
/// The caller side states the machine representation the call site observed
/// for an argument register. The callee side states the type a proven
/// prototype declares. The two are separate claims: a disagreement is
/// reported as a conflict with both sides kept, and neither side is
/// preferred over the other.
fn caller_callee_constraints(
 calls:&[(u64,u64,u64)],
 signature_symbols:&[(u64,String)],
 external_prototypes:&std::collections::BTreeMap<String,super::signatures::Signature>,
 observed:&std::collections::BTreeMap<u64,std::collections::BTreeMap<u8,u8>>,
)->super::semantic::artifact::ConstraintSolution{
 use super::semantic::artifact::{Constraint,ConstraintKind};
 // The proven prototype of each function, by address.
 let mut prototypes:std::collections::BTreeMap<u64,super::signatures::Signature>=std::collections::BTreeMap::new();
 for (address,name) in signature_symbols{
  if let Some(signature)=super::signatures::symbol(name).or_else(||external_prototypes.get(name).cloned()){
   prototypes.insert(*address,signature);
  }
 }
 let mut caller_claims:Vec<Constraint>=Vec::new();
 let mut callee_claims:Vec<Constraint>=Vec::new();
 for &(func,site,target) in calls.iter().take(2000){
  // The callee side: a proven prototype for the target declares the type
  // of each argument, keyed by the register the argument travels in. The
  // register is the identity both sides share across the call.
  if let Some(signature)=prototypes.get(&target){
   for (spelling,register) in super::signatures::abi_parameter_types(signature){
    let (Some(spelling),Some(register))=(spelling,register) else{continue};
    if register>=8{continue}
    callee_claims.push(Constraint{site,callee:target,kind:ConstraintKind::ArgumentType,index:Some(register),spelling,origin:"callee-prototype",strength:0.95});
   }
  }
  // The caller side: the caller's own proven parameter types, also keyed by
  // the register the value travels in.
  if let Some(signature)=prototypes.get(&func){
   for (spelling,register) in super::signatures::abi_parameter_types(signature){
    let (Some(spelling),Some(register))=(spelling,register) else{continue};
    if register>=8{continue}
    caller_claims.push(Constraint{site,callee:target,kind:ConstraintKind::ArgumentType,index:Some(register),spelling,origin:"caller-prototype",strength:0.9});
   }
  }
  // The observed machine representation of an argument register. It is a
  // width, not a source type, so it is a separate claim kind and is never
  // compared against a prototype spelling.
  if let Some(registers)=observed.get(&site){
   for (register,width) in registers{
    if *width==0{continue}
    caller_claims.push(Constraint{site,callee:target,kind:ConstraintKind::ArgumentWidth,index:Some(*register),spelling:format!("machine-width-{width}"),origin:"caller-observation",strength:0.6});
   }
  }
 }
 super::semantic::artifact::solve_constraints(caller_claims,callee_claims)
}

/// The machine representation of each argument register observed at a call
/// site, from the reaching definition in the same block. The shape names the
/// width the definition proves, not a source type: a caller observes the
/// machine representation, the callee side states the declared type, and the
/// constraint solver keeps the two as separate claims.
fn observed_argument_widths(calls:&[(u64,u64,u64)],starts:&[u64],code:&[u8],base:u64)->std::collections::BTreeMap<u64,std::collections::BTreeMap<u8,u8>>{
 let mut out=std::collections::BTreeMap::new();
 for &(_,site,_) in calls.iter().take(2000){
  let lo=site.saturating_sub(base) as usize;
  let function_start=starts.iter().copied().rfind(|start|*start<=site).unwrap_or(base);
  let start=lo.saturating_sub(16*4).max(function_start.saturating_sub(base) as usize);
  let mut defs:[Option<u64>;8]=[None;8];
  if lo<=code.len(){
   for (i,ch) in code[start..lo.min(code.len())].chunks_exact(4).enumerate(){
    let w=le32(ch);
    if (w&0x7c000000)==0x14000000||is_conditional(w)||is_terminal(w){defs=[None;8];continue}
    if let Some(r)=modeled_def_reg(w){if r<8{defs[r]=Some(base+start as u64+i as u64*4);}}
   }
  }
  let mut observed=std::collections::BTreeMap::new();
  for register in 0..8u8{
   if let Some(pc)=defs[register as usize]{
    let offset=pc.saturating_sub(base) as usize;
    if offset+4<=code.len(){
     let width=definition_value_width(le32(&code[offset..offset+4]));
     observed.insert(register,width);
    }
   }
  }
  if !observed.is_empty(){out.insert(site,observed);}
 }
 out
}

/// The width in bits a definition proves, or zero when it proves none.
fn definition_value_width(w:u32)->u8{
 match definition_value_shape(w){
  "loaded-8-bit-value"=>8,
  "loaded-16-bit-value"=>16,
  "loaded-32-bit-value"=>32,
  "integer-u32-zero-extended"=>32,
  "loaded-64-bit-or-pointer-width"=>64,
  "integer-u64"=>64,
  "u64-or-pointer-width"=>64,
  "pointer-width-address-candidate"=>64,
  _=>0,
 }
}

fn call_argument_provenance(calls:&[(u64,u64,u64)],starts:&[u64],code:&[u8],base:u64,symbols:&[(u64,String)])->String{
 let mut arguments=String::new();
 for &(func,site,target) in calls.iter().take(2000){let lo=site.saturating_sub(base) as usize;let function_start=func.saturating_sub(base) as usize;let start=lo.saturating_sub(16*4).max(function_start.min(lo));let mut defs:[Option<(u64,u32)>;8]=[None;8];if lo<=code.len(){for (i,ch) in code[start..lo.min(code.len())].chunks_exact(4).enumerate(){let w=le32(ch);if (w&0x7c000000)==0x14000000||is_conditional(w)||is_terminal(w){defs=[None;8];continue}if let Some(r)=modeled_def_reg(w){if r<8{defs[r]=Some((base+start as u64+i as u64*4,w));}}}}let callee=symbol_name_at(symbols,target).unwrap_or("<target>");let args=(0..8).filter_map(|r|defs[r].map(|(pc,w)|format!("x{r}<-{pc:#x} shape={}",definition_value_shape(w)))).collect::<Vec<_>>();arguments.push_str(&format!("call {site:#018x} in {func:#018x} -> {target:#018x} {callee} observed-argument-register-definitions={} args=[{}] return-register=x0\n",args.len(),args.join(", ")));if arguments.len()>18_000{break}}
 if arguments.is_empty(){arguments.push_str("No direct calls were available for bounded argument provenance.\n")}
 format!("AArch64 PCS signature evidence: x0-x7 are integer/pointer argument and result registers. Incoming parameter candidates are entry-block uses before a modeled definition; outgoing arguments are nearby modeled definitions; cast rows require exact extension encodings. Value shapes do not distinguish pointers from same-width integers. No source-level types or callee prototypes are inferred.\n\nIncoming parameter candidates\n=============================\n{}\nOutgoing call arguments\n=======================\n{}\nExplicit integer casts\n======================\n{}",function_parameter_candidates(starts,code,base,symbols),arguments,explicit_cast_evidence(code,base))
}

fn annotated_disassembly(code:&[u8],base:u64,symbols:&[(u64,String)],relocations:&[(u64,String,u32,bool,u32,Option<u64>)])->String{
 let reloc_by_site=relocations.iter().map(|r|(r.0,r)).collect::<std::collections::BTreeMap<_,_>>();let mut out=String::new();for (i,ch) in code.chunks_exact(4).enumerate().take(4000){let pc=base+i as u64*4;if let Some(name)=symbol_name_at(symbols,pc){out.push_str(&format!("\n{pc:#018x} <{name}>:\n"));}let w=le32(ch);let mut asm_buf=[0 as c_char;192];unsafe{re_arm64_decode(w,pc,asm_buf.as_mut_ptr(),asm_buf.len())};let asm=unsafe{CStr::from_ptr(asm_buf.as_ptr())}.to_string_lossy();out.push_str(&format!("  {pc:#018x}  {w:08x}  {asm}"));if let Some(r)=reloc_by_site.get(&pc).copied(){let (_,name,_,_,_,target)=r;out.push_str(&format!("  ; reloc -> {name}"));if let Some(t)=target{out.push_str(&format!(" @ {t:#018x}"));}}else if let Some((target,kind,_))=branch_target(w,pc){if let Some(name)=symbol_name_at(symbols,target){out.push_str(&format!("  ; {kind} -> {name}"));}}out.push('\n');if out.len()>60_000{break}}out
}

fn classify_jump_candidate(code:&[u8],base:u64,pc:u64,reg:u32)->&'static str{
 let idx=pc.saturating_sub(base) as usize;if idx<4{return "unresolved indirect branch"}let mut strong=false;for back in [4usize,8,12,16]{if idx<back{continue}let w=le32(&code[idx-back..idx-back+4]);let rd=w&31;let is_load=(w&0x3f000000)==0x39000000&&((w>>22)&3)!=0;if is_load&&rd==reg{strong=true;break}}if strong{"strong jump-table candidate: branch register has a nearby modeled load definition; table provenance/bounds still unproven"}else{"unresolved indirect branch; insufficient local provenance for jump-table promotion"}
}

fn arm64_address_value(word:u32,pc:u64)->Option<(u32,u64)>{
 if word&0x1f00_0000!=0x1000_0000{return None}
 let immlo=(word>>29)&3;let immhi=(word>>5)&0x7ffff;let raw=((immhi<<2)|immlo)as i64;
 let signed=if raw&(1<<20)!=0{raw-(1<<21)}else{raw};let page=word>>31!=0;
 let origin=if page{pc&!0xfff}else{pc};let displacement=if page{signed.checked_mul(4096)?}else{signed};
 Some((word&31,origin.wrapping_add_signed(displacement)))
}

fn arm64_add_immediate(word:u32)->Option<(u32,u32,u64)>{
 if word&0x7f00_0000!=0x1100_0000||word&(1<<29)!=0{return None}
 let mut immediate=((word>>10)&0xfff)as u64;if word&(1<<22)!=0{immediate=immediate.checked_shl(12)?;}
 Some((word&31,(word>>5)&31,immediate))
}

fn arm64_indexed_jump_load(word:u32)->Option<(u32,u32,u32,bool)>{
 if word&0x3f20_0c00!=0x3820_0800||word&(1<<12)==0{return None}
 let size=(word>>30)&3;let opc=(word>>22)&3;let option=(word>>13)&7;
 if option!=2&&option!=3{return None}
 let (width,signed)=match(size,opc){(3,1)=>(8,false),(2,2)=>(4,true),_=>return None};
 Some((word&31,(word>>5)&31,width,signed))
}

/// Recover a tightly bounded ARM64 indexed table only when a scaled indexed
/// load feeds BR directly or through `ADD base, base, loaded-offset`. Every
/// emitted target must land in the admitted executable mapping.
fn jump_table_entries(file:&mut File,secs:&[SectionInfo],size:u64,code:&[u8],base:u64,pc:u64,branch_reg:u32,scan_end:u64)->io::Result<Option<String>>{
 let index=pc.saturating_sub(base)as usize/4;if index>=code.len()/4{return Ok(None)}
 let mut load_index=None;let mut relative_base=None;
 for candidate in (index.saturating_sub(4)..index).rev(){
  let word=le32(&code[candidate*4..candidate*4+4]);let Some((rd,rn,rm,wide))=shifted_add_register(word)else{continue};
  if rd==branch_reg&&rn!=31&&rm!=31&&wide{
   for load in (candidate.saturating_sub(3)..candidate).rev(){let Some((loaded,table_reg,width,signed))=arm64_indexed_jump_load(le32(&code[load*4..load*4+4]))else{continue};if loaded==rm&&table_reg==rn&&width==4&&signed{load_index=Some(load);relative_base=Some(table_reg);break}}
   if load_index.is_some(){break}
  }
 }
 let (load_index,table_reg,width,signed)=if let Some(load_index)=load_index{
  (load_index,relative_base.unwrap(),4,true)
 }else{
  let mut found=None;for load in (index.saturating_sub(3)..index).rev(){if let Some((loaded,table_reg,width,signed))=arm64_indexed_jump_load(le32(&code[load*4..load*4+4])){if loaded==branch_reg{found=Some((load,table_reg,width,signed));break}}}let Some(found)=found else{return Ok(None)};found
 };
 let mut table_address=None;
 let search_start=load_index.saturating_sub(6);
 for add_index in (search_start..load_index).rev(){
  let add_word=le32(&code[add_index*4..add_index*4+4]);let Some((rd,rn,immediate))=arm64_add_immediate(add_word)else{continue};if rd!=table_reg{continue}
  for address_index in (add_index.saturating_sub(5)..add_index).rev(){let address_word=le32(&code[address_index*4..address_index*4+4]);let Some((dest,value))=arm64_address_value(address_word,base+address_index as u64*4)else{continue};if dest==rn{table_address=value.checked_add(immediate);break}}
  if table_address.is_some(){break}
 }
 if table_address.is_none(){for address_index in (search_start..load_index).rev(){let word=le32(&code[address_index*4..address_index*4+4]);if let Some((dest,value))=arm64_address_value(word,base+address_index as u64*4){if dest==table_reg{table_address=Some(value);break}}}}
 let Some(table_address)=table_address else{return Ok(None)};
 let Some(section)=section_for_va(secs,table_address)else{return Ok(None)};if section.offset>size||section.size>size-section.offset{return Ok(None)}
 let table_offset=table_address.checked_sub(section.addr).and_then(|rel|section.offset.checked_add(rel));let Some(table_offset)=table_offset else{return Ok(None)};
 if table_offset>=size{return Ok(None)}
 let section_entries=((section.addr+section.size).saturating_sub(table_address)/(width as u64))as usize;let count=section_entries.min(32);
 if count==0{return Ok(None)}
 let index_register=(le32(&code[load_index*4..load_index*4+4])>>16)&31;
 let index_provenance = if index_register != 31 {
  // Check nearby loads for index register origin
  let mut origin = String::new();
  for li in (load_index.saturating_sub(4)..load_index).rev() {
    let w = le32(&code[li*4..li*4+4]);
    let rn = (w >> 5) & 31;
    // LDR (indexed) pattern: 0xF8xxxxxx with Rn in bits 20-16
    if (w & 0xfc00_0000) == 0xf800_0000 {
      let imm = ((w & 0x03ff_0000) >> 10) as i64;
      if rn == index_register { origin = format!("LDRw-indexed@{imm:#x}"); break; }
    }
  }
  if origin.is_empty() { "unknown".into() } else { origin }
} else { "x31".into() };
 let mut out=format!("table={table_address:#018x} format={} entry_width={width} table_index=x{index_register} index-provenance={} scan_limit=32 confidence=candidate\n",index_provenance,if signed{"table-relative signed offsets"}else{"absolute pointers"});let mut valid=0usize;let mut stopped=None;
 for case in 0..count{let Some(offset)=table_offset.checked_add(case as u64*width as u64)else{stopped=Some((case,"file offset overflow"));break};if offset>size||width as u64>size-offset{stopped=Some((case,"entry exceeds file"));break}let entry=read_exact_range(file,offset,width as usize)?;let raw=if width==8{le64(&entry)}else{le32(&entry)as u64};let target=if signed{table_address.checked_add_signed((raw as u32 as i32)as i64)}else{Some(raw)};
  let Some(target)=target.filter(|target|*target>=base&&*target<scan_end&&(*target-base)%4==0)else{stopped=Some((case,"entry target is outside admitted executable bytes or is unaligned"));break};valid+=1;out.push_str(&format!("  case-index={case} entry={offset:#x} target={target:#018x} validation=executable-aligned\n"));
 }
 if valid==0{return Ok(None)}
 if let Some((case,reason))=stopped{out.push_str(&format!("unresolved-entry case-index={case} reason={reason} index-provenance={}\n",index_provenance));}else if section_entries>32{out.push_str(&format!("unresolved-entry case-index=32 reason=32-entry analysis cap reached index-provenance={}\n",index_provenance));}else{out.push_str(&format!("enumeration stopped at containing-section boundary index-provenance={}\n",index_provenance));}
 out.push_str(&format!("validated_entries={valid}; table extent is bounded by its containing section and 32-entry scan cap; case mapping and original source switch remain unproven."));
 Ok(Some(out))
}

fn shifted_add_register(word:u32)->Option<(u32,u32,u32,bool)>{
 if word&0x7f20_0000!=0x0b00_0000||word&(1<<29)!=0||(word>>10)&0x3f!=0{return None}
 Some((word&31,(word>>5)&31,(word>>16)&31,word>>31!=0))
}


fn chained_fixups_summary(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let mut at=32usize;let mut found=None;if window.len()<32{return Ok("No load-command window available.\n".into())}let ncmd=le32(&window[16..20]);
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000034&&cs>=16{found=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize));break}at+=cs;}
 let Some((off,len))=found else{return Ok("LC_DYLD_CHAINED_FIXUPS is not present.\n".into())};
 if len<28||len>8*1024*1024||off>size||len as u64>size-off{return Ok("LC_DYLD_CHAINED_FIXUPS is present but its payload is outside the validated file range.\n".into())}
 let data=read_exact_range(file,off,len)?;let version=le32(&data[0..4]);let starts_off=le32(&data[4..8]);let imports_off=le32(&data[8..12]);let symbols_off=le32(&data[12..16]);let imports_count=le32(&data[16..20]);let imports_format=le32(&data[20..24]);let symbols_format=le32(&data[24..28]);
 let valid=(starts_off as usize) <= len&&(imports_off as usize) <= len&&(symbols_off as usize) <= len;
 Ok(format!("LC_DYLD_CHAINED_FIXUPS\n  file={off:#x} size={len:#x}\n  fixups_version={version}\n  starts_offset={starts_off:#x}\n  imports_offset={imports_off:#x} count={imports_count} format={imports_format}\n  symbols_offset={symbols_off:#x} format={symbols_format}\n  payload_offsets_valid={}\n\nContainer/header validation is complete; pointer-chain walking remains evidence-gated until per-segment starts/page records validate.\n",valid))
}
fn objc_protocol_property_graph(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;let mut methods=0usize;let mut properties=0usize;
 for sec in secs.iter().filter(|s|s.sect=="__objc_protolist") {if sec.offset>size||sec.size>size-sec.offset||sec.size>2*1024*1024{continue}let bytes=read_exact_range(file,sec.offset,sec.size as usize)?;for p in bytes.chunks_exact(8).take(20_000){let target=le64(p);if target==0{continue}objc_protocol_object(file,&secs,size,target,&mut out,&mut count)?;}}
 // Protocol references are useful even when the referenced protocol object is in a dylib.
 for sec in secs.iter().filter(|s|s.sect=="__objc_protorefs") {if sec.offset>size||sec.size>size-sec.offset||sec.size>2*1024*1024{continue}let bytes=read_exact_range(file,sec.offset,sec.size as usize)?;for (i,p) in bytes.chunks_exact(8).enumerate().take(20_000){let target=le64(p);if target==0{continue}let site=sec.addr+i as u64*8;if let Some(ts)=section_for_va(&secs,target){out.push_str(&format!("reference {},{} {site:#018x} -> {target:#018x} {},{} confidence=validated\n",sec.seg,sec.sect,ts.seg,ts.sect));count+=1;if out.len()>60_000{return Ok(out)}}}}
 for sec in secs.iter().filter(|s|s.sect=="__objc_catlist") {if sec.offset>size||sec.size>size-sec.offset||sec.size>2*1024*1024{continue}let bytes=read_exact_range(file,sec.offset,sec.size as usize)?;for (i,p) in bytes.chunks_exact(8).enumerate().take(20_000){let cat=le64(p);let Some(c)=read_va(file,&secs,size,cat,48)? else{continue};let name_va=le64(&c[..8]);let cls=le64(&c[8..16]);let name=if name_va!=0{if let Some(s)=section_for_va(&secs,name_va){read_cstr_at(file,s,name_va,512)?.unwrap_or_else(||"<category name unresolved>".into())}else{"<category name unresolved>".into()}}else{"<category name unresolved>".into()};let class_name=objc_class_name(file,&secs,size,cls)?;let protocols_va=le64(&c[32..40]);let conformances=objc_protocol_names(file,&secs,size,protocols_va)?;let resolved_class=class_name.map(|value|format!(" class_name={value}")).unwrap_or_default();let conforms=if conformances.is_empty(){String::new()}else{format!(" conforms={}",conformances.join(","))};out.push_str(&format!("category {cat:#018x} name={name} class={cls:#018x}{resolved_class}{conforms} confidence=validated\n"));count+=1;objc_method_list(file,&secs,size,le64(&c[16..24]),"category-instance",None,&[],&mut out,&mut methods)?;objc_method_list(file,&secs,size,le64(&c[24..32]),"category-class",None,&[],&mut out,&mut methods)?;objc_protocol_list(file,&secs,size,protocols_va,&mut out,&mut count)?;objc_property_list(file,&secs,size,le64(&c[40..48]),&mut out,&mut properties)?;if out.len()>60_000{return Ok(out)}let _=i;}}
 if count==0{out.push_str("No validated Objective-C protocol/category pointer edges were recovered in the bounded pass.\n");}else{out.push_str(&format!("\nValidated Objective-C protocol/category objects: {count}; category methods: {methods}; properties: {properties}.\n"));}Ok(out)
}
fn swift_field_graph(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;
 for sec in secs.iter().filter(|s|s.sect=="__swift5_fieldmd"){if sec.offset>size||sec.size>size-sec.offset||sec.size>4*1024*1024{continue}let data=read_exact_range(file,sec.offset,sec.size as usize)?;let mut off=0usize;while off+16<=data.len()&&count<20_000{let base=sec.addr+off as u64;let mrel=i32::from_le_bytes(data[off..off+4].try_into().unwrap()) as i64;let srel=i32::from_le_bytes(data[off+4..off+8].try_into().unwrap()) as i64;let kind=u16::from_le_bytes(data[off+8..off+10].try_into().unwrap());let recsz=u16::from_le_bytes(data[off+10..off+12].try_into().unwrap()) as usize;let fields=le32(&data[off+12..off+16]) as usize;let resolve=|site:u64,rel:i64|->Option<u64>{if rel<0{site.checked_sub((-rel) as u64)}else{site.checked_add(rel as u64)}};out.push_str(&format!("field_descriptor {base:#018x} kind={kind} records={fields} record_size={recsz} mangled_type={} superclass={}\n",resolve(base,mrel).map(|x|format!("{x:#x}")).unwrap_or_else(||"unresolved".into()),resolve(base+4,srel).map(|x|format!("{x:#x}")).unwrap_or_else(||"none/unresolved".into())));count+=1;let span=16usize.saturating_add(recsz.saturating_mul(fields));if span==0||off+span>data.len(){break}off+=span;if out.len()>60_000{return Ok(out)}}}
 if count==0{out.push_str("No bounded Swift field descriptors were recovered from __swift5_fieldmd.\n");}Ok(out)
}
fn chained_page_starts(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let mut at=32usize;let mut found=None;if window.len()<32{return Ok("No load-command window available.\n".into())}let ncmd=le32(&window[16..20]);
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000034&&cs>=16{found=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize));break}at+=cs;}
 let Some((off,len))=found else{return Ok("LC_DYLD_CHAINED_FIXUPS is not present.\n".into())};
 if len<32||len>8*1024*1024||off>size||len as u64>size-off{return Ok("Chained-fixup payload is outside the validated file range.\n".into())}
 let data=read_exact_range(file,off,len)?;let starts=le32(&data[4..8]) as usize;if starts+4>data.len(){return Ok("Chained starts table is out of bounds.\n".into())}
 let seg_count=le32(&data[starts..starts+4]) as usize;if seg_count>4096||starts+4+seg_count*4>data.len(){return Ok("Chained starts segment table is invalid.\n".into())}
 let mut out=String::new();let mut total_pages=0usize;let mut total_starts=0usize;
 for seg in 0..seg_count{let rel=le32(&data[starts+4+seg*4..starts+8+seg*4]) as usize;if rel==0{continue}let base=starts+rel;if base+22>data.len(){continue}let struct_size=le32(&data[base..base+4]) as usize;if struct_size<22||base+struct_size>data.len(){continue}let page_size=u16::from_le_bytes(data[base+4..base+6].try_into().unwrap());let pointer_format=u16::from_le_bytes(data[base+6..base+8].try_into().unwrap());let segment_offset=le64(&data[base+8..base+16]);let max_valid=le32(&data[base+16..base+20]);let page_count=u16::from_le_bytes(data[base+20..base+22].try_into().unwrap()) as usize;if base+22+page_count*2>data.len(){continue}
  out.push_str(&format!("segment[{seg}] page_size={page_size} pointer_format={pointer_format} segment_offset={segment_offset:#x} max_valid_pointer={max_valid:#x} pages={page_count}\n"));total_pages+=page_count;
  for page in 0..page_count.min(8192){let v=u16::from_le_bytes(data[base+22+page*2..base+24+page*2].try_into().unwrap());if v==0xffff{continue}if v&0x8000!=0{out.push_str(&format!("  page[{page}] multi-start table index={} (validated marker; overflow chain not expanded)\n",v&0x7fff));}else{out.push_str(&format!("  page[{page}] chain_start={:#x} file-relative-site={:#x}\n",v,segment_offset.saturating_add((page as u64).saturating_mul(page_size as u64)).saturating_add(v as u64)));total_starts+=1;}if out.len()>60_000{return Ok(out)}}
 }
 if out.is_empty(){out.push_str("No validated per-segment chained page starts were recovered.\n");}else{out.push_str(&format!("\nValidated chained-start topology: {seg_count} segment slots, {total_pages} pages, {total_starts} direct page starts. Pointer targets are not decoded unless their format semantics are explicitly supported.\n"));}Ok(out)
}
fn swift_field_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;let resolve=|site:u64,rel:i64|->Option<u64>{if rel<0{site.checked_sub((-rel) as u64)}else{site.checked_add(rel as u64)}};
 for sec in secs.iter().filter(|s|s.sect=="__swift5_fieldmd"){if sec.offset>size||sec.size>size-sec.offset||sec.size>4*1024*1024{continue}let data=read_exact_range(file,sec.offset,sec.size as usize)?;let mut off=0usize;while off+16<=data.len()&&count<20_000{let base=sec.addr+off as u64;let recsz=u16::from_le_bytes(data[off+10..off+12].try_into().unwrap()) as usize;let fields=le32(&data[off+12..off+16]) as usize;if recsz<12{break}let records=off+16;if records.saturating_add(recsz.saturating_mul(fields))>data.len(){break}out.push_str(&format!("descriptor {base:#018x} fields={fields} record_size={recsz}\n"));for i in 0..fields.min(4096){let ro=records+i*recsz;let rva=sec.addr+ro as u64;let flags=le32(&data[ro..ro+4]);let trel=i32::from_le_bytes(data[ro+4..ro+8].try_into().unwrap()) as i64;let nrel=i32::from_le_bytes(data[ro+8..ro+12].try_into().unwrap()) as i64;let type_va=resolve(rva+4,trel);let name_va=resolve(rva+8,nrel);let type_name=if let Some(v)=type_va{if let Some(ss)=section_for_va(&secs,v){read_cstr_at(file,ss,v,512)?.unwrap_or_else(||format!("@{v:#x}"))}else{"<unmapped>".into()}}else{"<invalid>".into()};let field_name=if let Some(v)=name_va{if let Some(ss)=section_for_va(&secs,v){read_cstr_at(file,ss,v,512)?.unwrap_or_else(||format!("@{v:#x}"))}else{"<unmapped>".into()}}else{"<invalid>".into()};out.push_str(&format!("  field[{i}] {rva:#018x} flags={flags:#x} name={field_name} type={type_name}\n"));count+=1;if out.len()>60_000{return Ok(out)}}off=records+recsz*fields;}
 }
 if count==0{out.push_str("No individually validated Swift field records were recovered.\n");}else{out.push_str(&format!("\nValidated Swift field variables: {count}. Names/types are sourced from bounded __swift5_fieldmd relative pointers and mapped C strings; storage layout, runtime offsets, and source-level declarations remain unresolved.\n"));}Ok(out)
}
fn predecessor_value_flow(code:&[u8],base:u64,leaders:&[u64],edges:&[(u64,u64,&str)],end:u64)->String{
 let limit=leaders.len().min(4000);let mut pred=vec![Vec::<usize>::new();limit];
 for &(from,to,k) in edges{if k=="call"{continue}let Some(tb)=block_index(leaders,to) else{continue};if tb>=limit{continue}let Some(fb)=block_index(leaders,from) else{continue};let row=&mut pred[tb];if row.len()<128&&!row.contains(&fb){row.push(fb);}}
 let local=(0..leaders.len().min(4001)).map(|b|{let pe=leaders.get(b+1).copied().unwrap_or(end);last_defs_for_block(code,base,leaders[b],pe)}).collect::<Vec<_>>();
 let mut out=String::new();for bi in 0..limit{let start=leaders[bi];let preds=&pred[bi];if preds.is_empty(){continue}out.push_str(&format!("B{bi:04} {start:#018x} predecessors="));for (pi,pb) in preds.iter().enumerate(){if pi>0{out.push(',')}out.push_str(&format!("B{pb:04}"));}out.push('\n');for r in 0..8{let mut defs=Vec::new();for &pb in preds{if let Some(row)=local.get(pb){if let Some(d)=row[r]{defs.push((pb,d));}}}defs.sort_unstable();defs.dedup();if !defs.is_empty(){out.push_str(&format!("  x{r} <- {}\n",defs.iter().map(|(b,d)|format!("B{b:04}@{d:#x}")).collect::<Vec<_>>().join(" | ")));}}if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No predecessor-reaching modeled argument-register definitions were recovered.\n");}out
}

fn interblock_value_flow(calls:&[(u64,u64,u64)],code:&[u8],base:u64,leaders:&[u64],symbols:&[(u64,String)])->String{
 let mut out=String::new();for &(func,site,target) in calls.iter().take(4000){let block=block_index(leaders,site).unwrap_or(0);let bs=leaders.get(block).copied().unwrap_or(base);let defs=last_defs_for_block(code,base,bs,site);let name=symbol_name_at(symbols,target).unwrap_or("<direct target>");let args=(0..8).filter_map(|r|defs[r].map(|d|format!("x{r}@{d:#x}"))).collect::<Vec<_>>().join(", ");out.push_str(&format!("call {site:#018x} func={func:#018x} -> {target:#018x} {name} block=B{block:04} reaching=[{args}]\n"));if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No direct calls with modeled reaching argument definitions were found.\n");}out
}


fn chained_pointer_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut dataoff=0u64;let mut datasize=0u64;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000034&&cs>=16{dataoff=le32(&window[at+8..at+12]) as u64;datasize=le32(&window[at+12..at+16]) as u64;break}at+=cs;}
 if dataoff==0||datasize<28||dataoff>size||datasize>size-dataoff{return Ok("No validated LC_DYLD_CHAINED_FIXUPS payload is available for pointer walking.\n".into())}
 let data=read_exact_range(file,dataoff,datasize.min(16*1024*1024) as usize)?;if data.len()<28{return Ok("Chained-fixup payload is too small.\n".into())}
 let starts_off=le32(&data[4..8]) as usize;if starts_off+4>data.len(){return Ok("Chained-fixup starts table is out of bounds.\n".into())}
 let seg_count=le32(&data[starts_off..starts_off+4]) as usize;if starts_off+4+seg_count*4>data.len(){return Ok("Chained-fixup segment-offset table is out of bounds.\n".into())}
 let image_base=secs.iter().filter(|x|x.seg=="__TEXT").map(|x|x.addr).min().or_else(||secs.iter().map(|x|x.addr).min()).unwrap_or(0);
 let mut segmaps=Vec::<(u64,u64,u64,u64)>::new();let mut ca=32usize;for _ in 0..ncmd{if ca+8>window.len(){break}let cmd=le32(&window[ca..ca+4]);let cs=le32(&window[ca+4..ca+8]) as usize;if cs<8||ca+cs>window.len(){break}if cmd==0x19&&cs>=72{segmaps.push((le64(&window[ca+24..ca+32]),le64(&window[ca+32..ca+40]),le64(&window[ca+40..ca+48]),le64(&window[ca+48..ca+56])))}ca+=cs;}
 for si in 0..seg_count.min(256){let rel=le32(&data[starts_off+4+si*4..starts_off+8+si*4]) as usize;if rel==0{continue}let so=starts_off+rel;if so+22>data.len(){continue}let struct_size=le32(&data[so..so+4]) as usize;if struct_size<22||so+struct_size>data.len(){continue}let page_size=u16::from_le_bytes([data[so+4],data[so+5]]) as u64;let fmt=u16::from_le_bytes([data[so+6],data[so+7]]);let seg_offset=le64(&data[so+8..so+16]);let page_count=u16::from_le_bytes([data[so+20],data[so+21]]) as usize;if so+22+page_count*2>data.len(){continue}
  let (stride,label)=match fmt{2=>(4u64,"DYLD_CHAINED_PTR_64"),6=>(4u64,"DYLD_CHAINED_PTR_64_OFFSET"),1|7|9|10|12=>{let n=match fmt{1=>"DYLD_CHAINED_PTR_ARM64E",7=>"DYLD_CHAINED_PTR_ARM64E_KERNEL",9=>"DYLD_CHAINED_PTR_ARM64E_USERLAND",10=>"DYLD_CHAINED_PTR_ARM64E_FIRMWARE",12=>"DYLD_CHAINED_PTR_ARM64E_USERLAND24",_=>"ARM64E"};out.push_str(&format!("segment[{si}] pointer_format={fmt} {n} authenticated-capable; format recognized but PAC payload decode remains conservative\n"));continue},_=>{out.push_str(&format!("segment[{si}] pointer_format={fmt} unsupported-for-node-decode\n"));continue}};
  let Some((_seg_vm,_,seg_file,seg_filesz))=segmaps.iter().copied().find(|(vm,_,_,_)|vm.saturating_sub(image_base)==seg_offset) else{out.push_str(&format!("segment[{si}] segment_offset={seg_offset:#x} has no validated LC_SEGMENT_64 mapping\n"));continue};
  for pi in 0..page_count.min(65535){let ps=u16::from_le_bytes([data[so+22+pi*2],data[so+23+pi*2]]);if ps==0xffff||ps&0x8000!=0{continue}let page_rel=pi as u64*page_size; if page_rel>=seg_filesz{continue}let mut fileoff=seg_file.saturating_add(page_rel).saturating_add(ps as u64);let page_end=seg_file.saturating_add((page_rel+page_size).min(seg_filesz));let mut hops=0usize;
   while fileoff+8<=size&&fileoff+8<=page_end&&hops<4096{let raw=read_exact_range(file,fileoff,8)?;let v=le64(&raw);let bind=(v>>63)&1;let next=(v>>51)&0xfff;let target36=v&0x0000_000f_ffff_ffff;let high8=(v>>36)&0xff;let semantic=if bind==1{format!("bind ordinal={} addend={}",v&0x00ff_ffff,(v>>24)&0xff)}else if fmt==6{format!("rebase image-offset={:#x} high8={:#x} candidate-va={:#018x}",target36,high8,image_base.saturating_add(target36))}else{format!("rebase target={:#018x} (target36={:#x} high8={:#x})",(high8<<56)|target36,target36,high8)};out.push_str(&format!("seg[{si}] page[{pi}] file+{fileoff:#x} {label} raw={v:#018x} {semantic} next={}\n",next*stride));count+=1;if out.len()>60_000{return Ok(out)}if next==0{break}let delta=next.saturating_mul(stride);if delta==0||fileoff.saturating_add(delta)<=fileoff{break}fileoff=fileoff.saturating_add(delta);hops+=1;}
  }
 }
 if count==0{out.push_str("No individually decoded supported chained-pointer nodes were recovered; unsupported formats and multi-start overflow pages remain explicit evidence only.\n");}Ok(out)
}


fn chained_multistart_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut dataoff=0u64;let mut datasize=0u64;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000034&&cs>=16{dataoff=le32(&window[at+8..at+12]) as u64;datasize=le32(&window[at+12..at+16]) as u64;break}at+=cs;}
 if dataoff==0||datasize<28||dataoff>size||datasize>size-dataoff{return Ok("No validated LC_DYLD_CHAINED_FIXUPS payload is available.\n".into())}
 let data=read_exact_range(file,dataoff,datasize.min(16*1024*1024) as usize)?;let starts=le32(&data[4..8]) as usize;if starts+4>data.len(){return Ok("Chained starts table is out of bounds.\n".into())}
 let seg_count=le32(&data[starts..starts+4]) as usize;if starts+4+seg_count*4>data.len(){return Ok("Chained starts segment table is invalid.\n".into())}
 let mut out=String::new();let mut expanded=0usize;
 for seg in 0..seg_count.min(256){let rel=le32(&data[starts+4+seg*4..starts+8+seg*4]) as usize;if rel==0{continue}let base=starts+rel;if base+22>data.len(){continue}let struct_size=le32(&data[base..base+4]) as usize;if struct_size<22||base+struct_size>data.len(){continue}let page_size=u16::from_le_bytes(data[base+4..base+6].try_into().unwrap());let page_count=u16::from_le_bytes(data[base+20..base+22].try_into().unwrap()) as usize;let starts_base=base+22;let overflow_base=starts_base.saturating_add(page_count.saturating_mul(2));if overflow_base>base+struct_size{continue}
  for page in 0..page_count.min(8192){let po=starts_base+page*2;if po+2>base+struct_size{break}let v=u16::from_le_bytes(data[po..po+2].try_into().unwrap());if v==0xffff||v&0x8000==0{continue}let mut oi=(v&0x7fff) as usize;let max_entries=(base+struct_size-overflow_base)/2;let mut local=0usize;out.push_str(&format!("segment[{seg}] page[{page}] multi-start index={oi} page_size={page_size}\n"));while oi<max_entries&&local<4096{let eo=overflow_base+oi*2;let e=u16::from_le_bytes(data[eo..eo+2].try_into().unwrap());let start=e&0x7fff;out.push_str(&format!("  start[{local}]={start:#x}{}\n",if e&0x8000!=0{" last"}else{""}));expanded+=1;local+=1;oi+=1;if e&0x8000!=0{break}if out.len()>60_000{return Ok(out)}}if local==0{out.push_str("  unresolved: overflow index is outside the validated starts structure\n");}}
 }
 if expanded==0{out.push_str("No validated multi-start overflow entries were expanded.\n");}else{out.push_str(&format!("\nExpanded {expanded} validated multi-start chain starts. Entries are starts only; pointer semantics are decoded separately by format.\n"));}Ok(out)
}

fn arm64e_pointer_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut dataoff=0u64;let mut datasize=0u64;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000034&&cs>=16{dataoff=le32(&window[at+8..at+12]) as u64;datasize=le32(&window[at+12..at+16]) as u64;break}at+=cs;}
 if dataoff==0||datasize<28||dataoff>size||datasize>size-dataoff{return Ok("No validated chained-fixup payload is available for ARM64e decoding.\n".into())}
 let data=read_exact_range(file,dataoff,datasize.min(16*1024*1024) as usize)?;let starts=le32(&data[4..8]) as usize;if starts+4>data.len(){return Ok("Chained starts table is invalid.\n".into())}let seg_count=le32(&data[starts..starts+4]) as usize;if starts+4+seg_count*4>data.len(){return Ok("Chained starts segment table is invalid.\n".into())}
 let image_base=secs.iter().filter(|x|x.seg=="__TEXT").map(|x|x.addr).min().or_else(||secs.iter().map(|x|x.addr).min()).unwrap_or(0);let mut segmaps=Vec::<(u64,u64,u64,u64)>::new();let mut ca=32usize;for _ in 0..ncmd{if ca+8>window.len(){break}let cmd=le32(&window[ca..ca+4]);let cs=le32(&window[ca+4..ca+8]) as usize;if cs<8||ca+cs>window.len(){break}if cmd==0x19&&cs>=72{segmaps.push((le64(&window[ca+24..ca+32]),le64(&window[ca+32..ca+40]),le64(&window[ca+40..ca+48]),le64(&window[ca+48..ca+56])))}ca+=cs;}
 let mut out=String::new();let mut count=0usize;
 for si in 0..seg_count.min(256){let rel=le32(&data[starts+4+si*4..starts+8+si*4]) as usize;if rel==0{continue}let so=starts+rel;if so+22>data.len(){continue}let struct_size=le32(&data[so..so+4]) as usize;if struct_size<22||so+struct_size>data.len(){continue}let page_size=u16::from_le_bytes(data[so+4..so+6].try_into().unwrap()) as u64;let fmt=u16::from_le_bytes(data[so+6..so+8].try_into().unwrap());if !matches!(fmt,1|7|9|10|12){continue}let seg_offset=le64(&data[so+8..so+16]);let page_count=u16::from_le_bytes(data[so+20..so+22].try_into().unwrap()) as usize;if so+22+page_count*2>data.len(){continue}let Some((_svm,_,sfile,sfilesz))=segmaps.iter().copied().find(|(vm,_,_,_)|vm.saturating_sub(image_base)==seg_offset) else{continue};
  for pi in 0..page_count.min(8192){let ps=u16::from_le_bytes(data[so+22+pi*2..so+24+pi*2].try_into().unwrap());if ps==0xffff||ps&0x8000!=0{continue}let page_rel=pi as u64*page_size;if page_rel>=sfilesz{continue}let mut fileoff=sfile.saturating_add(page_rel).saturating_add(ps as u64);let page_end=sfile.saturating_add((page_rel+page_size).min(sfilesz));let mut hops=0usize;while fileoff+8<=size&&fileoff+8<=page_end&&hops<4096{let v=le64(&read_exact_range(file,fileoff,8)?);let auth=(v>>63)&1;let bind=(v>>62)&1;let next=(v>>51)&0x7ff;let detail=if auth==1{let diversity=(v>>32)&0xffff;let addr_div=(v>>48)&1;let key=(v>>49)&0x3;if bind==1{format!("auth-bind ordinal={} diversity={:#x} addrDiv={} key={}",v&0xffff,diversity,addr_div,key)}else{format!("auth-rebase target32={:#x} candidate-va={:#018x} diversity={:#x} addrDiv={} key={}",v&0xffff_ffff,image_base.saturating_add(v&0xffff_ffff),diversity,addr_div,key)}}else if bind==1{let raw_add=(v>>32)&0x7ffff;let addend=if raw_add&0x40000!=0{(raw_add as i64)-(1<<19)}else{raw_add as i64};format!("bind ordinal={} addend={}",if fmt==12{v&0xff_ffff}else{v&0xffff},addend)}else{let target=v&0x7ff_ffff_ffff;let high8=(v>>43)&0xff;format!("rebase target43={:#x} high8={:#x}",target,high8)};out.push_str(&format!("seg[{si}] page[{pi}] file+{fileoff:#x} fmt={fmt} raw={v:#018x} {detail} next={}\n",next*8));count+=1;if out.len()>60_000{return Ok(out)}if next==0{break}let delta=next.saturating_mul(8);if delta==0||fileoff.saturating_add(delta)<=fileoff{break}fileoff=fileoff.saturating_add(delta);hops+=1;}}
 }
 if count==0{out.push_str("No directly started ARM64e chained-pointer nodes were decoded. Multi-start pages are listed separately until their starts are fed into the walker.\n");}Ok(out)
}



fn dyld_import_entries(file:&mut File,window:&[u8],size:u64)->io::Result<Vec<(usize,String,i64,i32,bool)>>{
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut dataoff=0u64;let mut datasize=0u64;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000034&&cs>=16{dataoff=le32(&window[at+8..at+12]) as u64;datasize=le32(&window[at+12..at+16]) as u64;break}at+=cs;}
 if dataoff==0||datasize<28||dataoff>size||datasize>size-dataoff{return Ok(Vec::new())}
 let data=read_exact_range(file,dataoff,datasize.min(16*1024*1024) as usize)?;if data.len()<28{return Ok(Vec::new())}
 let imports_off=le32(&data[8..12]) as usize;let symbols_off=le32(&data[12..16]) as usize;let count=le32(&data[16..20]) as usize;let format=le32(&data[20..24]);let symbols_format=le32(&data[24..28]);
 if count>1_000_000||imports_off>data.len()||symbols_off>data.len()||symbols_format!=0{return Ok(Vec::new())}
 let entry_size=match format{1=>4usize,2=>8,3=>16,_=>return Ok(Vec::new())};if imports_off.saturating_add(count.saturating_mul(entry_size))>data.len(){return Ok(Vec::new())}
 let mut out=Vec::new();for i in 0..count.min(200_000){let off=imports_off+i*entry_size;let (lib,weak,name_off,addend)=match format{
  1=>{let v=le32(&data[off..off+4]);(((v&0xff) as i8) as i32,((v>>8)&1)!=0,(v>>9) as usize,0i64)},
  2=>{let v=le32(&data[off..off+4]);let a=i32::from_le_bytes(data[off+4..off+8].try_into().unwrap()) as i64;(((v&0xff) as i8) as i32,((v>>8)&1)!=0,(v>>9) as usize,a)},
  3=>{let v=le64(&data[off..off+8]);let a=i64::from_le_bytes(data[off+8..off+16].try_into().unwrap());(((v&0xffff) as i16) as i32,((v>>16)&1)!=0,(v>>32) as usize,a)},
  _=>unreachable!()};
  let no=symbols_off.saturating_add(name_off);if no>=data.len(){continue}let tail=&data[no..];let end=tail.iter().position(|&b|b==0).unwrap_or(tail.len()).min(4096);if end==0{continue}let name=String::from_utf8_lossy(&tail[..end]).into_owned();out.push((i,name,addend,lib,weak));
 }
 Ok(out)
}

fn dyld_import_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let entries=dyld_import_entries(file,window,size)?;let mut out=String::new();for (ordinal,name,addend,lib,weak) in entries.iter().take(20_000){out.push_str(&format!("ordinal={ordinal} lib_ordinal={lib} weak={} addend={addend} symbol={name}\n",if *weak{"yes"}else{"no"}));if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No supported uncompressed dyld chained-import table was recovered. Supported formats: DYLD_CHAINED_IMPORT, _ADDEND, and _ADDEND64.\n");}Ok(out)
}

fn arm64e_bind_symbol_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let imports=dyld_import_entries(file,window,size)?;let pointer_text=arm64e_pointer_records(file,window,size)?;let mut out=String::new();
 for line in pointer_text.lines().filter(|l|l.contains("bind ordinal=")){let Some(pos)=line.find("ordinal=") else{continue};let rest=&line[pos+8..];let digits=rest.chars().take_while(|c|c.is_ascii_digit()).collect::<String>();let Ok(ord)=digits.parse::<usize>() else{continue};if let Some((_,name,addend,lib,weak))=imports.iter().find(|e|e.0 == ord){out.push_str(&format!("{line} -> symbol={name} import_addend={addend} lib_ordinal={lib} weak={}\n",if *weak{"yes"}else{"no"}));}else{out.push_str(&format!("{line} -> import ordinal unresolved\n"));}if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No ARM64e bind nodes could be joined to a supported dyld import table.\n");}Ok(out)
}


fn dylib_ordinal_names(window:&[u8])->Vec<(i32,String)>{
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut ordinal=1i32;let mut out=Vec::new();
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}
  if matches!(cmd,0x0c|0x18|0x1f|0x80000018|0x8000001f|0x80000023)&&cs>=24{let no=le32(&window[at+8..at+12]) as usize;if no<cs{let tail=&window[at+no..at+cs];let end=tail.iter().position(|&b|b==0).unwrap_or(tail.len());let name=String::from_utf8_lossy(&tail[..end]).into_owned();if !name.is_empty(){out.push((ordinal,name));ordinal+=1;}}}
  at+=cs;
 }
 out
}
fn dylib_ordinal_records(window:&[u8])->String{
 let libs=dylib_ordinal_names(window);let mut out=String::new();for (ord,name) in libs{out.push_str(&format!("ordinal={ord} dylib={name}\n"));if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No dylib load commands with resolvable ordinal names were recovered.\n");}out
}
fn resolved_dyld_import_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let imports=dyld_import_entries(file,window,size)?;let libs=dylib_ordinal_names(window);let mut out=String::new();
 for (ordinal,name,addend,lib,weak) in imports.iter().take(20_000){let libname=match *lib{0=>"<self>".to_string(),-1=>"<main-executable>".to_string(),-2=>"<flat-lookup>".to_string(),-3=>"<weak-lookup>".to_string(),n if n>0=>libs.iter().find(|(o,_)|*o==n).map(|x|x.1.clone()).unwrap_or_else(||format!("<dylib-ordinal-{n}-unresolved>")),n=>format!("<special-{n}>")};out.push_str(&format!("import[{ordinal}] {libname}::{name} addend={addend} weak={}\n",if *weak{"yes"}else{"no"}));if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No dyld imports could be joined to a validated dylib ordinal/name table.\n");}Ok(out)
}
fn authenticated_bind_targets(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let imports=dyld_import_entries(file,window,size)?;let libs=dylib_ordinal_names(window);let pointer_text=arm64e_pointer_records(file,window,size)?;let mut out=String::new();
 for line in pointer_text.lines().filter(|l|l.contains("bind ordinal=")){let Some(pos)=line.find("ordinal=") else{continue};let digits=line[pos+8..].chars().take_while(|c|c.is_ascii_digit()).collect::<String>();let Ok(ord)=digits.parse::<usize>() else{continue};if let Some((_,name,import_addend,lib,weak))=imports.iter().find(|e|e.0 == ord){let node_addend=line.find("addend=").and_then(|p|line[p+7..].split_whitespace().next()).and_then(|x|x.parse::<i64>().ok()).unwrap_or(0);let effective=(*import_addend).saturating_add(node_addend);let libname=if *lib>0{libs.iter().find(|(o,_)|*o==*lib).map(|x|x.1.as_str()).unwrap_or("<dylib-unresolved>")}else{"<special>"};out.push_str(&format!("ordinal={ord} target={libname}::{name} import_addend={import_addend} node_addend={node_addend} effective_addend={effective} weak={}\n",if *weak{"yes"}else{"no"}));}else{out.push_str(&format!("ordinal={ord} unresolved import\n"));}if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No authenticated bind nodes could be composed with dyld imports.\n");}Ok(out)
}

fn swift_field_ownership(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::new();let mut count=0usize;
 for sec in secs.iter().filter(|s|s.sect=="__swift5_fieldmd"){if sec.offset>size||sec.size>size-sec.offset||sec.size>8*1024*1024{continue}let data=read_exact_range(file,sec.offset,sec.size as usize)?;let mut off=0usize;while off+16<=data.len(){let base=sec.addr+off as u64;let type_rel=i32::from_le_bytes(data[off..off+4].try_into().unwrap()) as i64;let super_rel=i32::from_le_bytes(data[off+4..off+8].try_into().unwrap()) as i64;let kind=u16::from_le_bytes(data[off+8..off+10].try_into().unwrap());let recsz=u16::from_le_bytes(data[off+10..off+12].try_into().unwrap()) as usize;let fields=le32(&data[off+12..off+16]) as usize;if recsz<12||recsz>4096||fields>65535{break}let owner_va=resolve(base,type_rel);let super_va=resolve(base+4,super_rel);let owner=owner_va.and_then(|v|section_for_va(&secs,v).and_then(|ss|read_cstr_at(file,ss,v,1024).ok().flatten())).unwrap_or_else(||"<owner-unresolved>".into());let super_name=super_va.and_then(|v|section_for_va(&secs,v).and_then(|ss|read_cstr_at(file,ss,v,1024).ok().flatten())).unwrap_or_else(||"<none-or-unresolved>".into());out.push_str(&format!("descriptor {base:#018x} kind={kind} owner={owner} superclass={super_name} fields={fields}\n"));let records=off+16;if records+recsz.saturating_mul(fields)>data.len(){break}for i in 0..fields.min(4096){let ro=records+i*recsz;let rva=sec.addr+ro as u64;let nrel=i32::from_le_bytes(data[ro+8..ro+12].try_into().unwrap()) as i64;let name=resolve(rva+8,nrel).and_then(|v|section_for_va(&secs,v).and_then(|ss|read_cstr_at(file,ss,v,512).ok().flatten())).unwrap_or_else(||"<unresolved>".into());out.push_str(&format!("  owns field[{i}] {name} @ {rva:#018x}\n"));count+=1;if out.len()>60_000{return Ok(out)}}off=records+recsz*fields;}}
 if count==0{out.push_str("No validated Swift field ownership relationships were recovered.\n");}Ok(out)
}

fn iterative_value_flow(code:&[u8],base:u64,leaders:&[u64],edges:&[(u64,u64,&str)],end:u64)->String{
 use std::collections::BTreeSet;const MAX_ITERATIVE_VALUE_BLOCKS:usize=2_048;const MAX_VALUE_FLOW_PASSES:usize=32;const MAX_VALUE_PREDECESSORS:usize=64;const MAX_REACHING_DEFS_PER_REGISTER:usize=16;let n=leaders.len();
 if n>MAX_ITERATIVE_VALUE_BLOCKS{return format!("Iterative inter-block value propagation omitted because the CFG has {n} blocks, above the {MAX_ITERATIVE_VALUE_BLOCKS}-block bounded propagation budget. Local/predecessor evidence remains available.\n")}
 let mut preds=vec![Vec::<usize>::new();n];for &(from,to,k) in edges{if k=="call"{continue}if let(Some(a),Some(b))=(block_index(leaders,from),block_index(leaders,to)){if a<n&&b<n&&preds[b].len()<MAX_VALUE_PREDECESSORS{preds[b].push(a)}}}for row in &mut preds{row.sort_unstable();row.dedup();}
 let local=(0..n).map(|b|{let s=leaders[b];let e=leaders.get(b+1).copied().unwrap_or(end);last_defs_for_block(code,base,s,e)}).collect::<Vec<_>>();let mut incoming=vec![vec![BTreeSet::<u64>::new();8];n];let mut outgoing=vec![vec![BTreeSet::<u64>::new();8];n];
 let mut converged=false;for _ in 0..MAX_VALUE_FLOW_PASSES{let mut changed=false;for b in 0..n{for r in 0..8{let mut ins=BTreeSet::new();'outer: for &p in &preds[b]{for &value in &outgoing[p][r]{ins.insert(value);if ins.len()>=MAX_REACHING_DEFS_PER_REGISTER{break 'outer}}}let mut outs=ins.clone();if let Some(d)=local[b][r]{outs.clear();outs.insert(d);}if ins!=incoming[b][r]{incoming[b][r]=ins;changed=true}if outs!=outgoing[b][r]{outgoing[b][r]=outs;changed=true}}}if !changed{converged=true;break}}
 let mut out=String::new();for b in 0..n.min(4000){let mut any=false;let mut lines=String::new();for r in 0..8{if !incoming[b][r].is_empty(){any=true;lines.push_str(&format!("  x{r} in=[{}] out=[{}]\n",incoming[b][r].iter().map(|x|format!("{x:#x}")).collect::<Vec<_>>().join(" | "),outgoing[b][r].iter().map(|x|format!("{x:#x}")).collect::<Vec<_>>().join(" | ")));}}if any{out.push_str(&format!("B{b:04} {:#018x}\n{lines}",leaders[b]));if out.len()>60_000{break}}}
 if out.is_empty(){out.push_str("No iterative inter-block modeled value sets were recovered.\n");}if !converged&&out.len()<58_000{out.push_str(&format!("Propagation stopped after the bounded {MAX_VALUE_FLOW_PASSES}-pass budget; each block accepts at most {MAX_VALUE_PREDECESSORS} predecessors and each register retains at most {MAX_REACHING_DEFS_PER_REGISTER} reaching definitions. Partial fixed-point state is explicitly bounded derived evidence.\n"));}out
}

fn indirect_call_resolution(code:&[u8],base:u64,leaders:&[u64],relocations:&[(u64,String,u32,bool,u32,Option<u64>)],symbols:&[(u64,String)])->String{
 let reloc_by_site=relocations.iter().map(|r|(r.0,r)).collect::<std::collections::BTreeMap<_,_>>();let mut out=String::new();for (i,ch) in code.chunks_exact(4).enumerate(){let pc=base+i as u64*4;let w=le32(ch);if w&0xffff_fc1f!=0xd63f_0000{continue}let reg=((w>>5)&31) as usize;let b=block_index(leaders,pc).unwrap_or(0);let bs=leaders.get(b).copied().unwrap_or(base);let defs=last_defs_for_block(code,base,bs,pc);let def=if reg<8{defs[reg]}else{None};let resolved=def.and_then(|d|reloc_by_site.get(&d).copied()).and_then(|x|x.5).map(|t|{let name=symbol_name_at(symbols,t).unwrap_or("<resolved-target>");format!("resolved {t:#018x} {name}")}).unwrap_or_else(||def.map(|d|format!("definition {d:#018x}; target unresolved")).unwrap_or_else(||"no modeled reaching definition".into()));out.push_str(&format!("BLR x{reg} @ {pc:#018x} B{b:04}: {resolved}\n"));if out.len()>60_000{break}}
 if out.is_empty(){out.push_str("No indirect BLR call sites were found in the bounded code scan.\n");}out
}


#[derive(Clone)]
#[allow(dead_code)]
struct LinkSection { sect:String, seg:String, addr:u64, size:u64, offset:u64, flags:u32, reserved1:u32, reserved2:u32 }
fn link_sections(window:&[u8])->Vec<LinkSection>{
 let mut out=Vec::new();if window.len()<32{return out}let ncmd=le32(&window[16..20]);let mut at=32usize;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}
  if cmd==0x19&&cs>=72{let nsects=le32(&window[at+64..at+68]) as usize;let mut sp=at+72;for _ in 0..nsects{if sp+80>at+cs{break}out.push(LinkSection{sect:cstr16(&window[sp..sp+16]),seg:cstr16(&window[sp+16..sp+32]),addr:le64(&window[sp+32..sp+40]),size:le64(&window[sp+40..sp+48]),offset:le32(&window[sp+48..sp+52]) as u64,flags:le32(&window[sp+64..sp+68]),reserved1:le32(&window[sp+68..sp+72]),reserved2:le32(&window[sp+72..sp+76])});sp+=80;}}
  at+=cs;
 }out
}
fn all_symbol_names(file:&mut File,window:&[u8],size:u64)->io::Result<Vec<String>>{
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut tab=None;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x2&&cs>=24{tab=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize,le32(&window[at+16..at+20]) as u64,le32(&window[at+20..at+24]) as usize));break}at+=cs;}
 let Some((so,n,stro,strlen))=tab else{return Ok(vec![])};let n=n.min(200_000);let bytes=n.saturating_mul(16);if bytes>16*1024*1024||strlen>16*1024*1024||so>size||bytes as u64>size-so||stro>size||strlen as u64>size-stro{return Ok(vec![])}
 let syms=read_exact_range(file,so,bytes)?;let strs=read_exact_range(file,stro,strlen)?;let mut names=Vec::with_capacity(n);
 for i in 0..n{let strx=le32(&syms[i*16..i*16+4]) as usize;if strx==0||strx>=strs.len(){names.push(String::new());continue}let end=strs[strx..].iter().position(|&b|b==0).map(|x|strx+x).unwrap_or(strs.len());let name=String::from_utf8_lossy(&strs[strx..end]).into_owned();names.push(if name.len()<=1024{name}else{String::new()});}Ok(names)
}
fn ascii_contains_fold(haystack:&[u8],needle:&[u8])->bool{!needle.is_empty()&&haystack.windows(needle.len()).any(|part|part.eq_ignore_ascii_case(needle))}
fn symbolication_symbol_names(file:&mut File,window:&[u8],size:u64)->io::Result<Vec<String>>{
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut tab=None;for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x2&&cs>=24{tab=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize,le32(&window[at+16..at+20]) as u64,le32(&window[at+20..at+24]) as usize));break}at+=cs;}
 let Some((so,n,stro,strlen))=tab else{return Ok(vec![])};let n=n.min(200_000);let bytes=n.saturating_mul(16);if bytes>16*1024*1024||strlen>16*1024*1024||so>size||bytes as u64>size-so||stro>size||strlen as u64>size-stro{return Ok(vec![])}
 let syms=read_exact_range(file,so,bytes)?;let strs=read_exact_range(file,stro,strlen)?;let needles:[&[u8];5]=[b"symbolication",b"cssymbol",b"cssource",b"cssymbolowner",b"cssymbolicator"];let mut out=Vec::new();
 for i in 0..n{let strx=le32(&syms[i*16..i*16+4]) as usize;if strx==0||strx>=strs.len(){continue}let end=strs[strx..].iter().position(|&b|b==0).map(|x|strx+x).unwrap_or(strs.len());let raw=&strs[strx..end];if raw.len()>1024||!needles.iter().any(|needle|ascii_contains_fold(raw,needle)){continue}out.push(String::from_utf8_lossy(raw).into_owned());if out.len()>=2048{break}}Ok(out)
}
fn indirect_symbol_data(file:&mut File,window:&[u8],size:u64)->io::Result<(String,String,Vec<(String,String,u64)>)>{
 let names=all_symbol_names(file,window,size)?;let secs=link_sections(window);let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut table=None;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0xb&&cs>=80{table=Some((le32(&window[at+56..at+60]) as u64,le32(&window[at+60..at+64]) as usize));break}at+=cs;}
 let Some((off,count))=table else{return Ok(("No LC_DYSYMTAB indirect-symbol table was found.\n".into(),"No lazy/non-lazy symbol pointer sections could be resolved without an indirect-symbol table.\n".into(),vec![]))};let count=count.min(2_000_000);let bytes=count.saturating_mul(4);if bytes>16*1024*1024||off>size||bytes as u64>size-off{return Ok(("LC_DYSYMTAB indirect-symbol table failed bounds validation.\n".into(),"Pointer-slot resolution unavailable because the indirect-symbol table failed bounds validation.\n".into(),vec![]))}
 let raw=read_exact_range(file,off,bytes)?;let mut indices=Vec::with_capacity(count);for p in raw.chunks_exact(4){indices.push(le32(p));}
 let mut indirect=String::new();let mut pointers=String::new();let mut facts=Vec::new();
 for sec in secs{let typ=sec.flags&0xff;let is_stub=typ==0x8;let is_ptr=matches!(typ,0x6|0x7|0x10);if !is_stub&&!is_ptr{continue}let width=if is_stub{sec.reserved2.max(1) as u64}else{8};let slots=(sec.size/width).min(200_000);for j in 0..slots{let ti=sec.reserved1 as usize+j as usize;if ti>=indices.len(){break}let si=indices[ti];let symbol=if si&0x8000_0000!=0{"<LOCAL>".to_string()}else if si&0x4000_0000!=0{"<ABS>".to_string()}else{names.get(si as usize).filter(|x|!x.is_empty()).cloned().unwrap_or_else(||format!("<sym:{}>",si))};let va=sec.addr+j*width;let kind=if is_stub{"stub"}else if typ==0x7||typ==0x10{"lazy-pointer"}else{"nonlazy-pointer"};let line=format!("{va:#018x}  {},{}  {kind}[{j}] -> {symbol}\n",sec.seg,sec.sect);if indirect.len()<60_000{indirect.push_str(&line)}if is_ptr&&pointers.len()<60_000{pointers.push_str(&line)}facts.push((symbol,kind.into(),va));if facts.len()>300_000{break}}}
 if indirect.is_empty(){indirect.push_str("No supported indirect-symbol sections were recovered.\n");}if pointers.is_empty(){pointers.push_str("No supported lazy/non-lazy symbol pointer slots were recovered.\n");}Ok((indirect,pointers,facts))
}
fn read_uleb(data:&[u8],p:&mut usize)->Option<u64>{let mut v=0u64;let mut shift=0u32;for _ in 0..10{if *p>=data.len(){return None}let b=data[*p];*p+=1;v|=((b&0x7f)as u64)<<shift;if b&0x80==0{return Some(v)}shift+=7;}None}
fn read_sleb(data:&[u8],p:&mut usize)->Option<i64>{let mut v=0i64;let mut shift=0u32;for _ in 0..10{if *p>=data.len(){return None}let byte=data[*p];*p+=1;v|=((byte&0x7f)as i64)<<shift;shift+=7;if byte&0x80==0{if shift<64&&byte&0x40!=0{v|=(!0i64)<<shift;}return Some(v)}}None}
fn objc_external_class_binds(file:&mut File,window:&[u8],size:u64)->io::Result<std::collections::BTreeMap<u64,(String,String)>>{
 let mut result=std::collections::BTreeMap::new();
 let Some(ranges)=dyld_info_ranges(window)else{return Ok(result)};
 let (offset,length)=ranges[1];
 if length==0||length>16*1024*1024||offset>size||length as u64>size-offset{return Ok(result)}
 let bytes=read_exact_range(file,offset,length)?;
 let (_,facts)=decode_dyld_bind_stream(&bytes,&dyld_segments(window),"bind",false);
 let libraries=dylib_ordinal_names(window).into_iter().collect::<std::collections::BTreeMap<_,_>>();
 let mut by_address=std::collections::BTreeMap::<u64,Vec<DyldBindFact>>::new();
 for fact in facts{by_address.entry(fact.address).or_default().push(fact);}
 for (address,mut facts) in by_address{
  facts.dedup();if facts.len()!=1{continue}let fact=&facts[0];
  if fact.kind!=1||fact.weak||fact.addend!=0||fact.library<=0{continue}
  let Some(name)=fact.symbol.strip_prefix("_OBJC_CLASS_$_").filter(|name|super::objc::identifier(name))else{continue};
  let Some(library)=libraries.get(&fact.library)else{continue};
  if library.bytes().any(|byte|byte.is_ascii_whitespace()||byte.is_ascii_control()){continue}
  result.insert(address,(name.to_owned(),library.clone()));
 }
 Ok(result)
}

#[derive(Clone)]
struct DyldSegment { name:String, vmaddr:u64, vmsize:u64 }
fn dyld_segments(window:&[u8])->Vec<DyldSegment>{let mut out=Vec::new();if window.len()<32{return out}let n=le32(&window[16..20]);let mut at=32usize;for _ in 0..n{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8])as usize;if cs<8||at+cs>window.len(){break}if cmd==0x19&&cs>=72{out.push(DyldSegment{name:cstr16(&window[at+8..at+24]),vmaddr:le64(&window[at+24..at+32]),vmsize:le64(&window[at+32..at+40])});}at+=cs;}out}
fn dyld_info_ranges(window:&[u8])->Option<[(u64,usize);4]>{if window.len()<32{return None}let n=le32(&window[16..20]);let mut at=32usize;for _ in 0..n{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8])as usize;if cs<8||at+cs>window.len(){break}if (cmd==0x22||cmd==0x80000022)&&cs>=48{return Some([(le32(&window[at+8..at+12])as u64,le32(&window[at+12..at+16])as usize),(le32(&window[at+16..at+20])as u64,le32(&window[at+20..at+24])as usize),(le32(&window[at+24..at+28])as u64,le32(&window[at+28..at+32])as usize),(le32(&window[at+32..at+36])as u64,le32(&window[at+36..at+40])as usize)]);}at+=cs;}None}
fn dyld_bind_stream(data:&[u8],segments:&[DyldSegment],label:&str,weak_stream:bool)->String{
 decode_dyld_bind_stream(data,segments,label,weak_stream).0
}
#[derive(Clone,Debug,PartialEq,Eq)]
struct DyldBindFact { address:u64, symbol:String, library:i32, kind:u8, weak:bool, addend:i64 }
fn decode_dyld_bind_stream(data:&[u8],segments:&[DyldSegment],label:&str,weak_stream:bool)->(String,Vec<DyldBindFact>){
 let mut facts=Vec::new();
 let mut completed=false;
 let mut out=String::new();let mut p=0usize;let mut seg=0usize;let mut off=0u64;let mut lib=if weak_stream{-3}else{0};let mut typ=1u8;let mut sym=String::new();let mut weak=false;let mut add=0i64;let ptr=8u64;let mut binds=0usize;
 let mut emit=|out:&mut String,seg:usize,off:u64,lib:i32,typ:u8,sym:&str,weak:bool,add:i64,op:&str|{let (sn,va)=segments.get(seg).map(|s|(s.name.as_str(),s.vmaddr.saturating_add(off))).unwrap_or(("<segment?>",off));out.push_str(&format!("{label} {op} segment={seg}:{sn} offset={off:#x} address={va:#018x} type={typ} lib_ordinal={lib} weak={} addend={add} symbol={}\n",if weak{"yes"}else{"no"},if sym.is_empty(){"<unset>"}else{sym}));
  if let Some(segment)=segments.get(seg){if off.checked_add(8).is_some_and(|end|end<=segment.vmsize)&&segment.vmaddr.checked_add(off)==Some(va)&&!sym.is_empty()&&sym.len()<=1024{
   facts.push(DyldBindFact{address:va,symbol:sym.into(),library:lib,kind:typ,weak,addend:add});
  }}
 };
 while p<data.len()&&binds<200_000&&out.len()<60_000{let byte=data[p];p+=1;let op=byte&0xf0;let imm=byte&0x0f;if byte==0&&label=="bind"{completed=true;}match op{
  0x00=>{if label=="lazy-bind"{seg=0;off=0;lib=0;typ=1;sym.clear();weak=false;add=0}else{break}},0x10=>lib=imm as i32,0x20=>{let Some(v)=read_uleb(data,&mut p)else{break};lib=v.min(i32::MAX as u64)as i32},0x30=>lib=if imm==0{0}else{(0xf0|imm)as i8 as i32},0x40=>{weak=imm&1!=0;let start=p;while p<data.len()&&data[p]!=0{p+=1}if p>=data.len(){break}sym=String::from_utf8_lossy(&data[start..p]).into_owned();p+=1},0x50=>typ=imm,0x60=>{let Some(v)=read_sleb(data,&mut p)else{break};add=v},0x70=>{seg=imm as usize;let Some(v)=read_uleb(data,&mut p)else{break};off=v},0x80=>{let Some(v)=read_uleb(data,&mut p)else{break};off=off.saturating_add(v)},0x90=>{emit(&mut out,seg,off,lib,typ,&sym,weak,add,"bind");off=off.saturating_add(ptr);binds+=1},0xa0=>{emit(&mut out,seg,off,lib,typ,&sym,weak,add,"bind+uleb");let Some(v)=read_uleb(data,&mut p)else{break};off=off.saturating_add(ptr).saturating_add(v);binds+=1},0xb0=>{emit(&mut out,seg,off,lib,typ,&sym,weak,add,"bind+scaled");off=off.saturating_add(ptr.saturating_mul(imm as u64+1));binds+=1},0xc0=>{let Some(count)=read_uleb(data,&mut p)else{break};let Some(skip)=read_uleb(data,&mut p)else{break};for _ in 0..count.min(200_000-binds as u64){emit(&mut out,seg,off,lib,typ,&sym,weak,add,"bind-repeat");off=off.saturating_add(ptr).saturating_add(skip);binds+=1;if out.len()>=60_000{break}}},_=>break}}
 // Typed consumers require a completed ordinary stream. Display-only rows
 // may still explain unsupported/truncated streams, but cannot seed types.
 if !completed||data[p..].iter().any(|byte|*byte!=0){facts.clear();}
 if out.is_empty(){out.push_str(&format!("No decoded {label} operations were recovered.\n"));}(out,facts)
}
fn dyld_rebase_stream(data:&[u8],segments:&[DyldSegment])->String{let mut out=String::new();let mut p=0usize;let mut seg=0usize;let mut off=0u64;let mut typ=1u8;let ptr=8u64;let mut count=0usize;let emit=|out:&mut String,seg:usize,off:u64,typ:u8,op:&str|{let(sn,va)=segments.get(seg).map(|s|(s.name.as_str(),s.vmaddr.saturating_add(off))).unwrap_or(("<segment?>",off));out.push_str(&format!("rebase {op} segment={seg}:{sn} offset={off:#x} address={va:#018x} type={typ}\n"));};while p<data.len()&&count<200_000&&out.len()<60_000{let b=data[p];p+=1;let op=b&0xf0;let imm=b&0xf;match op{0x00=>break,0x10=>typ=imm,0x20=>{seg=imm as usize;let Some(v)=read_uleb(data,&mut p)else{break};off=v},0x30=>{let Some(v)=read_uleb(data,&mut p)else{break};off=off.saturating_add(v)},0x40=>off=off.saturating_add(imm as u64*ptr),0x50=>for _ in 0..imm{emit(&mut out,seg,off,typ,"repeat-imm");off=off.saturating_add(ptr);count+=1},0x60=>{let Some(n)=read_uleb(data,&mut p)else{break};for _ in 0..n.min(200_000-count as u64){emit(&mut out,seg,off,typ,"repeat-uleb");off=off.saturating_add(ptr);count+=1}},0x70=>{let Some(skip)=read_uleb(data,&mut p)else{break};emit(&mut out,seg,off,typ,"once+skip");off=off.saturating_add(ptr).saturating_add(skip);count+=1},0x80=>{let Some(n)=read_uleb(data,&mut p)else{break};let Some(skip)=read_uleb(data,&mut p)else{break};for _ in 0..n.min(200_000-count as u64){emit(&mut out,seg,off,typ,"repeat+skip");off=off.saturating_add(ptr).saturating_add(skip);count+=1}},_=>break}}if out.is_empty(){out.push_str("No decoded legacy dyld rebase operations were recovered.\n");}out}
fn dyld_linking_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{let Some(ranges)=dyld_info_ranges(window)else{return Ok("No LC_DYLD_INFO/LC_DYLD_INFO_ONLY opcode payload is present; this image may use chained fixups exclusively.\n".into())};let segs=dyld_segments(window);let mut out=String::new();for (i,(off,len)) in ranges.into_iter().enumerate(){let label=["rebase","bind","weak-bind","lazy-bind"][i];if len==0{continue}if len>16*1024*1024||off>size||len as u64>size-off{out.push_str(&format!("{label}: payload failed bounds validation.\n"));continue}let data=read_exact_range(file,off,len)?;if i==0{out.push_str(&dyld_rebase_stream(&data,&segs))}else{out.push_str(&dyld_bind_stream(&data,&segs,label,i==2))}}if out.is_empty(){out.push_str("LC_DYLD_INFO is present but its rebase/bind streams are empty.\n");}Ok(out)}
fn weak_load_commands(window:&[u8])->Vec<String>{let mut out=Vec::new();if window.len()<32{return out}let n=le32(&window[16..20]);let mut at=32usize;for _ in 0..n{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8])as usize;if cs<8||at+cs>window.len(){break}if matches!(cmd,0x0c|0x18|0x1f|0x20|0x23|0x80000018|0x8000001f|0x80000020|0x80000023)&&cs>=24{let no=le32(&window[at+8..at+12])as usize;if no<cs{let tail=&window[at+no..at+cs];let end=tail.iter().position(|&b|b==0).unwrap_or(tail.len());let name=String::from_utf8_lossy(&tail[..end]).into_owned();let kind=if cmd==0x80000018{"weak-load"}else if cmd==0x18{"weak-load"}else{"load"};if !name.is_empty(){out.push(format!("{kind} {name}"));}}}at+=cs;}out}
fn softlinking_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{let mut out=String::new();for l in weak_load_commands(window){if l.starts_with("weak-load ")||l.to_ascii_lowercase().contains("softlinking.framework"){out.push_str(&format!("framework {l}\n"));}}let imports=dyld_import_entries(file,window,size)?;for (_,name,add,lib,weak) in imports{let low=name.to_ascii_lowercase();if weak||low.contains("softlink")||matches!(name.as_str(),"_dlopen"|"_dlsym"|"_NSClassFromString"|"_NSProtocolFromString"|"_NSSelectorFromString"){out.push_str(&format!("chained-import symbol={name} lib_ordinal={lib} weak={} addend={add} classification={}\n",if weak{"yes"}else{"no"},if weak{"weak-import"}else{"dynamic-softlink-helper"}));if out.len()>60_000{break}}}if let Some(r)=dyld_info_ranges(window){for(i,(off,len))in r.into_iter().enumerate().skip(1){if len==0||len>16*1024*1024||off>size||len as u64>size-off{continue}let txt=dyld_bind_stream(&read_exact_range(file,off,len)?,&dyld_segments(window),["","bind","weak-bind","lazy-bind"][i],i==2);for line in txt.lines(){let low=line.to_ascii_lowercase();if line.contains("weak=yes")||i==2||low.contains("softlink")||low.contains("_dlopen")||low.contains("_dlsym")||low.contains("nsclassfromstring"){out.push_str("opcode-");out.push_str(line);out.push('\n');if out.len()>60_000{break}}}}}if out.is_empty(){out.push_str("No explicit weak-framework, weak-import, SoftLinking.framework, or common runtime soft-link helper evidence was recovered. Absence is not proof that higher-level availability checks are unused.\n");}Ok(out)}
fn export_trie_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let ncmd=if window.len()>=20{le32(&window[16..20])}else{0};let mut at=32usize;let mut payload=None;
 for _ in 0..ncmd{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8]) as usize;if cs<8||at+cs>window.len(){break}if cmd==0x80000033&&cs>=16{payload=Some((le32(&window[at+8..at+12]) as u64,le32(&window[at+12..at+16]) as usize));break}else if matches!(cmd,0x22|0x80000022)&&cs>=48{payload=Some((le32(&window[at+40..at+44]) as u64,le32(&window[at+44..at+48]) as usize));}at+=cs;}
 let Some((off,len))=payload else{return Ok("No dyld export trie load command was found.\n".into())};if len==0||len>16*1024*1024||off>size||len as u64>size-off{return Ok("Dyld export trie payload failed bounds validation.\n".into())}let data=read_exact_range(file,off,len)?;let mut out=String::new();let mut stack=vec![(0usize,String::new(),0usize)];let mut seen=std::collections::HashSet::<(usize,String)>::new();let mut count=0usize;
 while let Some((node,prefix,depth))=stack.pop(){if depth>128||node>=data.len()||!seen.insert((node,prefix.clone())){continue}let mut p=node;let Some(term_size)=read_uleb(&data,&mut p) else{continue};let term_end=p.saturating_add(term_size as usize);if term_end>data.len(){continue}if term_size>0{let mut t=p;if let Some(flags)=read_uleb(&data,&mut t){if flags&0x08!=0{let ord=read_uleb(&data,&mut t).unwrap_or(0);let start=t;while t<term_end&&data[t]!=0{t+=1}let imported=if t>start{String::from_utf8_lossy(&data[start..t]).into_owned()}else{prefix.clone()};out.push_str(&format!("export {prefix} reexport ordinal={ord} import={imported}\n"));}else if let Some(addr)=read_uleb(&data,&mut t){out.push_str(&format!("export {prefix} address={addr:#x} flags={flags:#x}"));if flags&0x10!=0{let resolver=read_uleb(&data,&mut t).unwrap_or(0);out.push_str(&format!(" resolver={resolver:#x}"));}out.push('\n');}count+=1;if out.len()>60_000||count>100_000{break}}}p=term_end;if p>=data.len(){continue}let children=data[p] as usize;p+=1;for _ in 0..children{let start=p;while p<data.len()&&data[p]!=0{p+=1}if p>=data.len(){break}let edge=String::from_utf8_lossy(&data[start..p]).into_owned();p+=1;let Some(child)=read_uleb(&data,&mut p) else{break};if (child as usize) < data.len()&&prefix.len()+edge.len()<=4096{stack.push((child as usize,format!("{prefix}{edge}"),depth+1));}}}
 if out.is_empty(){out.push_str("Export trie was present but no bounded export terminals were recovered.\n");}Ok(out)
}

fn macho_uuid(window:&[u8])->Option<String>{
 if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]{return None}let n=le32(&window[16..20]);let mut at=32usize;
 for _ in 0..n{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8])as usize;if cs<8||at+cs>window.len(){break}if cmd==0x1b&&cs>=24{let u=&window[at+8..at+24];return Some(format!("{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",u[0],u[1],u[2],u[3],u[4],u[5],u[6],u[7],u[8],u[9],u[10],u[11],u[12],u[13],u[14],u[15]));}at+=cs;}None
}
fn has_function_starts(window:&[u8])->bool{if window.len()<32{return false}let n=le32(&window[16..20]);let mut at=32usize;for _ in 0..n{if at+8>window.len(){break}let cmd=le32(&window[at..at+4]);let cs=le32(&window[at+4..at+8])as usize;if cs<8||at+cs>window.len(){break}if cmd==0x26{return true}at+=cs;}false}
fn unwind_info_function_starts(file:&mut File,window:&[u8],size:u64,code_start:u64,scan_end:u64)->io::Result<Vec<u64>>{
 let Some(sec)=sections(window).into_iter().find(|section|section.sect=="__unwind_info") else{return Ok(Vec::new())};
 if sec.size<28||sec.size>16*1024*1024||sec.offset>size||sec.size>size-sec.offset{return Ok(Vec::new())}
 let Some(image_base)=dyld_segments(window).into_iter().find(|segment|segment.name=="__TEXT").map(|segment|segment.vmaddr) else{return Ok(Vec::new())};
 let data=read_exact_range(file,sec.offset,sec.size as usize)?;
 Ok(unwind_info_starts_from_bytes(&data,image_base,code_start,scan_end))
}
fn unwind_info_starts_from_bytes(data:&[u8],image_base:u64,code_start:u64,scan_end:u64)->Vec<u64>{
 if data.len()<28{return Vec::new()}
 if le32(&data[0..4])!=1{return Vec::new()}
 let index_offset=le32(&data[20..24]) as usize;let index_count=le32(&data[24..28]) as usize;
 if index_count<2||index_count>200_000||index_offset>data.len()||index_count.checked_mul(12).map_or(true,|bytes|bytes>data.len()-index_offset){return Vec::new()}
 let mut starts=std::collections::BTreeSet::new();
 let mut add_offset=|offset:u32|{if let Some(address)=image_base.checked_add(offset as u64){if address>=code_start&&address<scan_end&&(address-code_start)%4==0{starts.insert(address);}}};
 for index in 0..index_count-1{
  let row=index_offset+index*12;let function_offset=le32(&data[row..row+4]);let page_offset=le32(&data[row+4..row+8]) as usize;
  add_offset(function_offset);
  if page_offset==0||page_offset.checked_add(8).map_or(true,|end|end>data.len()){continue}
  let page_end=page_offset.saturating_add(4096).min(data.len());
  let kind=le32(&data[page_offset..page_offset+4]);let entries_offset=u16::from_le_bytes(data[page_offset+4..page_offset+6].try_into().unwrap()) as usize;let count=u16::from_le_bytes(data[page_offset+6..page_offset+8].try_into().unwrap()) as usize;
  let stride=match kind{2=>8,3=>4,_=>continue};let Some(entries_start)=page_offset.checked_add(entries_offset) else{continue};let Some(bytes)=count.checked_mul(stride) else{continue};if entries_start>page_end||bytes>page_end-entries_start{continue}
  for item in 0..count{let at=entries_start+item*stride;let raw=le32(&data[at..at+4]);let offset=if kind==3{function_offset.checked_add(raw&0x00ff_ffff)}else{Some(raw)};if let Some(offset)=offset{add_offset(offset);}}
 }
 starts.into_iter().collect()
}
fn compact_unwind_records(file:&mut File,secs:&[SectionInfo],size:u64)->io::Result<String>{
 let Some(sec)=secs.iter().find(|s|s.sect=="__compact_unwind") else{return Ok("compact-unwind=<section absent>\n".into())};
 if sec.size==0{return Ok("compact-unwind=<empty section>\n".into())}
 if sec.offset>size||sec.size>size-sec.offset||sec.size>8*1024*1024{return Ok("compact-unwind=<section failed bounds validation>\n".into())}
 let entry_size=if sec.size%32==0{32}else if sec.size%20==0{20}else{return Ok(format!("compact-unwind=<unsupported record extent size={:#x}>\n",sec.size))};
 let data=read_exact_range(file,sec.offset,sec.size as usize)?;let mut out=format!("compact-unwind record_size={} bytes fileoff={:#x} count={}\n",entry_size,sec.offset,sec.size/(entry_size as u64));
 for (index,entry) in data.chunks_exact(entry_size).enumerate().take(200_000){
  let start=if entry_size==32{le64(&entry[0..8])}else{le32(&entry[0..4]) as u64};
  let length=le32(&entry[8..12]) as u64;
  let encoding=le32(&entry[12..16]);
  let (personality,lsda) = if entry_size==32{(le64(&entry[16..24]),le64(&entry[24..32]))}else{(le32(&entry[16..20]) as u64,0)};
  if length==0||start.checked_add(length).is_none(){continue}
  out.push_str(&format!("  record[{index}] range={start:#018x}..{:#018x} encoding={encoding:#010x} personality={personality:#018x} lsda={lsda:#018x}\n",start+length));
  if out.len()>60_000{return Ok(out)}
 }
 if data.chunks_exact(entry_size).remainder().len()!=0{out.push_str("  trailing bytes were not interpreted because they do not form a complete record.\n")}
 out.push_str("  Records are decoded from validated section extents; encoding semantics and source lines are not inferred.\n");Ok(out)
}
fn boundary_confidence(sources: Option<&std::collections::BTreeSet<&'static str>>) -> &'static str {
 if sources.is_some_and(|set|set.iter().any(|source|!matches!(*source,"direct BL target"|"section-start fallback"))) { "metadata-backed" }
 else if sources.is_some_and(|set|set.contains("direct BL target")) { "candidate" }
 else { "bounded" }
}
fn symbolication_records(file:&mut File,window:&[u8],size:u64,symbols:&[(u64,String)],starts:&[u64],boundary_sources:&std::collections::BTreeMap<u64,std::collections::BTreeSet<&'static str>>,scan_end:u64)->io::Result<String>{
 let secs=sections(window);let mut out=String::from("Static symbolication evidence\n===========================\n");let function_starts_present=has_function_starts(window);
 match macho_uuid(window){Some(uuid)=>out.push_str(&format!("image_uuid={uuid}\n")),None=>out.push_str("image_uuid=<missing LC_UUID>\n")}
 out.push_str(&format!("LC_FUNCTION_STARTS={}\ndirect_text_symbols={}\nfunction_boundaries={}\n",if function_starts_present{"present"}else{"absent"},symbols.len(),starts.len()));
 let debug=secs.iter().filter(|s|s.seg=="__DWARF"||s.sect.starts_with("__debug_")||matches!(s.sect.as_str(),"__unwind_info"|"__eh_frame"|"__compact_unwind")).collect::<Vec<_>>();
 if debug.is_empty(){out.push_str("debug_sections=<none recovered>\n");}else{for sec in debug.iter().take(256){out.push_str(&format!("debug-section {},{} addr={:#018x} size={} fileoff={:#x}\n",sec.seg,sec.sect,sec.addr,sec.size,sec.offset));}}
 out.push_str(&compact_unwind_records(file,&secs,size)?);
 let mut emitted=0usize;for (i,&start) in starts.iter().enumerate(){if start>=scan_end{continue}let end=starts.get(i+1).copied().unwrap_or(scan_end).min(scan_end);let direct=symbol_name_at(symbols,start);let name=direct.unwrap_or("<discovered>");let sources=boundary_sources.get(&start);let source=sources.map(|set|set.iter().copied().collect::<Vec<_>>().join(",")).unwrap_or_else(||"validated control-flow discovery".into());let confidence=boundary_confidence(sources);out.push_str(&format!("range start={start:#018x} end={end:#018x} size={} symbol={name} source={source} confidence={confidence}\n",end.saturating_sub(start)));emitted+=1;if emitted>=4096||out.len()>60_000{break}}
 out.push_str("\nResolution rule: an unslid address is associated only with the greatest validated function/symbol start not exceeding it and below that range's end. File/line recovery requires matching DWARF/dSYM data and is not inferred from stripped binaries.\n");Ok(out)
}

fn apple_symbolication_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let mut out=String::from("Apple Symbolication framework evidence\n====================================\n");let mut hits=0usize;
 for load in weak_load_commands(window){let low=load.to_ascii_lowercase();if low.contains("symbolication.framework")||low.contains("symbolication"){out.push_str(&format!("framework-load {load}\n"));hits+=1;}}
 for name in symbolication_symbol_names(file,window,size)?{out.push_str(&format!("symbol={name} classification=apple-symbolication-api-reference\n"));hits+=1;if out.len()>45_000{break}}
 if out.len()<45_000{let budget=size.min(4*1024*1024) as usize;let data=read_exact_range(file,0,budget)?;let text=String::from_utf8_lossy(&data);for token in text.split(|c:char|c=='\0'||c=='\n'||c=='\r'){let t=token.trim();if t.len()<5||t.len()>1024{continue}let raw=t.as_bytes();if ascii_contains_fold(raw,b"symbolication.framework")||ascii_contains_fold(raw,b"cssymbolicator")||ascii_contains_fold(raw,b"cssymbolowner"){out.push_str(&format!("string={t:?} classification=framework-or-api-name\n"));hits+=1;if out.len()>60_000{break}}}}
 if hits==0{out.push_str("No explicit Symbolication.framework load, Symbolication-named import, or bounded framework/API string reference was recovered. This does not exclude symbolication through public tooling, CoreSymbolication variants, dSYM/DWARF readers, or dynamically constructed names.\n");}
 out.push_str("Decoder scope: static framework/API evidence only. HindSight does not invoke private Symbolication/CoreSymbolication APIs or claim private ABI compatibility.\n");Ok(out)
}


fn unified_symbol_records(symbols:&[(u64,String)],starts:&[u64],scan_end:u64,facts:&[(String,String,u64)],exports:&str,imports:&str,export_names:&std::collections::BTreeMap<u64,String>)->String{
 use std::collections::{BTreeMap,BTreeSet};
 let mut direct=BTreeMap::<u64,Vec<&str>>::new();
 for (address,name) in symbols{if !name.is_empty(){direct.entry(*address).or_default().push(name.as_str());}}
 for names in direct.values_mut(){names.sort_unstable();names.dedup();}
 let mut linkage=BTreeMap::<u64,Vec<(&str,&str)>>::new();
 for (symbol,kind,address) in facts{if !symbol.starts_with('<'){linkage.entry(*address).or_default().push((symbol.as_str(),kind.as_str()));}}
 for rows in linkage.values_mut(){rows.sort_unstable();rows.dedup();}
 let function_entries=starts.iter().copied().collect::<BTreeSet<_>>();
 let mut out=String::from("Unified symbol resolver\n=======================\n");
 out.push_str("Precedence: exact nlist code symbol > validated dyld export at an admitted function entry > exact indirect-symbol stub/pointer name > validated function-start boundary. Unmapped imports/exports remain name-only evidence. No dSYM or DWARF line is invented.\n\n");
 let mut emitted=0usize;
 for (index,&start) in starts.iter().enumerate(){
  if start>=scan_end{continue}
  let end=starts.get(index+1).copied().unwrap_or(scan_end).min(scan_end);
  let (name,source,aliases)=if let Some(names)=direct.get(&start){
   (names[0].to_string(),"nlist",if names.len()>1{names[1..].join(" | ")}else{String::new()})
  }else if let Some(name)=export_names.get(&start){
   (name.clone(),"dyld-export",String::new())
  }else if let Some(rows)=linkage.get(&start){
   let preferred=rows.iter().find(|(_,kind)|*kind=="stub").copied().unwrap_or(rows[0]);
   let aliases=rows.iter().filter_map(|(symbol,_)|if *symbol!=preferred.0{Some((*symbol).to_string())}else{None}).collect::<Vec<_>>().join(" | ");
   (preferred.0.to_string(),"indirect-symbol",aliases)
  }else{
   ("<unnamed-function>".into(),"function-start",String::new())
  };
  out.push_str(&format!("range start={start:#018x} end={end:#018x} size={} symbol={name} source={source}",end.saturating_sub(start)));
  if !aliases.is_empty(){out.push_str(&format!(" aliases={aliases:?}"));}
  out.push('\n');emitted+=1;if emitted>=4096||out.len()>48_000{break}
 }
 if out.len()<56_000{
  out.push_str("\nLinkage addresses outside admitted function entries\n");
  let mut extra=0usize;
  for (address,rows) in &linkage{if function_entries.contains(address){continue}for (symbol,kind) in rows{out.push_str(&format!("linkage address={address:#018x} symbol={symbol} source=indirect-symbol kind={kind}\n"));extra+=1;if extra>=2048||out.len()>56_000{break}}if extra>=2048||out.len()>56_000{break}}
  if extra==0{out.push_str("<none>\n");}
 }
 if out.len()<59_000{
  out.push_str("\nName-only linkage evidence\n");let mut seen=BTreeSet::<String>::new();let mut count=0usize;
  for line in exports.lines().chain(imports.lines()){
   let name=if let Some(rest)=line.strip_prefix("export "){rest.split_whitespace().next()}
    else if let Some(pos)=line.find("symbol="){Some(line[pos+7..].split(|c:char|c.is_whitespace()||c==','||c==';').next().unwrap_or(""))}
    else{None};
   let Some(name)=name else{continue};if name.is_empty()||name.starts_with('<')||export_names.values().any(|mapped|mapped==name)||!seen.insert(name.to_string()){continue}
   let source=if line.starts_with("export "){"export-trie"}else{"dyld-import"};out.push_str(&format!("name-only symbol={name} source={source}\n"));count+=1;if count>=1024||out.len()>60_000{break}
  }
  if count==0{out.push_str("<none>\n");}
 }
 if emitted==0{out.push_str("No executable function ranges were admitted for address-based symbol resolution.\n");}
 out
}

fn symbol_provenance_records(facts:&[(String,String,u64)])->String{
 let mut by=std::collections::BTreeMap::<String,Vec<(String,u64)>>::new();for (sym,kind,va) in facts{if sym.starts_with('<'){continue}by.entry(sym.clone()).or_default().push((kind.clone(),*va));}let mut out=String::new();for (sym,items) in by{out.push_str(&format!("symbol {sym}\n"));for (kind,va) in items.iter().take(32){out.push_str(&format!("  {kind}: {va:#018x}\n"));}if items.len()>32{out.push_str(&format!("  … {} more sites\n",items.len()-32));}if out.len()>60_000{break}}if out.is_empty(){out.push_str("No stub/pointer symbol provenance could be reconstructed from LC_DYSYMTAB.\n");}out
}

fn joined_linkage_records(file:&mut File,window:&[u8],size:u64,facts:&[(String,String,u64)])->io::Result<String>{
 let imports=dyld_import_entries(file,window,size)?;let libs=dylib_ordinal_names(window);let exports=export_trie_records(file,window,size)?;
 let mut by=std::collections::BTreeMap::<String,Vec<(String,u64)>>::new();for (sym,kind,va) in facts{if !sym.starts_with('<'){by.entry(sym.clone()).or_default().push((kind.clone(),*va));}}for (_,name,_,_,_) in &imports{by.entry(name.clone()).or_default();}
 let imported=imports.iter().map(|entry|(entry.1.as_str(),entry)).collect::<std::collections::BTreeMap<_,_>>();let libmap=libs.iter().map(|(o,n)|(*o,n.as_str())).collect::<std::collections::BTreeMap<_,_>>();let mut exported=std::collections::BTreeSet::<&str>::new();for line in exports.lines(){if let Some(rest)=line.strip_prefix("export "){if let Some(name)=rest.split_whitespace().next(){if !name.is_empty(){exported.insert(name);}}}}
 let mut out=String::new();for (sym,mut sites) in by{sites.sort_by_key(|x|x.1);let imp=imported.get(sym.as_str()).copied();out.push_str(&format!("symbol={sym}"));if let Some((ord,_,add,lib,weak))=imp{let libname=if *lib>0{libmap.get(lib).copied().unwrap_or("<dylib-unresolved>")}else{"<special>"};out.push_str(&format!(" import[{ord}] dylib={libname} addend={add} weak={}",if *weak{"yes"}else{"no"}));}if exported.contains(sym.as_str()){out.push_str(" exported=yes");}out.push('\n');for (kind,va) in sites.iter().take(24){out.push_str(&format!("  {kind} {va:#018x}\n"));}if sites.len()>24{out.push_str(&format!("  … {} more linkage sites\n",sites.len()-24));}if out.len()>60_000{break}}if out.is_empty(){out.push_str("No joined stub/pointer/import/export linkage records were recovered.\n");}Ok(out)
}
fn symbol_caller_records(facts:&[(String,String,u64)],calls:&[(u64,u64,u64)])->String{
 let mut stubs=std::collections::BTreeMap::<u64,&str>::new();for (sym,kind,va) in facts{if kind=="stub"&&!sym.starts_with('<'){stubs.insert(*va,sym.as_str());}}let mut out=String::new();for &(func,site,target) in calls.iter().take(100_000){if let Some(sym)=stubs.get(&target){out.push_str(&format!("symbol={sym} caller={func:#018x} callsite={site:#018x} stub={target:#018x}\n"));if out.len()>60_000{break}}}if out.is_empty(){out.push_str("No direct call sites targeting recovered Mach-O stubs were found in the bounded code scan.\n");}out
}
fn got_auth_slot_records(pointers:&str)->String{
 let mut out=String::new();for line in pointers.lines(){if line.contains(",__got")||line.contains(",__auth_got")||line.contains(",__auth_ptr")||line.contains(",__nl_symbol_ptr")||line.contains(",__la_symbol_ptr"){out.push_str(line);out.push('\n');if out.len()>60_000{break}}}if out.is_empty(){out.push_str("No validated GOT/auth-GOT/lazy/non-lazy pointer slots were recovered through the indirect symbol table.\n");}out
}
fn import_export_collision_records(file:&mut File,window:&[u8],size:u64)->io::Result<String>{
 let imports=dyld_import_entries(file,window,size)?;let exports=export_trie_records(file,window,size)?;let imported=imports.iter().map(|x|x.1.clone()).collect::<std::collections::BTreeSet<_>>();let mut exported=std::collections::BTreeSet::new();for line in exports.lines(){if let Some(rest)=line.strip_prefix("export "){if let Some(name)=rest.split_whitespace().next(){if !name.is_empty(){exported.insert(name.to_string());}}}}let mut out=String::new();for name in imported.intersection(&exported){out.push_str(&format!("symbol={name} imported=yes exported=yes classification=import-export-name-collision\n"));if out.len()>60_000{break}}if out.is_empty(){out.push_str("No symbol names were simultaneously present in the decoded chained-import table and bounded export trie.\n");}Ok(out)
}

/// Structural ranking for investigation triage. The score is intentionally a
/// transparent connectivity/size heuristic, not a semantic importance or risk claim.
fn function_intelligence(starts:&[u64],leaders:&[u64],calls:&[(u64,u64,u64)],edges:&[(u64,u64,&str)],symbols:&[(u64,String)],objc_names:&std::collections::BTreeMap<u64,String>,scan_end:u64)->String{
 if starts.is_empty(){return "Function intelligence is unavailable because no executable function range was admitted.\n".into()}
 #[derive(Default)] struct Stat{incoming:usize,outgoing:usize,external_calls:usize,blocks:usize,branches:usize,self_calls:usize}
 let mut stats=std::collections::BTreeMap::<u64,Stat>::new();for &f in starts{stats.entry(f).or_default();}
 for &b in leaders{if let Some(f)=admitted_function_for(starts,b,scan_end){stats.entry(f).or_default().blocks+=1;}}
 for &(caller,_,target) in calls{let Some(caller)=admitted_function_for(starts,caller,scan_end) else{continue};let st=stats.entry(caller).or_default();st.outgoing+=1;if let Some(callee)=admitted_function_for(starts,target,scan_end){stats.entry(callee).or_default().incoming+=1;if caller==callee{stats.entry(caller).or_default().self_calls+=1;}}else{stats.entry(caller).or_default().external_calls+=1;}}
 for &(site,_,kind) in edges{if kind=="call"||kind=="fallthrough"{continue}if let Some(f)=admitted_function_for(starts,site,scan_end){stats.entry(f).or_default().branches+=1;}}
 let names=symbols.iter().map(|(address,name)|(*address,name.as_str())).collect::<std::collections::BTreeMap<_,_>>();
 let mut rows=Vec::<(usize,u64,u64,Stat)>::with_capacity(starts.len());for (i,&start) in starts.iter().enumerate(){let end=starts.get(i+1).copied().unwrap_or(scan_end);let size=end.saturating_sub(start);let st=stats.remove(&start).unwrap_or_default();let score=st.incoming.saturating_mul(5).saturating_add(st.outgoing.saturating_mul(3)).saturating_add(st.branches.saturating_mul(2)).saturating_add(st.blocks).saturating_add((size/64).min(25) as usize);rows.push((score,start,size,st));}
 rows.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));let mut out=String::from("Attention score = 5×internal incoming calls + 3×outgoing direct BL calls + 2×non-fallthrough branches + basic blocks + min(size/64,25). External BL targets are counted as outgoing evidence but never attributed to the last discovered function. This is a navigation heuristic, not a confidence, vulnerability, or semantic-importance score.\n\n");
 for (rank,(score,start,size,st)) in rows.into_iter().take(2000).enumerate(){if out.len()>60_000{break}let name=names.get(&start).copied().map(str::to_string).or_else(||objc_names.get(&start).cloned()).unwrap_or_else(||"<discovered>".into());out.push_str(&format!("rank={:<4} score={:<5} {start:#018x} size={:<7} in={:<4} out={:<4} ext={:<4} branches={:<4} blocks={:<4} self={:<3} {name}\n",rank+1,score,size,st.incoming,st.outgoing,st.external_calls,st.branches,st.blocks,st.self_calls));}
 out
}

/// Unconditional direct B edges that cross discovered function boundaries and land
/// exactly on another admitted function entry. These are tail-call candidates,
/// never promoted to proven calls because a direct branch may encode other control flow.
fn cross_function_transfer_candidates(starts:&[u64],edges:&[(u64,u64,&str)],symbols:&[(u64,String)],scan_end:u64)->(String,Vec<(u64,u64,u64)>){
 let entries=starts.iter().copied().collect::<std::collections::BTreeSet<_>>();let names=symbols.iter().map(|(a,n)|(*a,n.as_str())).collect::<std::collections::BTreeMap<_,_>>();let mut facts=Vec::new();
 for &(site,target,kind) in edges{if kind!="branch"||!entries.contains(&target){continue}let Some(caller)=admitted_function_for(starts,site,scan_end) else{continue};if caller==target{continue}facts.push((caller,site,target));if facts.len()>=20_000{break}}
 facts.sort_unstable();facts.dedup();let mut out=String::from("Classification rule: unconditional direct B from one admitted function to the exact entry of another admitted function. This is a cross-function transfer and tail-call candidate, not a proven call/return semantic claim.\n\n");
 for &(caller,site,target) in facts.iter().take(4000){if out.len()>60_000{break}let cn=names.get(&caller).copied().unwrap_or("<discovered>");let tn=names.get(&target).copied().unwrap_or("<discovered>");out.push_str(&format!("caller={caller:#018x} {cn}  branch@{site:#018x} -> entry={target:#018x} {tn}  classification=tail-call-candidate\n"));}
 if facts.is_empty(){out.push_str("No cross-function unconditional branches to exact admitted function entries were found in the bounded code scan.\n");}(out,facts)
}

fn function_neighborhood_report(starts:&[u64],calls:&[(u64,u64,u64)],tails:&[(u64,u64,u64)],symbols:&[(u64,String)],objc_names:&std::collections::BTreeMap<u64,String>,scan_end:u64)->String{
 if starts.is_empty(){return "Function neighborhoods are unavailable because no executable function range was admitted.\n".into()}
 let names=symbols.iter().map(|(a,n)|(*a,n.as_str())).collect::<std::collections::BTreeMap<_,_>>();
 // Keep only aggregate counts for the full corpus. Older releases retained a Vec
 // of every incoming/outgoing edge per function even though the report renders
 // only the top neighborhood rows; that could create substantial transient
 // memory pressure on firmware-scale call graphs.
 let mut incoming_count=std::collections::BTreeMap::<u64,usize>::new();
 let mut outgoing_count=std::collections::BTreeMap::<u64,usize>::new();
 for &(caller,_,target) in calls{
  // The first field is already the admitted caller function entry when calls
  // are recovered, so do not binary-search the function table again.
  *outgoing_count.entry(caller).or_default()+=1;
  if let Some(tf)=admitted_function_for(starts,target,scan_end){*incoming_count.entry(tf).or_default()+=1;}
 }
 for &(caller,_,target) in tails{
  *outgoing_count.entry(caller).or_default()+=1;
  *incoming_count.entry(target).or_default()+=1;
 }
 let mut scored=starts.iter().copied().map(|f|(incoming_count.get(&f).copied().unwrap_or(0)+outgoing_count.get(&f).copied().unwrap_or(0),f)).filter(|x|x.0>0).collect::<Vec<_>>();
 scored.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
 let selected=scored.into_iter().take(256).map(|(_,f)|f).collect::<Vec<_>>();
 let selected_set=selected.iter().copied().collect::<std::collections::BTreeSet<_>>();
 let mut incoming=std::collections::BTreeMap::<u64,Vec<(u64,u64,&'static str)>>::new();
 let mut outgoing=std::collections::BTreeMap::<u64,Vec<(u64,u64,&'static str)>>::new();
 // Materialize detail only for the bounded set that can actually appear in the
 // report, and cap each side at the same 12 rows the renderer emits.
 for &(caller,site,target) in calls{
  if selected_set.contains(&caller){let v=outgoing.entry(caller).or_default();if v.len()<12{v.push((site,target,"BL"));}}
  if let Some(tf)=admitted_function_for(starts,target,scan_end){if selected_set.contains(&tf){let v=incoming.entry(tf).or_default();if v.len()<12{v.push((site,caller,"BL"));}}}
 }
 for &(caller,site,target) in tails{
  if selected_set.contains(&caller){let v=outgoing.entry(caller).or_default();if v.len()<12{v.push((site,target,"B-tail-candidate"));}}
  if selected_set.contains(&target){let v=incoming.entry(target).or_default();if v.len()<12{v.push((site,caller,"B-tail-candidate"));}}
 }
 let mut out=String::from("One-hop neighborhoods contain only direct BL evidence plus separately labelled cross-function B tail-call candidates. External BL targets remain visible as outgoing edges but are not assigned to an internal function. Detail is bounded to the highest-connectivity 256 functions and at most 12 incoming/12 outgoing rows per function.\n\n");
 for f in selected{if out.len()>58_000{break}let ins=incoming.get(&f).map_or(&[][..],|v|v.as_slice());let outs=outgoing.get(&f).map_or(&[][..],|v|v.as_slice());let name=names.get(&f).copied().map(str::to_string).or_else(||objc_names.get(&f).cloned()).unwrap_or_else(||"<discovered>".into());let total_in=incoming_count.get(&f).copied().unwrap_or(0);let total_out=outgoing_count.get(&f).copied().unwrap_or(0);out.push_str(&format!("function {f:#018x} {name}  incoming={total_in} outgoing={total_out}\n"));for &(site,peer,kind) in ins{let pn=names.get(&peer).copied().unwrap_or("<discovered>");out.push_str(&format!("  <- {kind:<16} site={site:#018x} from={peer:#018x} {pn}\n"));}if total_in>ins.len(){out.push_str(&format!("  <- … {} additional incoming edges omitted\n",total_in-ins.len()));}for &(site,peer,kind) in outs{let pn=names.get(&peer).copied().or_else(||objc_names.get(&peer).map(String::as_str)).unwrap_or("<external-or-discovered>");out.push_str(&format!("  -> {kind:<16} site={site:#018x} to={peer:#018x} {pn}\n"));}if total_out>outs.len(){out.push_str(&format!("  -> … {} additional outgoing edges omitted\n",total_out-outs.len()));}out.push('\n');}
 if out.lines().count()<=2{out.push_str("No direct function neighborhood edges were recovered in the bounded code scan.\n");}out
}

fn typed_call_graph_report(calls:&[(u64,u64,u64)],tails:&[(u64,u64,u64)],symbols:&[(u64,String)],objc_names:&std::collections::BTreeMap<u64,String>,export_names:&std::collections::BTreeMap<u64,String>,stub_names:&std::collections::BTreeMap<u64,String>,starts:&[u64],scan_end:u64,indirect_facts:&[(String,String,u64)],thunks:&std::collections::BTreeSet<u64>)->String{
 let mut out=String::from("Typed call graph\n=================\nEdge types: direct (BL to a discovered function start), import (BL to a proven import stub), dispatch (BL to a proven objc_msgSend entry point; receiver and target method unresolved), thunk (BL to a trampoline/stub; target is a stub address), indirect (BLR through register; candidate set from relocation/relocation+heuristic), unresolved (BL to an address that is neither a discovered function start nor a proven import stub nor a thunk), tail (cross-function B candidate). Confidence: high/medium/low. Indirect call sites are also reported by the separate indirect-call evidence.\n\n");
 let stubs:std::collections::BTreeSet<u64>=stub_names.keys().copied().collect();
 let admitted:std::collections::BTreeSet<u64>=starts.iter().copied().collect();
 let mut direct_count=0usize;let mut import_count=0usize;let mut tail_count=0usize;let mut dispatch_count=0usize;let mut unresolved_count=0usize;let mut thunk_count=0usize;
 let mut callers_of:std::collections::BTreeMap<u64,Vec<(u64,u64,&str,f64)>>=std::collections::BTreeMap::new();
 let mut callees_of:std::collections::BTreeMap<u64,Vec<(u64,u64,&str,f64)>>=std::collections::BTreeMap::new();
 for &(func,site,target) in calls.iter().take(5000){
  let caller_name=resolved_function_name(func,symbols,objc_names,export_names,stub_names).unwrap_or("<discovered>");
  let callee_name=resolved_function_name(target,symbols,objc_names,export_names,stub_names).unwrap_or("<target>");
  // A message send is proven by the callee symbol, not by the callee address.
  let is_dispatch=symbol_name_at(symbols,target).is_some_and(|name|name.contains("objc_msgSend"));
  // A target outside the captured image is named by a relocation rather than
  // resolved to an address, so the edge is external.
  let outside_image=target<scan_end.saturating_sub(1)&&target>=scan_end;
  let edge_type=if is_dispatch{"dispatch"}else if thunks.contains(&target){"thunk"}else if stubs.contains(&target){"import"}else if admitted.contains(&target){"direct"}else if outside_image{"unresolved"}else{"unresolved"};
  let confidence=match edge_type{"direct"=>0.9,"import"=>0.95,"dispatch"=>0.6,"thunk"=>0.7,"unresolved"=>0.3,_=>0.5};
  match edge_type{"direct"=>direct_count+=1,"import"=>import_count+=1,"dispatch"=>dispatch_count+=1,"thunk"=>thunk_count+=1,"unresolved"=>unresolved_count+=1,_=>{}}
  if out.len()<58000{out.push_str(&format!("edge={edge_type:<8} confidence={confidence:.2} caller={func:#018x} {caller_name}  site={site:#018x}  callee={target:#018x} {callee_name}\n"));}
  callers_of.entry(target).or_default().push((func,site,edge_type,confidence));
  callees_of.entry(func).or_default().push((target,site,edge_type,confidence));
 }
 for &(caller,site,target) in tails.iter().take(5000){
  tail_count+=1;
  let caller_name=resolved_function_name(caller,symbols,objc_names,export_names,stub_names).unwrap_or("<discovered>");
  let callee_name=resolved_function_name(target,symbols,objc_names,export_names,stub_names).unwrap_or("<target>");
  if out.len()<58000{out.push_str(&format!("edge=tail      confidence=0.60 caller={caller:#018x} {caller_name}  site={site:#018x}  callee={target:#018x} {callee_name}  candidate=true\n"));}
  callers_of.entry(target).or_default().push((caller,site,"tail",0.6));
  callees_of.entry(caller).or_default().push((target,site,"tail",0.6));
 }
 // The candidate set for an indirect edge: the symbols the indirect-symbol
 // table proves a pointer slot may hold. A candidate is never promoted to a
 // target; the set only bounds what the edge may reach.
 let candidates:Vec<(u64,String)>=indirect_facts.iter().take(2000).map(|(name,kind,address)|(*address,format!("{name} ({kind})"))).collect();
 if !candidates.is_empty()&&out.len()<58000{
  out.push_str("\nIndirect candidate set (from the indirect-symbol table; a candidate is never a proven target):\n");
  for (address,label) in candidates.iter().take(200){
   if out.len()>=58400{break}
   out.push_str(&format!("  candidate={address:#018x} {label}\n"));
  }
 }
 if out.len()<58000{
  out.push_str(&format!("\nSummary: direct={direct_count} import={import_count} dispatch={dispatch_count} thunk={thunk_count} unresolved={unresolved_count} tail={tail_count}\n"));
   out.push_str(&format!("Total edges: {}\n",direct_count+import_count+dispatch_count+thunk_count+unresolved_count+tail_count));
 }
 if out.len()<58000{
  out.push_str("\nCaller/callee navigation (top 50 by outgoing edge count):\n");
  let mut nav:Vec<(usize,u64)>=callees_of.iter().map(|(k,v)|(v.len(),*k)).collect();
  nav.sort_by_key(|a| std::cmp::Reverse(a.0));
  for (count,addr) in nav.into_iter().take(50){
   let name=resolved_function_name(addr,symbols,objc_names,export_names,stub_names).unwrap_or("<discovered>");
   if out.len()<59500{out.push_str(&format!("  {addr:#018x} {name}: {count} outgoing edges\n"));}
  }
  out.push_str("\nTop 50 by incoming edge count:\n");
  let mut nav2:Vec<(usize,u64)>=callers_of.iter().map(|(k,v)|(v.len(),*k)).collect();
  nav2.sort_by_key(|a| std::cmp::Reverse(a.0));
  for (count,addr) in nav2.into_iter().take(50){
   let name=resolved_function_name(addr,symbols,objc_names,export_names,stub_names).unwrap_or("<target>");
   if out.len()<59900{out.push_str(&format!("  {addr:#018x} {name}: {count} incoming edges\n"));}
  }
 }
 if out.len()>=58000{out.push_str("\n... (output truncated)\n");}
 out
}
/// Recover only decoded instructions whose textual operands explicitly use
/// the stack pointer and an immediate offset. These are variable-location
/// candidates, not source variables: names, lifetimes, layouts, and source
/// types stay unresolved until matching debug information is available.
fn stack_slot_value_shape(mnemonic:&str,assembly:&str)->&'static str{
 if mnemonic.ends_with('b'){"integer-u8"}
 else if mnemonic.ends_with('h'){"integer-u16"}
 else if mnemonic=="ldrsw"{"signed-i32"}
 else if mnemonic.ends_with('p')&&assembly.contains(" w"){"pair-u32"}
 else if mnemonic.ends_with('p'){"pair-u64-or-pointer-width"}
 else if assembly.contains(" w"){"integer-u32"}
 else{"u64-or-pointer-width"}
}
fn stack_slot_candidates(ir:&str)->String{
 let mut out=String::from("Bounded stack-slot candidates grouped by signed pointer-relative offset (SP movement, source names, lifetimes, aggregate layouts, and source types remain unresolved)\n");
 // Group by (base register, signed offset) so SP-relative and frame-pointer-relative
 // accesses are kept distinct instead of being collapsed onto one shared slot.
 let mut slots=std::collections::BTreeMap::<(String,i64),(std::collections::BTreeSet<&'static str>,std::collections::BTreeSet<&'static str>,usize,Vec<String>)>::new();
 for line in ir.lines().take(20_000){
  let mut fields=line.split_whitespace();
  let Some(address)=fields.next() else{continue};
  let Some(kind)=fields.next() else{continue};
  if kind!="op"{continue}
  let assembly=fields.collect::<Vec<_>>().join(" ");
  let mnemonic=assembly.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
  if !matches!(mnemonic.as_str(),"ldr"|"ldrb"|"ldrh"|"ldrsw"|"ldp"|"str"|"strb"|"strh"|"stp")||!assembly.contains('[')||!assembly.contains('#'){continue}
  // Extract the base register and offset from `[base, #imm]` (or `[base,#imm]`).
  let Some(bracket)=assembly.split('[').nth(1) else{continue};
  let Some(close)=bracket.find(']') else{continue};
  let inner=&bracket[..close];
  let mut parts=inner.split(',');let Some(base_raw)=parts.next() else{continue};
  let base=base_raw.trim().to_ascii_lowercase();
  if !matches!(base.as_str(),"sp"|"x29"|"fp"|"x20"|"x19"){continue}
  let offset_part=parts.collect::<Vec<_>>().join(",");
  let Some(offset_text)=offset_part.split('#').nth(1).map(|tail|tail.split(|ch:char|ch==']'||ch==','||ch.is_whitespace()).next().unwrap_or("")) else{continue};
  let offset=if let Some(hex)=offset_text.strip_prefix("-0x"){i64::from_str_radix(hex,16).ok().and_then(i64::checked_neg)}else if let Some(hex)=offset_text.strip_prefix("0x"){i64::from_str_radix(hex,16).ok()}else{offset_text.parse::<i64>().ok()};let Some(offset)=offset else{continue};
  let role=if mnemonic.starts_with("ldr"){"read"}else{"write"};let shape=stack_slot_value_shape(&mnemonic,&assembly);let slot=slots.entry((base.clone(),offset)).or_default();slot.0.insert(shape);slot.1.insert(role);slot.2+=1;if slot.3.len()<8{slot.3.push(format!("{address}:{role}:{shape}"));}
 }
 if slots.is_empty(){out.push_str("No decoded pointer-relative load/store candidates were recovered.\n");}
 else{for ((base,offset),(shapes,roles,count,sites)) in slots{let base_tag=match base.as_str(){"sp"=>"sp","x29"|"fp"=>"fp",other=>other};let name=if offset<0{format!("stack_slot_{base_tag}_m{:x}",offset.unsigned_abs())}else{format!("stack_slot_{base_tag}_p{offset:x}")};out.push_str(&format!("  {name} base={base} offset={offset:+#x} accesses={count} roles={} value-shape={} examples={} classification=stack-slot-candidate provenance=decoded-stack-relative-address type=unknown\n",roles.into_iter().collect::<Vec<_>>().join(","),shapes.into_iter().collect::<Vec<_>>().join(","),sites.join(",")));if out.len()>58_000{break}}out.push_str("Names are stable labels for observed pointer-relative offsets only; a base adjustment may change the absolute slot represented by a label.\n");}
 out
}
fn analysis_quality_report(map_bytes:u64,scan_bytes:u64,encrypted:bool,truncated:bool,instructions:usize,decoded:usize,data_words:usize,functions:usize,blocks:usize,calls:usize,tails:usize,relocations:usize,data_refs:&str,dominators:&str)->String{
 let decoder_pct=if instructions==0{0.0}else{decoded as f64*100.0/instructions as f64};let scan_pct=if map_bytes==0{0.0}else{scan_bytes as f64*100.0/map_bytes as f64};let refs=data_refs.lines().filter(|l|l.contains("provenance=")).count();let skipped=dominators.lines().filter(|l|l.contains("dominator detail omitted")).count();
 let code_status=if encrypted{"metadata-only: Mach-O declares code encrypted"}else if truncated{"bounded: executable mapping exceeds scan budget"}else{"complete within admitted executable mapping"};let dom_status=if skipped==0{"complete for admitted functions under per-function budget"}else{"partially budgeted; see Dominators for exact functions"};
 format!("Analysis quality ledger\n=======================\nCode admission: {code_status}\nExecutable bytes scanned: {scan_bytes}/{map_bytes} ({scan_pct:.1}%)\nInstruction words admitted: {instructions}\nDecoder coverage: {decoded}/{instructions} ({decoder_pct:.1}%)\nData-in-code words excluded: {data_words}\nDiscovered functions: {functions}\nBasic blocks: {blocks}\nDirect BL edges: {calls}\nCross-function B tail-call candidates: {tails}\nMach-O text relocations: {relocations}\nValidated PC-relative data references: {refs}\nDominator detail: {dom_status}; budget-skipped functions={skipped}\n\nEvidence classes\n- observed/container: Mach-O ranges, relocations, section mappings, captured-byte identity\n- decoded/derived: ARM64 direct branches, CFG, bounded register/data-flow semantics\n- validated-reference: immediate-derived target additionally lands in a parsed Mach-O section\n- candidate: cross-function B/tail-call and indirect-branch classifications that are explicitly not promoted to proven call semantics\n\nGuardrail: a high decoder percentage is coverage of supported instruction decoding, not proof of semantic completeness. Bounded or omitted detail is reported and not silently treated as negative evidence.\n")
}
fn sign_extend_u64(value:u64,bits:u32)->i64{let shift=64-bits;((value<<shift) as i64)>>shift}
fn mapped_reference_label(file:&mut File,secs:&[SectionInfo],target:u64)->io::Result<Option<String>>{
 let Some(sec)=section_for_va(secs,target) else{return Ok(None)};if sec.flags&0x8000_0000!=0||sec.flags&0x0000_0400!=0{return Ok(None)};let mut label=format!("{},{}",sec.seg,sec.sect);if ["__cstring","__objc_classname","__objc_methname","__objc_methtype","__swift5_reflstr"].contains(&sec.sect.as_str()){if let Some(text)=read_cstr_at(file,sec,target,512)?{label.push_str(&format!(" string={text:?}"));}}Ok(Some(label))
}
/// Recover only architecturally explicit PC-relative references whose final
/// address validates against a parsed Mach-O section. ADRP is promoted only through an immediately following matching ADD-immediate,
/// avoiding speculative value propagation across unknown instructions or control flow.
fn pc_relative_data_references(file:&mut File,window:&[u8],code:&[u8],base:u64)->io::Result<String>{
 let secs=sections(window);let mut pending:[Option<(u64,u64,usize)>;32]=[None;32];let mut out=String::new();let mut proven=0usize;
 for (i,ch) in code.chunks_exact(4).enumerate(){let pc=base+i as u64*4;let w=le32(ch);for slot in pending.iter_mut(){if let Some((_,_,born))=*slot{if i.saturating_sub(born)>1{*slot=None;}}}
  let rd=(w&31) as usize;
  if (w&0x9f000000)==0x10000000{let imm=((w as u64>>5)&0x7ffff)<<2|((w as u64>>29)&3);let rel=sign_extend_u64(imm,21);if let Some(target)=resolve(pc,rel){if let Some(label)=mapped_reference_label(file,&secs,target)?{out.push_str(&format!("{pc:#018x} ADR  x{rd} -> {target:#018x}  {label}  provenance=instruction-immediate\n"));proven+=1;}}pending[rd]=None;}
  else if (w&0x9f000000)==0x90000000{let imm=((w as u64>>5)&0x7ffff)<<2|((w as u64>>29)&3);let rel_pages=sign_extend_u64(imm,21).saturating_mul(4096);let page=pc&!0xfffu64;if let Some(target)=resolve(page,rel_pages){pending[rd]=Some((pc,target,i));}else{pending[rd]=None;}}
  else if w>>31!=0&&(w&0x7f000000)==0x11000000{let rn=((w>>5)&31) as usize;let shift=if (w>>22)&1==0{0}else{12};let imm=(((w>>10)&0xfff) as u64)<<shift;let mut paired=false;if let Some((origin,page,born))=pending[rn]{if i==born.saturating_add(1){if let Some(target)=page.checked_add(imm){if let Some(label)=mapped_reference_label(file,&secs,target)?{out.push_str(&format!("{origin:#018x} ADRP x{rn}; {pc:#018x} ADD x{rd},x{rn} -> {target:#018x}  {label}  provenance=paired-immediates\n"));proven+=1;}pending[rd]=Some((origin,target,i));paired=true;}}}if !paired{pending[rd]=None;}}
  else if (w&0x3b000000)==0x18000000{let rel=sign_extend_u64(((w as u64>>5)&0x7ffff)<<2,21);if let Some(target)=resolve(pc,rel){if let Some(label)=mapped_reference_label(file,&secs,target)?{out.push_str(&format!("{pc:#018x} literal-reference -> {target:#018x}  {label}  provenance=literal-immediate\n"));proven+=1;}}if rd<32{pending[rd]=None;}}
  else if let Some(def)=modeled_def_reg(w){pending[def]=None;}
  if branch_target(w,pc).is_some()||is_terminal(w){pending=[None;32];}
  if out.len()>60_000{break}
 }
 if proven==0{out.push_str("No ADR, validated ADRP+ADD, or literal-load reference in the bounded code scan resolved into a parsed Mach-O section.\n");}else{out.push_str(&format!("\nValidated PC-relative data-reference records: {proven}. Unmapped targets and non-adjacent/incomplete ADRP chains are intentionally omitted.\n"));}Ok(out)
}
fn analysis_insights_report(functions:&str,data_refs:&str,quality:&str,calls:&[(u64,u64,u64)],tails:&[(u64,u64,u64)],relocations:&[(u64,String,u32,bool,u32,Option<u64>)],jump_tables:&str)->String{
 let mut out=String::from("Evidence synthesis\n==================\n");out.push_str(&format!("Direct BL edges: {}\nCross-function B tail-call candidates: {}\nMach-O text relocations: {}\nIndirect branch candidates: {}\nValidated PC-relative data references: {}\n\n",calls.len(),tails.len(),relocations.len(),jump_tables.lines().filter(|l|!l.trim().is_empty()).count(),data_refs.lines().filter(|l|l.contains("provenance=")).count()));
 out.push_str("Analysis quality\n");for line in quality.lines().skip(2).take(7){out.push_str("  ");out.push_str(line);out.push('\n');}
 out.push_str("\nHigh-connectivity functions (transparent structural heuristic)\n");for line in functions.lines().filter(|l|l.starts_with("rank=")).take(12){out.push_str("  ");out.push_str(line);out.push('\n');}
 out.push_str("\nRepresentative validated data references\n");for line in data_refs.lines().filter(|l|l.contains("provenance=")).take(12){out.push_str("  ");out.push_str(line);out.push('\n');}
 out.push_str("\nInterpretation guardrail: this view prioritizes proven structural evidence for navigation. Tail-call rows remain candidates; decoder coverage is not semantic completeness. HindSight does not infer intent, vulnerability, maliciousness, or runtime reachability.\n");out
}

struct FunctionBoundaryFacts{
 symbols:Vec<(u64,String)>,starts:Vec<u64>,lc_function_starts:Vec<u64>,
 direct_call_starts:std::collections::BTreeSet<u64>,unwind_starts:Vec<u64>,
 objc_classes:String,objc_layouts:Vec<super::objc::ClassLayout>,
 objc_function_names:std::collections::BTreeMap<u64,String>,objc_starts:Vec<u64>,
 export_trie:String,export_starts:Vec<u64>,
}

/// Shared admission path for ordinary reports and reconstruction artifacts.
fn function_boundary_facts(file:&mut File,window:&[u8],map:&Mapping,size:u64,code:&[u8],data_ranges:&[(u64,u64,u16)])->io::Result<FunctionBoundaryFacts>{
 let (symbols,mut starts,lc_function_starts)=parse_metadata(file,window,map,size)?;
 let scan_end=map.address+code.len() as u64;
 if map.encrypted{starts.clear();}else{starts.retain(|&x|x>=map.address&&x<scan_end);if starts.is_empty(){starts.push(map.address)}}
 let mut direct_call_starts=std::collections::BTreeSet::new();
 if !map.encrypted{for (index,chunk) in code.chunks_exact(4).enumerate(){let pc=map.address+(index as u64)*4;let position=map.offset+(index as u64)*4;if data_ranges.iter().any(|&(lo,hi,_)|position<hi&&position.saturating_add(4)>lo){continue}let word=u32::from_le_bytes(chunk.try_into().unwrap());if let Some((target,"call",_))=branch_target(word,pc){if target>=map.address&&target<scan_end&&(target-map.address)%4==0{direct_call_starts.insert(target);}}}}
 starts.extend(direct_call_starts.iter().copied());
 let unwind_starts=if map.encrypted{Vec::new()}else{unwind_info_function_starts(file,window,size,map.address,scan_end)?};starts.extend(unwind_starts.iter().copied());
 let (objc_classes,objc_layouts)=objc_class_graph(file,window,size)?;
 let objc_function_names=objc_imp_names(&objc_classes);
 let objc_starts=objc_function_names.keys().copied().filter(|address|*address>=map.address&&*address<scan_end&&(*address-map.address)%4==0).collect::<Vec<_>>();
 if !map.encrypted{starts.extend(objc_starts.iter().copied());}
 let export_trie=export_trie_records(file,window,size)?;
 let export_starts=if map.encrypted{Vec::new()}else{export_function_starts(&export_trie,window,map.address,scan_end)};
 starts.extend(export_starts.iter().copied());starts.sort_unstable();starts.dedup();
 Ok(FunctionBoundaryFacts{symbols,starts,lc_function_starts,direct_call_starts,unwind_starts,objc_classes,objc_layouts,objc_function_names,objc_starts,export_trie,export_starts})
}

fn reconstruction_symbols(symbols:&[(u64,String)],exports:&std::collections::BTreeMap<u64,String>,indirect:&[(String,String,u64)])->Vec<(u64,String)>{
 let mut result=symbols.to_vec();
 result.extend(exports.iter().map(|(address,name)|(*address,name.clone())));
 result.extend(indirect.iter().filter(|(_,kind,_)|kind=="stub").map(|(name,_,address)|(*address,name.clone())));
 result.sort();result.dedup();result
}

pub fn reconstruct(path:&str,digest:&str,size:&str,start:&str,end:&str,id:&str)->io::Result<String>{
 reconstruction_request(path,digest,size,start,end,id,None)
}
pub fn interpret(path:&str,digest:&str,size:&str,start:&str,end:&str,inputs:&str,id:&str)->io::Result<String>{
 if inputs.len()>151{return Err(io::Error::new(io::ErrorKind::InvalidInput,"Interpreter input budget exceeded"));}
 let values=inputs.split(',').map(|value|value.strip_prefix("0x").filter(|s|!s.is_empty()&&s.len()<=16).and_then(|s|u64::from_str_radix(s,16).ok()).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"Interpreter requires eight hexadecimal X0-X7 inputs"))).collect::<io::Result<Vec<_>>>()?;
 let registers:[u64;8]=values.try_into().map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,"Interpreter requires eight hexadecimal X0-X7 inputs"))?;
 reconstruction_request(path,digest,size,start,end,id,Some(registers))
}
fn reconstruction_request(path:&str,digest:&str,size:&str,start:&str,end:&str,id:&str,registers:Option<[u64;8]>)->io::Result<String>{
 let invalid=|message|io::Error::new(io::ErrorKind::InvalidInput,message);
 if path.len()>16384||!std::path::Path::new(path).is_absolute()||digest.len()!=64||!digest.bytes().all(|byte|byte.is_ascii_digit()||(b'a'..=b'f').contains(&byte))||id.len()!=36||!id.bytes().all(|byte|byte.is_ascii_hexdigit()||byte==b'-'){return Err(invalid("Invalid reconstruction identity"));}
 let size:u64=size.parse().map_err(|_|invalid("Invalid captured source size"))?;
 let parse_address=|value:&str|value.strip_prefix("0x").filter(|value|!value.is_empty()&&value.len()<=16).and_then(|value|u64::from_str_radix(value,16).ok()).ok_or_else(||invalid("Addresses must be bounded hexadecimal values"));
 let start=parse_address(start)?;let end=parse_address(end)?;
 let cpath=CString::new(path)?;let expected=CString::new(digest)?;
 let fd=unsafe{re_open_source(cpath.as_ptr())};if fd<0{return Err(invalid("Cannot open captured regular file without following symlinks"));}
 let mut file=unsafe{File::from_raw_fd(fd)};let before=file.metadata()?;let mut error=[0 as c_char;512];
 if unsafe{re_verify_fd(file.as_raw_fd(),expected.as_ptr(),size,error.as_mut_ptr(),error.len())}!=0{return Err(io::Error::new(io::ErrorKind::InvalidData,unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy().into_owned()));}
 let window=read_window(&mut file,size)?;
 if window.len()<32||&window[..4]!=[0xcf,0xfa,0xed,0xfe]||le32(&window[4..8])!=0x0100000c{return Err(invalid("Reconstruction currently requires a thin little-endian AArch64 Mach-O"));}
 let map=mapping(&window,size).map_err(|message|io::Error::new(io::ErrorKind::InvalidData,message))?;if map.encrypted{return Err(invalid("Encrypted code cannot be reconstructed"));}
 let code=read_exact_range(&mut file,map.offset,map.bytes.min(4*1024*1024) as usize)?;
 let data_ranges=data_in_code_ranges(&mut file,&window,size)?;
 let facts=function_boundary_facts(&mut file,&window,&map,size,&code,&data_ranges)?;
 let export_names=export_function_names(&facts.export_trie,&window,&facts.starts,map.address+code.len() as u64);
 // Use the same validated export/stub names as analysis. Undefined nlist
 // symbols alone do not identify the address of an imported call target.
 let (_,_,indirect_facts)=indirect_symbol_data(&mut file,&window,size)?;
 let signature_symbols=reconstruction_symbols(&facts.symbols,&export_names,&indirect_facts);
 let native=super::reconstruction::NativeEvidence{symbols:signature_symbols.clone(),objc_layouts:facts.objc_layouts.clone(),objc_records:facts.objc_classes.clone(),objc_function_names:facts.objc_function_names.clone(),export_names,export_starts:facts.export_starts.clone(),lc_function_starts:facts.lc_function_starts.clone(),unwind_starts:facts.unwind_starts.clone()};
 let index=facts.starts.binary_search(&start).map_err(|_|invalid("Start is not an admitted function boundary"))?;
 if index+1==facts.starts.len()&&(code.len() as u64)<map.bytes{return Err(invalid("Final function bound is outside the code scan budget"));}
 let canonical_end=facts.starts.get(index+1).copied().unwrap_or(map.address+map.bytes);
 if end!=canonical_end||end<=start{return Err(invalid("End does not match the canonical function boundary"));}
 let mut sources=Vec::new();
 if facts.lc_function_starts.contains(&start){sources.push("LC_FUNCTION_STARTS".into());}
 if facts.unwind_starts.contains(&start){sources.push("__unwind_info".into());}
 if facts.direct_call_starts.contains(&start){sources.push("direct BL target candidate".into());}
 let names=signature_symbols.iter().filter(|(address,_)|*address==start).map(|(_,name)|name.as_str()).collect::<std::collections::BTreeSet<_>>();
 if facts.symbols.iter().any(|(address,_)|*address==start){sources.push("exact nlist symbol".into());}
 if facts.objc_starts.contains(&start){sources.push("validated Objective-C IMP".into());}
 if facts.export_starts.contains(&start){sources.push("export trie".into());}
 if sources.is_empty(){sources.push("section-start fallback candidate".into());}
 let name=if names.len()==1{names.iter().next().copied()}else if names.is_empty(){facts.objc_function_names.get(&start).map(String::as_str)}else{None};
 let architecture=if le32(&window[8..12])&0x00ffffff==2{"arm64e"}else{"arm64"};
 let external=super::signatures::external_prototype_database(digest)?;
 let signatures=super::signatures::source_candidates(start,&names.iter().copied().collect::<Vec<_>>(),&facts.objc_classes,&external);
 let mut call_prototypes=Vec::new();
 for (address,symbol_name) in &signature_symbols {
  // Preserve SDK/catalog disagreements for the artifact's conflict gate;
  // lookup precedence must not silently turn a contradiction into certainty.
  for (name,signature) in super::signatures::source_candidates(*address,&[symbol_name.as_str()],"",&external) {
   call_prototypes.push((*address,name,signature));
  }
 }
 call_prototypes.sort_by(|left,right|(left.0,&left.1,&left.2.declaration).cmp(&(right.0,&right.1,&right.2.declaration)));
 call_prototypes.dedup_by(|left,right|left.0==right.0&&left.1==right.1&&left.2.declaration==right.2.declaration);
 let input=super::reconstruction::Input{code:&code,base:map.address,start,end,file_offset:map.offset,data_ranges:&data_ranges,source:digest,architecture,name,boundary_sources:sources,signatures,call_prototypes,native};
 let artifact=if let Some(registers)=registers{super::semantic::interpret_artifact(input,registers)}else{super::semantic::reconstruct_artifact(input)}?;
 if fingerprint(&before)!=fingerprint(&file.metadata()?){return Err(io::Error::new(io::ErrorKind::InvalidData,"Captured source changed during reconstruction"));}
 if artifact.len()>16*1024*1024{return Err(invalid("Reconstruction artifact output budget exceeded"));}Ok(artifact)
}

// Analysis fields are transported as individually bounded sections. Keep a
// little room below the protocol's 64 KiB ceiling for a truthful marker.
const CALL_EFFECT_SUMMARIES_LIMIT: usize = 60 * 1024;
const CALL_EFFECT_SUMMARIES_TRUNCATION: &str = "... call effect summaries truncated at the 60 KiB section budget\n";

fn append_bounded_call_effect_summary(output: &mut String, row: &str) -> bool {
    if output.len().saturating_add(row.len()) <= CALL_EFFECT_SUMMARIES_LIMIT {
        output.push_str(row);
        return true;
    }
    output.push_str(CALL_EFFECT_SUMMARIES_TRUNCATION);
    false
}

pub fn analyze(path:&str,digest:&str,size:&str,id:&str)->io::Result<String>{
 analysis_progress(2,"verify","Validating analysis identity and opening the captured artifact.");
 if path.len()>16384||!std::path::Path::new(path).is_absolute()||digest.len()!=64||!digest.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))||id.len()!=36||!id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-'){return Err(io::Error::new(io::ErrorKind::InvalidInput,"Invalid analysis identity"));}
 let size:u64=size.parse().map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,"Invalid source size"))?;let cpath=CString::new(path)?;let hash=CString::new(digest)?;let fd=unsafe{re_open_source(cpath.as_ptr())};if fd<0{return Err(io::Error::new(io::ErrorKind::InvalidInput,"Cannot open captured source"));}
 let mut file=unsafe{File::from_raw_fd(fd)};let before=file.metadata()?;let mut error=[0 as c_char;512];if unsafe{re_verify_fd(file.as_raw_fd(),hash.as_ptr(),size,error.as_mut_ptr(),error.len())}!=0{return Err(io::Error::new(io::ErrorKind::InvalidData,unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy().into_owned()));}
 analysis_progress(8,"mapping","Source hash verified; reading bounded Mach-O layout and metadata.");
 let window=read_window(&mut file,size)?;let map=mapping(&window,size).map_err(|e|io::Error::new(io::ErrorKind::InvalidData,e))?;let relocations=parse_text_relocations(&mut file,&window,size)?;let metadata=platform_metadata(&window);let runtime_records=runtime_records(&mut file,&window,size)?;let swift_types=swift_nominal_types(&mut file,&window,size)?;let runtime_graph=runtime_reference_graph(&mut file,&window,size)?;let data_ranges=data_in_code_ranges(&mut file,&window,size)?;
 analysis_progress(20,"metadata","Mach-O, symbol, runtime, relocation, and data-in-code metadata recovered.");
 let scan_bytes=if map.encrypted{0}else{map.bytes.min(4*1024*1024)};let truncated=map.encrypted||scan_bytes<map.bytes;let code=if scan_bytes==0{Vec::new()}else{read_exact_range(&mut file,map.offset,scan_bytes as usize)?};let scan_end=map.address+scan_bytes;
 let parsed_sections=sections(&window);
 let FunctionBoundaryFacts{symbols,starts,lc_function_starts,direct_call_starts,unwind_starts,objc_classes,objc_layouts,objc_function_names,objc_starts,export_trie,export_starts}=function_boundary_facts(&mut file,&window,&map,size,&code,&data_ranges)?;
 let mut boundary_sources=std::collections::BTreeMap::<u64,std::collections::BTreeSet<&'static str>>::new();
 for address in &lc_function_starts{boundary_sources.entry(*address).or_default().insert("LC_FUNCTION_STARTS");}
 for address in &unwind_starts{boundary_sources.entry(*address).or_default().insert("__unwind_info");}
 for address in &direct_call_starts{boundary_sources.entry(*address).or_default().insert("direct BL target");}
 for (address,_) in &symbols{boundary_sources.entry(*address).or_default().insert("nlist");}
 for address in objc_starts{boundary_sources.entry(address).or_default().insert("Objective-C IMP");}
 for address in export_starts{boundary_sources.entry(address).or_default().insert("export");}
 boundary_sources.retain(|address,_|starts.binary_search(address).is_ok());
 if boundary_sources.get(&map.address).map_or(true,std::collections::BTreeSet::is_empty){boundary_sources.entry(map.address).or_default().insert("section-start fallback");}
 let mut functions=String::new();
 let mut leaders=starts.clone();let mut edges:Vec<(u64,u64,&str)>=Vec::new();let mut calls:Vec<(u64,u64,u64)>=Vec::new();let mut ir=String::new();let mut dataflow=String::new();let mut ssa=String::new();let mut jump_tables=String::new();let mut decoded=0usize;let mut data_words=0usize;let mut versions=[0u32;32];let mut data_index=0usize;
 for (i,ch) in code.chunks_exact(4).enumerate(){let pc=map.address+(i as u64)*4;let file_position=map.offset+(i as u64)*4;while data_index<data_ranges.len()&&data_ranges[data_index].1<=file_position{data_index+=1}if data_index<data_ranges.len()&&file_position<data_ranges[data_index].1&&file_position.saturating_add(4)>data_ranges[data_index].0{data_words+=1;let kind=data_ranges[data_index].2;if ir.len()<60_000{ir.push_str(&format!("{pc:#018x}  data     .data-in-code kind={kind}\n"));}if dataflow.len()<60_000{dataflow.push_str(&format!("{pc:#018x}  data-in-code barrier\n"));}if ssa.len()<60_000{ssa.push_str(&format!("{pc:#018x}  barrier: LC_DATA_IN_CODE range; versions invalidated\n"));}versions=[0u32;32];continue}let w=u32::from_le_bytes(ch.try_into().unwrap());let mut out=[0 as c_char;192];let known=unsafe{re_arm64_decode(w,pc,out.as_mut_ptr(),out.len())};if known==1{decoded+=1}let asm=unsafe{CStr::from_ptr(out.as_ptr())}.to_string_lossy();let kind=if is_terminal(w){"term"}else if is_conditional(w){"cbranch"}else if branch_target(w,pc).is_some(){"branch"}else if known==1{"op"}else{"opaque"};if ir.len()<60_000{ir.push_str(&format!("{pc:#018x}  {kind:<7}  {asm}\n"));}
  if known==1{if let Some(flow)=flow_hint(w){if dataflow.len()<60_000{dataflow.push_str(&format!("{pc:#018x}  {flow}\n"));}}else if dataflow.len()<60_000{dataflow.push_str(&format!("{pc:#018x}  decoded semantics outside the conservative data-flow model\n"));}}else if dataflow.len()<60_000{dataflow.push_str(&format!("{pc:#018x}  opaque barrier\n"));}if known==1{if let Some(line)=ssa_hint(w,&mut versions){if ssa.len()<60_000{ssa.push_str(&format!("{pc:#018x}  {line}\n"));}}}else{versions=[0u32;32];if ssa.len()<60_000{ssa.push_str(&format!("{pc:#018x}  barrier: unknown semantics; versions invalidated\n"));}}if let Some(reg)=indirect_branch_reg(w){if jump_tables.len()<60_000{if let Some(entries)=jump_table_entries(&mut file,&parsed_sections,size,&code,map.address,pc,reg,scan_end)?{jump_tables.push_str(&format!("{pc:#018x}  BR x{reg}  bounded indexed-table evidence\n{entries}\n"));}else{let evidence=classify_jump_candidate(&code,map.address,pc,reg);jump_tables.push_str(&format!("{pc:#018x}  BR x{reg}  {evidence}\n"));}}}
  if let Some((target,k,split))=branch_target(w,pc){if k=="call"{calls.push((function_for(&starts,pc),pc,target));}if target>=map.address&&target<scan_end{leaders.push(target);edges.push((pc,target,k));}if split&&pc+4<scan_end{leaders.push(pc+4);edges.push((pc,pc+4,"fallthrough"));}}
  else if is_terminal(w)&&pc+4<scan_end{leaders.push(pc+4);}
 }
 let stack_slots=stack_slot_candidates(&ir);if dataflow.len().saturating_add(stack_slots.len())<60_000{dataflow.push('\n');dataflow.push_str(&stack_slots);}
 analysis_progress(44,"decode","Instruction scan completed; constructing bounded control-flow topology.");
 leaders.retain(|x|*x>=map.address&&*x<scan_end&&(*x-map.address)%4==0);leaders.sort_unstable();leaders.dedup();let block_count=leaders.len();let cfg_limit=leaders.len().min(4000);let mut cfg_edges=vec![Vec::<(u64,u64,&str)>::new();cfg_limit];for &(from,to,k) in &edges{if let Some(block)=block_index(&leaders,from){if block<cfg_limit&&cfg_edges[block].len()<16{cfg_edges[block].push((from,to,k));}}}let mut cfg=String::new();for n in 0..cfg_limit{let start=leaders[n];let end=leaders.get(n+1).copied().unwrap_or(scan_end);cfg.push_str(&format!("block B{n:04} {start:#018x}..{end:#018x}\n"));for &(from,to,k) in &cfg_edges[n]{cfg.push_str(&format!("    {k:<11} {from:#018x} -> {to:#018x}\n"));}if cfg.len()>60_000{break}}
 analysis_progress(54,"cfg","CFG leaders and direct edges recovered; solving bounded per-function structure.");
 let (dominators,phi_nodes,dominance_frontiers)=structural_analysis(&leaders,&starts,&edges,&code,map.address,scan_end);let ssa_renamed=ssa_rename_trace(&code,map.address,&leaders,scan_end);let phi_operands=phi_operand_trace(&leaders,&starts,&edges,&code,map.address,scan_end);
 analysis_progress(61,"structure","Bounded dominator, phi, and SSA structure completed.");
 let swift_contexts=swift_context_graph(&mut file,&window,size)?;let mut call_values=call_argument_provenance(&calls,&starts,&code,map.address,&symbols);let annotated_disassembly=annotated_disassembly(&code,map.address,&symbols,&relocations);let chained_fixups=chained_fixups_summary(&mut file,&window,size)?;let chained_starts=chained_page_starts(&mut file,&window,size)?;let objc_protocols=objc_protocol_property_graph(&mut file,&window,size)?;let swift_fields=swift_field_graph(&mut file,&window,size)?;let swift_field_records=swift_field_records(&mut file,&window,size)?;let chained_pointers=chained_pointer_records(&mut file,&window,size)?;let chained_multistarts=chained_multistart_records(&mut file,&window,size)?;let arm64e_pointers=arm64e_pointer_records(&mut file,&window,size)?;let swift_field_ownership=swift_field_ownership(&mut file,&window,size)?;
 analysis_progress(69,"runtime","Objective-C, Swift, chained-fixup, and annotated-code evidence completed.");
 let interblock_values=interblock_value_flow(&calls,&code,map.address,&leaders,&symbols);let predecessor_values=predecessor_value_flow(&code,map.address,&leaders,&edges,scan_end);let propagated_values=iterative_value_flow(&code,map.address,&leaders,&edges,scan_end);let indirect_calls=indirect_call_resolution(&code,map.address,&leaders,&relocations,&symbols);
 analysis_progress(75,"value-flow","Bounded inter-block value flow and indirect-call evidence completed.");
 let dyld_imports=dyld_import_records(&mut file,&window,size)?;let dyld_linking=dyld_linking_records(&mut file,&window,size)?;let softlinking=softlinking_records(&mut file,&window,size)?;let symbolication=symbolication_records(&mut file,&window,size,&symbols,&starts,&boundary_sources,scan_end)?;let apple_symbolication=apple_symbolication_records(&mut file,&window,size)?;let arm64e_bind_symbols=arm64e_bind_symbol_records(&mut file,&window,size)?;let dylib_ordinals=dylib_ordinal_records(&window);let resolved_imports=resolved_dyld_import_records(&mut file,&window,size)?;let authenticated_bind_targets=authenticated_bind_targets(&mut file,&window,size)?;let (indirect_symbols,pointer_slots,indirect_facts)=indirect_symbol_data(&mut file,&window,size)?;
 let export_names=export_function_names(&export_trie,&window,&starts,scan_end);
 let stub_names=indirect_facts.iter().filter(|(_,kind,_)|kind=="stub").map(|(name,_,address)|(*address,name.clone())).collect::<std::collections::BTreeMap<_,_>>();
 for (idx,&start) in starts.iter().take(2000).enumerate(){let end=starts.get(idx+1).copied().unwrap_or(scan_end);let name=resolved_function_name(start,&symbols,&objc_function_names,&export_names,&stub_names).unwrap_or("<discovered>");let source_set=boundary_sources.get(&start);let sources=source_set.map(|set|set.iter().copied().collect::<Vec<_>>().join(",")).unwrap_or_else(||"validated control-flow discovery".into());let confidence=boundary_confidence(source_set);let row=format!("{start:#018x}  {end:#018x}  {:>8} bytes  {name}  source={sources} confidence={confidence}\n",end.saturating_sub(start));if functions.len()+row.len()>60_000{break}functions.push_str(&row);}
 let symbol_resolver=unified_symbol_records(&symbols,&starts,scan_end,&indirect_facts,&export_trie,&dyld_imports,&export_names);let symbol_provenance=symbol_provenance_records(&indirect_facts);let linkage_records=joined_linkage_records(&mut file,&window,size,&indirect_facts)?;let symbol_callers=symbol_caller_records(&indirect_facts,&calls);let got_auth_slots=got_auth_slot_records(&pointer_slots);let import_export_collisions=import_export_collision_records(&mut file,&window,size)?;
 analysis_progress(82,"linkage","Dyld, soft-linking, symbolication, unified-symbol, indirect-symbol, and joined-linkage evidence completed.");
 let mut function_intelligence=function_intelligence(&starts,&leaders,&calls,&edges,&symbols,&objc_function_names,scan_end);let patterns=super::binary::arm64_patterns(&code,map.address,map.offset,&data_ranges);if function_intelligence.len()+patterns.len()<65536{function_intelligence.push_str(&patterns);}let data_references=pc_relative_data_references(&mut file,&window,&code,map.address)?;let (tail_call_candidates,tail_call_facts)=cross_function_transfer_candidates(&starts,&edges,&symbols,scan_end);let function_neighborhoods=function_neighborhood_report(&starts,&calls,&tail_call_facts,&symbols,&objc_function_names,scan_end);let instruction_words=(code.len()/4).saturating_sub(data_words);let analysis_quality=analysis_quality_report(map.bytes,scan_bytes,map.encrypted,truncated,instruction_words,decoded,data_words,starts.len(),block_count,calls.len(),tail_call_facts.len(),relocations.len(),&data_references,&dominators);let analysis_insights=analysis_insights_report(&function_intelligence,&data_references,&analysis_quality,&calls,&tail_call_facts,&relocations,&jump_tables);
 analysis_progress(86,"synthesis","Structural, language-runtime, linkage, value-flow, and evidence synthesis completed.");
 let mut xrefs=String::new();for &(from,to,k) in edges.iter().filter(|(_,_,k)|*k!="fallthrough").take(5000){if xrefs.len()>60_000{break}xrefs.push_str(&format!("{from:#018x}  --{k:<6}-->  {to:#018x}\n"));}
 let mut call_graph=String::new();for &(func,site,target) in calls.iter().take(5000){if call_graph.len()>60_000{break}let caller=resolved_function_name(func,&symbols,&objc_function_names,&export_names,&stub_names).unwrap_or("<discovered>");let callee=resolved_function_name(target,&symbols,&objc_function_names,&export_names,&stub_names).unwrap_or("<target>");call_graph.push_str(&format!("{func:#018x} {caller}  --BL@{site:#018x}-->  {target:#018x} {callee}\n"));}
 // A thunk is a stub that trampolines onward rather than resolving a proven
 // import. A stub whose symbol is a proven dyld import is an import edge; a
 // stub with no proven import behind it is a local trampoline, so the call
 // graph reports it as a thunk edge and never claims the eventual target.
 let import_symbols:std::collections::BTreeSet<String>=dyld_import_entries(&mut file,&window,size)?.into_iter().map(|(_,name,_,_,_)|name).collect();
 let thunks:std::collections::BTreeSet<u64>=stub_names.iter().filter(|(_,name)|!import_symbols.contains(*name)).map(|(address,_)|*address).collect();
 let typed_call_graph=typed_call_graph_report(&calls,&tail_call_facts,&symbols,&objc_function_names,&export_names,&stub_names,&starts,scan_end,&indirect_facts,&thunks);

// Build call effect summaries from calls
 let mut call_effect_summaries = String::new();
 for &(func, site, target) in calls.iter().take(5000) {
     let caller_name = resolved_function_name(func, &symbols, &objc_function_names, &export_names, &stub_names).unwrap_or("<discovered>");
     let callee_name = resolved_function_name(target, &symbols, &objc_function_names, &export_names, &stub_names).unwrap_or("<target>");
      // A stub is a proven import; any other target is a direct call to an
      // address that is not an import stub.
      let edge_type = if stub_names.contains_key(&target) { "import" } else { "direct" };
     let row = format!(
         "effect: caller={func:#018x} ({caller_name}) site={site:#018x} callee={target:#018x} ({callee_name}) type={edge_type} clobbers=gpr:{CALLER_SAVED_GPR},vector:{CALLER_SAVED_VECTOR},nzcv preserved=gpr:{CALLEE_SAVED_GPR},vector:{CALLEE_SAVED_VECTOR},{PRESERVED_SPECIAL} memory=unresolved
",
     );
     if !append_bounded_call_effect_summary(&mut call_effect_summaries, &row) {
         break;
     }
 }

 // Caller/callee constraint iteration. The caller side states what a call
 // site observed about an argument; the callee side states what a proven
 // prototype declares. The two sides are compared and any disagreement is
 // reported as a conflict with both claims kept. Neither side is preferred,
 // and the iteration is bounded.
 let mut relocation_xrefs=String::new();for &(site,ref name,rtype,pcrel,length,target) in relocations.iter().take(5000){if relocation_xrefs.len()>60_000{break}let applied=match target{Some(t) if pcrel=>format!(" resolved={t:#018x} delta={}",(t as i128)-(site as i128)),Some(t)=>format!(" resolved={t:#018x}"),None=>" unresolved-target".into()};relocation_xrefs.push_str(&format!("{site:#018x}  relocation type={rtype} pcrel={} width={} -> {name}{applied}\n",pcrel,1u32<<length));}
 let selector_xrefs=selector_call_xrefs(&relocations,&calls,&symbols);
 analysis_progress(92,"evidence","Formatting xrefs, call evidence, relocations, selectors, and bounded strings.");
 let cfstring_ranges=sections(&window).into_iter().filter(|section|section.sect=="__cfstring"&&section.offset<=size&&section.size<=size-section.offset).map(|section|(section.offset,section.size)).collect::<Vec<_>>();let strings=printable_strings(&mut file,size,&cfstring_ranges)?;let object_detections=object_detections(&mut file,&window,size,&strings,&dyld_imports)?;let mut runtime_records=runtime_records;runtime_records.push_str("\n\nC / Apple object detections\n===========================\n");runtime_records.push_str(&object_detections);
 let mut signature_symbols=symbols.clone();
 signature_symbols.extend(export_names.iter().map(|(address,name)|(*address,name.clone())));
 signature_symbols.extend(indirect_facts.iter().filter(|(_,kind,_)|kind=="stub").map(|(name,_,address)|(*address,name.clone())));
 let objc_selector_calls=objc_selector_call_evidence(&mut file,&window,size,&code,map.address,&calls,&signature_symbols)?;
 let external_prototypes=super::signatures::external_prototype_database(digest)?;
 let observed_argument_widths=observed_argument_widths(&calls,&starts,&code,map.address);
 let solved=caller_callee_constraints(&calls,&signature_symbols,&external_prototypes,&observed_argument_widths);
 let prototype_conflicts=super::semantic::artifact::constraint_report(&solved);
 let (mut signature_report,reconstructed_pseudocode)=super::signatures::recover_arm64(&starts,&signature_symbols,&objc_classes,&calls,scan_end,&code,map.address,map.offset,&data_ranges,&call_values,&tail_call_facts,&jump_tables,&objc_selector_calls,&external_prototypes,&objc_layouts);
 let mut protocol_signatures=super::signatures::protocol_signatures(&objc_protocols);let conformance_records=format!("{objc_classes}\n{objc_protocols}");let conformance_signatures=super::signatures::objc_conformance_declarations(&conformance_records);if !conformance_signatures.is_empty(){if !protocol_signatures.is_empty(){protocol_signatures.push('\n')}protocol_signatures.push_str(&conformance_signatures)}
 if !protocol_signatures.is_empty(){if signature_report.len()+protocol_signatures.len()+2<=60_000{signature_report.push_str("\n\n");signature_report.push_str(&protocol_signatures);}else if signature_report.len()+80<60_000{signature_report.push_str("\nProtocol prototypes withheld because signature output budget was exhausted.\n");}}
 call_values.push_str("\n\n");
 let mut signature_budget=60_000usize.saturating_sub(call_values.len()+1024);
 for ch in signature_report.chars(){if ch.len_utf8()>signature_budget{break}call_values.push(ch);signature_budget-=ch.len_utf8();}
 let mut semantic_preview=super::semantic::render(&code,map.address,map.offset,&starts,&data_ranges);
 const PSEUDO_MARKER:&str="\n\nReconstructed pseudocode\n========================\n";
 if semantic_preview.len()>30_000 {semantic_preview=semantic_preview.chars().take(29_000).collect();semantic_preview.push_str("\n/* instruction preview truncated for pseudocode budget */\n");}
 semantic_preview.push_str(PSEUDO_MARKER);
 let mut pseudocode_budget=60_000usize.saturating_sub(semantic_preview.len()+1024);
 for ch in reconstructed_pseudocode.chars(){if ch.len_utf8()>pseudocode_budget{break}semantic_preview.push(ch);pseudocode_budget-=ch.len_utf8();}
 let semantic_evidence=semantic_preview.lines().filter(|line|line.trim_start().starts_with("0x")).collect::<Vec<_>>().join("\n");
 let mut evidence_count=0usize;let evidence_cap=20_000usize;let mut evidence_truncated=false;
 for (source,workspace,kind,text,priority) in [
  ("Analysis Insights","Analysis Insights","Synthesis",analysis_insights.as_str(),100u16),
  ("Analysis Quality","Analysis Quality","Quality",analysis_quality.as_str(),95),
  ("Function Intelligence","Function Intelligence","Function",function_intelligence.as_str(),90),
  ("Function Neighborhoods","Function Neighborhoods","Function",function_neighborhoods.as_str(),82),
  ("Functions","Functions","Function",functions.as_str(),74),
  ("CFG","CFG","Control Flow",cfg.as_str(),76),
  ("Xrefs","Xrefs","Control Flow",xrefs.as_str(),72),
  ("Call Graph","Call Graph","Control Flow",call_graph.as_str(),78),
  ("Arguments & Types","Call Values","Semantic",call_values.as_str(),80),
  ("Semantic Preview","IR","Semantic",semantic_evidence.as_str(),80),
  ("Data References","Data References","Data",data_references.as_str(),88),
  ("Tail-Call Candidates","Tail-Call Candidates","Control Flow",tail_call_candidates.as_str(),70),
  ("Relocations","Relocations","Linkage",relocation_xrefs.as_str(),70),
  ("Indirect Calls","Indirect Calls","Control Flow",indirect_calls.as_str(),72),
  ("Jump Tables","Jump Tables","Control Flow",jump_tables.as_str(),66),
  ("Strings","Strings","Data",strings.as_str(),44),
  ("Object Detections","Object Detections","Data",object_detections.as_str(),70),
  ("Runtime Records","Runtime Records","Runtime",runtime_records.as_str(),58),
  ("Swift Types","Swift Types","Semantic",swift_types.as_str(),58),
  ("ObjC Classes","ObjC Classes","Semantic",objc_classes.as_str(),60),
  ("Dyld Imports","Dyld Imports","Linkage",dyld_imports.as_str(),62),
  ("Dyld Linking","Dyld Linking","Linkage",dyld_linking.as_str(),64),
  ("Soft Linking","Soft Linking","Linkage",softlinking.as_str(),65),
  ("Symbolication","Symbolication","Linkage",symbolication.as_str(),70),
  ("Symbol Resolver","Symbol Resolver","Linkage",symbol_resolver.as_str(),74),
  ("Apple Symbolication","Apple Symbolication","Linkage",apple_symbolication.as_str(),69),
  ("Resolved Imports","Resolved Imports","Linkage",resolved_imports.as_str(),68),
  ("Symbol Provenance","Symbol Provenance","Linkage",symbol_provenance.as_str(),68),
  ("Linkage Records","Linkage Records","Linkage",linkage_records.as_str(),66),
 ]{evidence_truncated|=emit_evidence_source(source,workspace,kind,text,priority,&mut evidence_count,evidence_cap);}
 let fp_metadata=stage_fingerprint(&[metadata.as_str(),runtime_records.as_str(),swift_types.as_str(),objc_classes.as_str()]);
 let fp_structure=stage_fingerprint(&[functions.as_str(),cfg.as_str(),xrefs.as_str(),call_graph.as_str(),jump_tables.as_str()]);
 let fp_linkage=stage_fingerprint(&[relocation_xrefs.as_str(),dyld_imports.as_str(),dyld_linking.as_str(),softlinking.as_str(),symbolication.as_str(),symbol_resolver.as_str(),apple_symbolication.as_str(),resolved_imports.as_str(),symbol_provenance.as_str(),linkage_records.as_str()]);
 let fp_synthesis=stage_fingerprint(&[function_intelligence.as_str(),function_neighborhoods.as_str(),call_values.as_str(),semantic_preview.as_str(),data_references.as_str(),analysis_quality.as_str(),analysis_insights.as_str()]);
 eprintln!("VG_EVIDENCE_MANIFEST\t{{\"schema\":1,\"record_count\":{},\"truncated\":{},\"stage_fingerprints\":{{\"metadata\":{},\"structure\":{},\"linkage\":{},\"synthesis\":{}}}}}",evidence_count,if evidence_truncated{"true"}else{"false"},super::quote(&fp_metadata),super::quote(&fp_structure),super::quote(&fp_linkage),super::quote(&fp_synthesis));
 let coverage=if instruction_words==0{0.0}else{decoded as f64*100.0/instruction_words as f64};
 let code_state=if map.encrypted{"Code bytes: declared encrypted by Mach-O metadata; instruction/CFG/SSA decode intentionally skipped. Metadata, symbols, linkage and bounded non-code evidence remain available.\n"}else{""};
 let summary=format!("ARM64 analysis v16\nCode range: {:#018x}..{:#018x}\nScanned: {} / {} code bytes\n{}Functions: {}\nSymbols in __text: {}\nBasic blocks: {}\nDirect code xrefs: {}\nDirect BL call edges: {}\nCross-function B tail-call candidates: {}\n__text relocations: {}\nIndirect branch candidates: {}\nInstructions scanned: {}\nData-in-code words skipped: {}\nDecoder coverage: {:.1}% ({}/{})\nString scan: first {} bytes of captured object\nDominators, immediate dominators, and dominance frontiers are solved per function under an explicit block budget; structural phi sites remain derived from the bounded CFG when code bytes are available; register SSA remains conservative across unsupported semantics. LC_FUNCTION_STARTS and LC_DATA_IN_CODE metadata do not veto analysis. Relocation xrefs are sourced from Mach-O relocation records. Indirect branch candidates are promoted only to stronger evidence when a nearby modeled load defines the branch register. Objective-C/Swift metadata reporting is section-grounded; string records, runtime reference edges, selector/message-send xrefs, and Swift nominal descriptor targets are emitted only when their ranges/targets validate. SSA renaming remains conservative across opaque semantics. Phi operand provenance is emitted only when modeled definitions are found on at least two predecessor paths. The unified symbol resolver ranks exact nlist names over exact indirect-symbol names and function-start-only ranges while retaining provenance; name-only imports/exports are never assigned invented addresses. Objective-C class/method and Swift context reconstruction require validated mapped records; arguments, incoming parameter candidates, and explicit integer casts report bounded instruction provenance rather than guessed source declarations.",map.address,map.address+map.bytes,scan_bytes,map.bytes,code_state,starts.len(),symbols.len(),block_count,edges.iter().filter(|(_,_,k)|*k!="fallthrough").count(),calls.len(),tail_call_facts.len(),relocations.len(),jump_tables.lines().count(),instruction_words,data_words,coverage,decoded,instruction_words,size.min(4*1024*1024));
 analysis_progress(97,"finalize","Rechecking captured-object identity and serializing the bounded analysis report.");
 if fingerprint(&before)!=fingerprint(&file.metadata()?){return Err(io::Error::new(io::ErrorKind::InvalidData,"Captured object changed during analysis"));}
 Ok(format!("{{\"protocol\":1,\"kind\":\"analysis_result\",\"request_id\":{},\"source_sha256\":{},\"function_count\":{},\"symbol_count\":{},\"basic_block_count\":{},\"analyzed_instructions\":{},\"truncated\":{},\"functions\":{},\"cfg\":{},\"ir\":{},\"strings\":{},\"xrefs\":{},\"call_graph\":{},\"typed_call_graph\":{},\"call_effect_summaries\":{},\"prototype_conflicts\":{},\"dataflow\":{},\"ssa\":{},\"dominators\":{},\"phi_nodes\":{},\"dominance_frontiers\":{},\"relocations\":{},\"jump_tables\":{},\"metadata\":{},\"runtime_records\":{},\"annotated_disassembly\":{},\"ssa_renamed\":{},\"runtime_graph\":{},\"swift_types\":{},\"selector_xrefs\":{},\"phi_operands\":{},\"objc_classes\":{},\"swift_contexts\":{},\"call_values\":{},\"chained_fixups\":{},\"chained_starts\":{},\"objc_protocols\":{},\"swift_fields\":{},\"swift_field_records\":{},\"interblock_values\":{},\"predecessor_values\":{},\"chained_pointers\":{},\"chained_multistarts\":{},\"arm64e_pointers\":{},\"swift_field_ownership\":{},\"propagated_values\":{},\"indirect_calls\":{},\"dyld_imports\":{},\"dyld_linking\":{},\"softlinking\":{},\"symbolication\":{},\"symbol_resolver\":{},\"apple_symbolication\":{},\"arm64e_bind_symbols\":{},\"dylib_ordinals\":{},\"resolved_imports\":{},\"authenticated_bind_targets\":{},\"indirect_symbols\":{},\"pointer_slots\":{},\"export_trie\":{},\"symbol_provenance\":{},\"linkage_records\":{},\"symbol_callers\":{},\"got_auth_slots\":{},\"import_export_collisions\":{},\"function_intelligence\":{},\"data_references\":{},\"tail_call_candidates\":{},\"function_neighborhoods\":{},\"analysis_quality\":{},\"analysis_insights\":{},\"semantic_preview\":{},\"summary\":{}}}",super::quote(id),super::quote(digest),starts.len(),symbols.len(),block_count,instruction_words,if truncated{"true"}else{"false"},super::quote(&functions),super::quote(&cfg),super::quote(&ir),super::quote(&strings),super::quote(&xrefs),super::quote(&call_graph),super::quote(&typed_call_graph),super::quote(&call_effect_summaries),super::quote(&prototype_conflicts),super::quote(&dataflow),super::quote(&ssa),super::quote(&dominators),super::quote(&phi_nodes),super::quote(&dominance_frontiers),super::quote(&relocation_xrefs),super::quote(&jump_tables),super::quote(&metadata),super::quote(&runtime_records),super::quote(&annotated_disassembly),super::quote(&ssa_renamed),super::quote(&runtime_graph),super::quote(&swift_types),super::quote(&selector_xrefs),super::quote(&phi_operands),super::quote(&objc_classes),super::quote(&swift_contexts),super::quote(&call_values),super::quote(&chained_fixups),super::quote(&chained_starts),super::quote(&objc_protocols),super::quote(&swift_fields),super::quote(&swift_field_records),super::quote(&interblock_values),super::quote(&predecessor_values),super::quote(&chained_pointers),super::quote(&chained_multistarts),super::quote(&arm64e_pointers),super::quote(&swift_field_ownership),super::quote(&propagated_values),super::quote(&indirect_calls),super::quote(&dyld_imports),super::quote(&dyld_linking),super::quote(&softlinking),super::quote(&symbolication),super::quote(&symbol_resolver),super::quote(&apple_symbolication),super::quote(&arm64e_bind_symbols),super::quote(&dylib_ordinals),super::quote(&resolved_imports),super::quote(&authenticated_bind_targets),super::quote(&indirect_symbols),super::quote(&pointer_slots),super::quote(&export_trie),super::quote(&symbol_provenance),super::quote(&linkage_records),super::quote(&symbol_callers),super::quote(&got_auth_slots),super::quote(&import_export_collisions),super::quote(&function_intelligence),super::quote(&data_references),super::quote(&tail_call_candidates),super::quote(&function_neighborhoods),super::quote(&analysis_quality),super::quote(&analysis_insights),super::quote(&semantic_preview),super::quote(&summary)))
}

#[cfg(test)]
mod tests {
 use super::{explicit_integer_cast,function_parameter_candidates,objc_imp_names,export_function_names,resolved_function_name,unified_symbol_records,unwind_info_starts_from_bytes};

 #[test]
 fn call_effect_summaries_stay_below_analysis_section_limit() {
  let mut summaries=String::new();
  let row="effect: caller=0x0000000000000000 callee=0x0000000000000001\n";
  while super::append_bounded_call_effect_summary(&mut summaries,row) {}
  assert!(summaries.len()<=super::CALL_EFFECT_SUMMARIES_LIMIT+super::CALL_EFFECT_SUMMARIES_TRUNCATION.len());
  assert!(summaries.len()<65_536);
  assert!(summaries.ends_with(super::CALL_EFFECT_SUMMARIES_TRUNCATION));
 }

 #[test]
 fn compressed_unwind_page_entries_are_relative_to_index_base() {
  let mut data=vec![0u8;68];
  data[0..4].copy_from_slice(&1u32.to_le_bytes());
  data[20..24].copy_from_slice(&28u32.to_le_bytes());
  data[24..28].copy_from_slice(&2u32.to_le_bytes());
  data[28..32].copy_from_slice(&0x100u32.to_le_bytes());
  data[32..36].copy_from_slice(&52u32.to_le_bytes());
  data[40..44].copy_from_slice(&0x200u32.to_le_bytes());
  data[52..56].copy_from_slice(&3u32.to_le_bytes());
  data[56..58].copy_from_slice(&12u16.to_le_bytes());
  data[58..60].copy_from_slice(&1u16.to_le_bytes());
  data[64..68].copy_from_slice(&0x0100_0020u32.to_le_bytes());
  assert_eq!(unwind_info_starts_from_bytes(&data,0x1000,0x1000,0x1300),vec![0x1100,0x1120]);
 }

 #[test]
 fn typed_dyld_bind_facts_require_completed_stream_and_segment_bounds() {
  let segments=vec![super::DyldSegment{name:"__DATA".into(),vmaddr:0x1000,vmsize:0x20}];
  let mut stream=vec![0x11,0x40];stream.extend_from_slice(b"_OBJC_CLASS_$_NSObject\0");
  let suffix=stream.len();stream.extend_from_slice(&[0x70,8,0x90,0]);
  let (_,facts)=super::decode_dyld_bind_stream(&stream,&segments,"bind",false);
  assert_eq!(facts.len(),1);assert_eq!(facts[0].address,0x1008);
  assert_eq!(facts[0].symbol,"_OBJC_CLASS_$_NSObject");assert_eq!(facts[0].library,1);
  let mut outside=stream.clone();outside[suffix+1]=0x20;
  assert!(super::decode_dyld_bind_stream(&outside,&segments,"bind",false).1.is_empty());
  let mut missing_segment=stream.clone();missing_segment[suffix]=0x7f;
  assert!(super::decode_dyld_bind_stream(&missing_segment,&segments,"bind",false).1.is_empty());
  assert!(super::decode_dyld_bind_stream(&stream[..stream.len()-1],&segments,"bind",false).1.is_empty());
  let mut operand_zero=stream[..stream.len()-1].to_vec();operand_zero.extend_from_slice(&[0x20,0]);
  assert!(super::decode_dyld_bind_stream(&operand_zero,&segments,"bind",false).1.is_empty());
  let mut unsupported=stream.clone();unsupported.insert(stream.len()-1,0xd0);
  assert!(super::decode_dyld_bind_stream(&unsupported,&segments,"bind",false).1.is_empty());
  let overflow=vec![super::DyldSegment{name:"__DATA".into(),vmaddr:u64::MAX-4,vmsize:0x20}];
  assert!(super::decode_dyld_bind_stream(&stream,&overflow,"bind",false).1.is_empty());
 }

 #[test]
 fn reconstruction_names_include_exports_and_call_stubs_not_pointer_slots() {
  let symbols=vec![(0x1000,"_local".into())];
  let exports=std::collections::BTreeMap::from([(0x1000,"_local".into()),(0x1100,"_exported".into())]);
  let indirect=vec![("_malloc".into(),"stub".into(),0x2000),("_free".into(),"pointer".into(),0x3000)];
  let joined=super::reconstruction_symbols(&symbols,&exports,&indirect);
  assert_eq!(joined,vec![(0x1000,"_local".into()),(0x1100,"_exported".into()),(0x2000,"_malloc".into())]);
  let imported=joined.iter().find(|(address,_)|*address==0x2000).unwrap();
  assert!(crate::signatures::symbol(&imported.1).is_some(),"import-stub names must reach SDK signature lookup");
 }

 #[test]
 fn function_names_join_validated_objc_and_dyld_exports() {
  let objc=objc_imp_names("  instance method 0x3000 name=run: types=v24@0:8@16 imp=0x0000000000001100 resolved=-[Widget run:] confidence=validated\n");
  assert_eq!(objc.get(&0x1100).map(String::as_str),Some("-[Widget run:]"));
  let mut window=vec![0u8;104];window[16..20].copy_from_slice(&1u32.to_le_bytes());window[32..36].copy_from_slice(&0x19u32.to_le_bytes());window[36..40].copy_from_slice(&72u32.to_le_bytes());window[40..46].copy_from_slice(b"__TEXT");window[56..64].copy_from_slice(&0x1000u64.to_le_bytes());
  let exports="export _dyldNamed address=0x104 flags=0x0\nexport _notFunction address=0x108 flags=0x0\nexport _reexport address=0x100 flags=0x8\n";
  let names=export_function_names(exports,&window,&[0x1100,0x1104],0x1200);
  assert_eq!(names.get(&0x1104).map(String::as_str),Some("_dyldNamed"));
  assert_eq!(names.len(),1);
  let symbols=vec![(0x1104,"_nlistName".into())];
  assert_eq!(resolved_function_name(0x1104,&symbols,&objc,&names,&Default::default()),Some("_nlistName"));
  let resolver=unified_symbol_records(&[],&[0x1100,0x1104],0x1200,&[],exports,"",&names);
  assert!(resolver.contains("symbol=_dyldNamed source=dyld-export"));
 }

 #[test]
 fn recognizes_exact_integer_extension_aliases() {
  assert_eq!(explicit_integer_cast(0x5300_1c20),Some((0,1,"zero-extend-u8-to-u32","unsigned-u32")));
  assert_eq!(explicit_integer_cast(0x9340_7c43),Some((3,2,"sign-extend-i32-to-i64","signed-i64")));
  assert_eq!(explicit_integer_cast(0xd340_7c20),Some((0,1,"zero-extend-u32-to-u64","unsigned-u64")));
  assert_eq!(explicit_integer_cast(0xd340_7820),None);
 }

 #[test]
 fn reports_entry_block_register_uses_as_parameter_candidates() {
  let words=[0x9100_0408u32,0xf900_03e1,0xd65f_03c0];
  let code=words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
  let report=function_parameter_candidates(&[0x1000],&code,0x1000,&[]);
  assert!(report.contains("parameter-registers=[x0, x1]"));
  assert!(report.contains("highest-abi-position=2"));
 }
}
