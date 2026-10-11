use std::{env,path::PathBuf,process::Command};
fn run(command:&mut Command){let status=command.status().expect("launch native compiler");assert!(status.success(),"native compiler failed");}
fn main(){
 let root=PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../../native/macho");
 let out=PathBuf::from(env::var("OUT_DIR").unwrap());
 let target=env::var("TARGET").unwrap();let host=env::var("HOST").unwrap();
 let apple=target.contains("apple-darwin");
 if !apple{assert_eq!(target,host,"cross compile requires an explicit supported target");}
 let apple_arch=if target.starts_with("aarch64-"){"arm64"}else if target.starts_with("x86_64-"){"x86_64"}else{""};
 if apple{assert!(!apple_arch.is_empty(),"unsupported Apple target: {target}");}
 let deployment=env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_|"14.0".into());
 let sdkroot=env::var("SDKROOT").ok();
 for source in ["inspect","capture","decode","llvm_decode"]{
  let mut compiler=Command::new(env::var("CC").unwrap_or_else(|_|"cc".into()));
  compiler.args(["-std=c17","-O2","-Wall","-Wextra","-Werror","-c"]).arg(root.join(format!("{source}.c"))).arg("-o").arg(out.join(format!("{source}.o")));
  if apple{
   compiler.arg("-arch").arg(apple_arch).arg(format!("-mmacosx-version-min={deployment}")).arg("-Wno-deprecated-declarations");
   if let Some(ref sdk)=sdkroot{compiler.arg("-isysroot").arg(sdk);}
  }
  run(&mut compiler);
  println!("cargo:rerun-if-changed={}",root.join(format!("{source}.c")).display());
  println!("cargo:rerun-if-changed={}",root.join(format!("{source}.h")).display());
 }
 run(Command::new(env::var("AR").unwrap_or_else(|_|"ar".into())).arg("crs").arg(out.join("libre_macho.a")).arg(out.join("inspect.o")).arg(out.join("capture.o")).arg(out.join("decode.o")).arg(out.join("llvm_decode.o")));
 println!("cargo:rustc-link-search=native={}",out.display());println!("cargo:rustc-link-lib=static=re_macho");
 if !apple{println!("cargo:rustc-link-lib=crypto");println!("cargo:rustc-link-lib=dl");}
 println!("cargo:rerun-if-env-changed=CC");println!("cargo:rerun-if-env-changed=AR");
 println!("cargo:rerun-if-env-changed=SDKROOT");println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
}
