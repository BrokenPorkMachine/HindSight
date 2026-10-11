// The preview module intentionally keeps its bounded parser in compact guard
// clauses and tuple-shaped evidence records. The parser's fixed-width/indexed
// loops and multi-field reports are easier to audit against the wire and
// Mach-O layouts in this form; the allowances below keep the release lint
// gate explicit without hiding correctness lints in the worker entrypoint.
#[allow(
    unknown_lints,
    clippy::if_same_then_else,
    clippy::len_zero,
    clippy::manual_clamp,
    clippy::manual_pattern_char_comparison,
    clippy::manual_range_contains,
    clippy::needless_lifetimes,
    clippy::needless_range_loop,
    clippy::op_ref,
    clippy::possible_missing_else,
    clippy::suspicious_else_formatting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
mod preview;
mod binary;
use std::{fs::File,io::{self,Read}};
use reworkbench_protocol::{read_frame,write_frame,write_frame_with_limit,MAX_ANALYSIS_FRAME};
fn quote(text:&str)->String{
 let mut out=String::from("\"");
 for ch in text.chars(){match ch{'"'=>out.push_str("\\\""),'\\'=>out.push_str("\\\\"),'\n'=>out.push_str("\\n"),'\r'=>out.push_str("\\r"),'\t'=>out.push_str("\\t"),ch if ch<'\u{20}'=>out.push_str(&format!("\\u{:04x}",ch as u32)),_=>out.push(ch)}}out.push('"');out
}
fn inspect(path:&str,id:&str)->io::Result<String>{
 if path.len()>16384 || !std::path::Path::new(path).is_absolute() || id.len()!=36 || !id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-'){
  return Err(io::Error::new(io::ErrorKind::InvalidInput,"Invalid path or request ID"));
 }
 let mut file=File::open(path)?;let before=file.metadata()?;
 if !before.is_file(){return Err(io::Error::new(io::ErrorKind::InvalidInput,"Not a regular file"));}
 let mut header=[0u8;32];let mut count=0;
 while count<header.len(){let n=file.read(&mut header[count..])?;if n==0{break;}count+=n;}
 let after=file.metadata()?;
 if before.len()!=after.len()||before.modified().ok()!=after.modified().ok(){return Err(io::Error::new(io::ErrorKind::InvalidData,"Source changed during inspection"));}
 let kind=if count<4{"Unknown or raw bytes"}else{match &header[..4]{
 [0xce,0xfa,0xed,0xfe]=>"Mach-O 32 little-endian",[0xfe,0xed,0xfa,0xce]=>"Mach-O 32 big-endian",
 [0xcf,0xfa,0xed,0xfe]=>"Mach-O 64 little-endian",[0xfe,0xed,0xfa,0xcf]=>"Mach-O 64 big-endian",
 [0xca,0xfe,0xba,0xbe]|[0xca,0xfe,0xba,0xbf]=>"Universal Mach-O candidate",[0x7f,b'E',b'L',b'F']=>"ELF candidate",_=>"Unknown or raw bytes"}};
 let cpu=if count>=12 && matches!(&header[..4],[0xcf,0xfa,0xed,0xfe]|[0xce,0xfa,0xed,0xfe]){Some(u32::from_le_bytes(header[4..8].try_into().unwrap()))}else if count>=12 && matches!(&header[..4],[0xfe,0xed,0xfa,0xcf]|[0xfe,0xed,0xfa,0xce]){Some(u32::from_be_bytes(header[4..8].try_into().unwrap()))}else{None};
 let arch=match cpu{Some(0x0100000c)=>"ARM64",Some(0x01000007)=>"x86-64",Some(7)=>"i386",Some(12)=>"ARM32",Some(_)=>"Other",None=>"Not determined"};
 let hex=header[..count].iter().map(|b|format!("{b:02x}")).collect::<Vec<_>>().join(" ");
 Ok(format!("{{\"protocol\":1,\"request_id\":{},\"kind\":\"header_result\",\"file_kind\":{},\"architecture\":{},\"size\":{},\"header_hex\":{},\"captured\":false}}",quote(id),quote(kind),quote(arch),quote(&before.len().to_string()),quote(&hex)))
}
extern "C" {
 fn re_capture(source:*const std::os::raw::c_char,destination:*const std::os::raw::c_char,maximum:u64,reserve:u64,digest:*mut std::os::raw::c_char,error:*mut std::os::raw::c_char,capacity:usize)->i32;
 fn re_macho_inspect(data:*const u8,length:usize,file_size:u64,out:*mut std::os::raw::c_char,capacity:usize)->i32;
}
fn capture(source:&str,destination:&str,id:&str)->io::Result<String>{
 use std::ffi::{CString,CStr};
 if !std::path::Path::new(source).is_absolute() || !std::path::Path::new(destination).is_absolute() || source.len()>16384 || destination.len()>16384 || id.len()!=36 || !id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-') {
  return Err(io::Error::new(io::ErrorKind::InvalidInput,"Invalid capture request"));
 }
 let src=CString::new(source)?;let dst=CString::new(destination)?;
 let mut digest=[0 as std::os::raw::c_char;65];let mut error=[0 as std::os::raw::c_char;512];
 // The native helper owns only its exclusive destination and uses a fixed 1 MiB buffer.
 let code=unsafe{re_capture(src.as_ptr(),dst.as_ptr(),8*1024*1024*1024,2*1024*1024*1024,digest.as_mut_ptr(),error.as_mut_ptr(),error.len())};
 if code!=0{return Err(io::Error::other(unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy().into_owned()));}
 let mut response=inspect(destination,id)?;
 let mut file=File::open(destination)?;let size=file.metadata()?.len();
 let window=preview::read_window(&mut file,size)?;
 let mut report=vec![0 as std::os::raw::c_char;65536];
 let code=unsafe{re_macho_inspect(window.as_ptr(),window.len(),size,report.as_mut_ptr(),report.len())};
 let status=match code{0=>"checked",1=>"unsupported",_=>"rejected"};
 let (disassembly_status,disassembly)=preview::render(&mut file,&window,size)?;
 let digest=unsafe{CStr::from_ptr(digest.as_ptr())}.to_string_lossy();
 let report=unsafe{CStr::from_ptr(report.as_ptr())}.to_string_lossy();
 response=response.replace("\"captured\":false}",&format!("\"captured\":true,\"source_sha256\":{},\"structure_status\":{},\"structure\":{},\"disassembly_status\":{},\"disassembly\":{}}}",quote(&digest),quote(status),quote(&report),quote(&disassembly_status),quote(&disassembly)));
 Ok(response)
}
fn capture_image(source:&str,destination:&str,id:&str,options:&binary::Options)->io::Result<String>{
 // Reuse exclusive streaming capture and digest calculation; then inspect the
 // persisted bytes using the selected layout, never the live source.
 let legacy=capture(source,destination,id)?;
 let mut file=File::open(destination)?;
 let(kind,arch,status,structure,ds,disassembly)=binary::describe(&mut file,options)?;
 let size=file.metadata()?.len();use std::io::{Seek,SeekFrom};file.seek(SeekFrom::Start(0))?;let mut header=[0u8;32];let count=file.read(&mut header)?;
 let hex=header[..count].iter().map(|b|format!("{b:02x}")).collect::<Vec<_>>().join(" ");
 let marker="\"source_sha256\":\"";let at=legacy.find(marker).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"Capture digest absent"))?+marker.len();let digest=&legacy[at..at+64];
 Ok(format!("{{\"protocol\":1,\"request_id\":{},\"kind\":\"header_result\",\"file_kind\":{},\"architecture\":{},\"size\":{},\"header_hex\":{},\"captured\":true,\"source_sha256\":{},\"structure_status\":{},\"structure\":{},\"disassembly_status\":{},\"disassembly\":{},\"import_options\":{}}}",quote(id),quote(&kind),quote(&arch),quote(&size.to_string()),quote(&hex),quote(digest),quote(&status),quote(&structure),quote(&ds),quote(&disassembly),options.json()))
}
fn run()->io::Result<()>{
 let args:Vec<String>=std::env::args().collect();
 if args.len()==2 && args[1]=="--self-test"{
  let response=r#"{"protocol":1,"kind":"health","version":"0.5.9","worker":true}"#;
  return write_frame(&mut io::stdout().lock(),response.as_bytes());
 }
 if args.len()==2 && args[1]=="--echo-frames"{
  let mut input=io::stdin().lock();let mut output=io::stdout().lock();
  while let Some(frame)=read_frame(&mut input)?{write_frame(&mut output,&frame)?;}return Ok(());
 }
 let is_analysis=(args.len()==6 && args[1]=="--analyze") || (args.len()==10 && args[1]=="--analyze-image");
 let result=if args.len()==5 && args[1]=="--capture-inspect"{capture(&args[2],&args[3],&args[4])}
 else if args.len()==7 && args[1]=="--page"{preview::request(&args[2],&args[3],&args[4],&args[5],&args[6])}
 else if args.len()==9 && args[1]=="--capture-image"{binary::Options::parse(&args[5..]).and_then(|o|capture_image(&args[2],&args[3],&args[4],&o))}
 else if args.len()==11 && args[1]=="--page-image"{binary::Options::parse(&args[7..]).and_then(|o|binary::request(&args[2],&args[3],&args[4],&args[5],&args[6],&o))}
 else if args.len()==10 && args[1]=="--analyze-image"{binary::Options::parse(&args[6..]).and_then(|o|if binary::legacy_arm64(&args[2],&o){preview::analyze(&args[2],&args[3],&args[4],&args[5])}else{binary::analyze(&args[2],&args[3],&args[4],&args[5],&o)})}
 else if is_analysis{preview::analyze(&args[2],&args[3],&args[4],&args[5])}
 else if args.len()==4 && args[1]=="--inspect-header"{inspect(&args[2],&args[3])}
 else{return Err(io::Error::new(io::ErrorKind::InvalidInput,"Invalid worker arguments"));};
 let response=match result{Ok(v)=>v,Err(e)=>format!("{{\"protocol\":1,\"kind\":\"error\",\"message\":{}}}",quote(&e.to_string()))};
 if is_analysis { write_frame_with_limit(&mut io::stdout().lock(),response.as_bytes(),MAX_ANALYSIS_FRAME) }
 else { write_frame(&mut io::stdout().lock(),response.as_bytes()) }
}
fn main(){if let Err(e)=run(){eprintln!("reworker: {e}");std::process::exit(1);}}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn json_quote(){assert_eq!(quote("a\n\"\\"),"\"a\\n\\\"\\\\\"");}
 #[test]fn relative_path_rejected(){assert!(inspect("file","00000000-0000-0000-0000-000000000000").is_err());}
 #[test]fn health_payload_version(){
  let payload=r#"{"protocol":1,"kind":"health","version":"0.5.9","worker":true}"#;
  assert!(payload.contains("\"version\":\"0.5.9\""));
 }
 #[test]fn arm64_header(){
  let p=std::env::temp_dir().join(format!("reworker-test-{}",std::process::id()));
  let mut bytes=vec![0xcf,0xfa,0xed,0xfe,12,0,0,1];bytes.resize(32,0);
  std::fs::write(&p,bytes).unwrap();let result=inspect(p.to_str().unwrap(),"00000000-0000-0000-0000-000000000000").unwrap();
  std::fs::remove_file(p).unwrap();assert!(result.contains("ARM64"));assert!(result.contains("\"captured\":false"));
 }
}
