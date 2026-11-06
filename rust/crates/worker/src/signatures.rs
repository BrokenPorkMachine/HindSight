//! Source signature evidence recovered from names and Objective-C runtime type encodings.
//! A declaration is emitted only when metadata or an exact API catalog entry supplies it.
use std::collections::BTreeMap;
use std::io::{self, Read};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub language: &'static str,
    pub declaration: String,
    pub source: &'static str,
}

/// Structured declaration/ABI input for source recovery. Machine analyses do
/// not parse a rendered signature report to discover names or locations.
#[derive(Clone, Debug)]
pub struct SourceParameter { pub name:String, pub spelling:String, pub bits:u8, pub signed:bool, pub pointer:bool, pub register:u8 }
#[derive(Clone, Debug)]
pub struct SourceSignature {
    pub signature:Signature, pub name:String, pub parameters:Vec<SourceParameter>,
    pub result_spelling:String, pub result_bits:u8, pub result_signed:bool, pub result_pointer:bool,
}

/// Declaration evidence has no register-location or implementation claim.
pub struct SourceDeclaration {
    pub signature: Signature, pub name: String, pub result_spelling: String,
    pub parameters: Vec<(String, String)>,
}

#[cfg(test)]
fn source_declaration(name: &str, signature: &Signature) -> Option<SourceDeclaration> {
    source_declaration_with_project_types(name, signature, &[])
}

pub fn source_declaration_with_project_types(name: &str, signature: &Signature,
    project_types: &[crate::reconstruction::ProjectTypeDependency]) -> Option<SourceDeclaration> {
    if signature.declaration.contains("...") || signature.language == "C++ / Objective-C++" { return None; }
    let recovered_name = if signature.language == "Objective-C / Objective-C++" {
        format!("hs_recovered_imp_{}", name.bytes().map(|b| if b.is_ascii_alphanumeric() { b as char } else { '_' }).collect::<String>())
    } else { name.strip_prefix('_').unwrap_or(name).to_owned() };
    if !c_identifier(&recovered_name) { return None; }
    // Reuse the body path's safe scalar spelling validation, not its argument
    // register eligibility. Plain FP declarations need no body/FP-mode proof.
    let safe_type = |ty: &str| matches!(ty.trim(), "float" | "double" | "long double" | "__int128" | "unsigned __int128" |
        "NSRange" | "CFRange" | "CGPoint" | "CGSize" | "CGRect" | "CGAffineTransform") || {
        let probe = Signature { language: "C", declaration: format!("void hs_declaration_probe({ty} value);"), source: signature.source };
        source_signature_with_project_types("hs_declaration_probe", &probe, project_types).is_some()
    };
    let result_spelling = signature_return_type(signature)?;
    if result_spelling.trim() != "void" && !safe_type(&result_spelling) { return None; }
    let mut names = std::collections::BTreeSet::new();
    let mut parameters = Vec::new();
    for parameter in abi_parameters(Some(signature)) {
        let ty = parameter.source_type?;
        if !safe_type(&ty) || !c_identifier(&parameter.name) || !names.insert(parameter.name.clone()) { return None; }
        parameters.push((ty, parameter.name));
    }
    Some(SourceDeclaration { signature: signature.clone(), name: recovered_name, result_spelling, parameters })
}

fn c_identifier(name:&str)->bool {
    let reserved=["auto","break","case","char","const","continue","default","do","double","else","enum","extern","float","for","goto","if","inline","int","long","register","restrict","return","short","signed","sizeof","static","struct","switch","typedef","union","unsigned","void","volatile","while","_Alignas","_Alignof","_Atomic","_Bool","_Complex","_Generic","_Imaginary","_Noreturn","_Static_assert","_Thread_local"];
    !name.is_empty()&&name.len()<=256&&!reserved.contains(&name)&&!["bool","true","false"].contains(&name)&&name.bytes().next().is_some_and(|b|b.is_ascii_alphabetic()||b==b'_')&&name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')
}

/// The proven parameter types of a function, in declaration order, together
/// with the general-purpose register each one travels in. A parameter with
/// no general register travels on the stack and is reported as `None`.
///
/// This is the structured form the caller/callee constraint solver consumes.
/// It never re-parses a rendered signature report.
pub fn abi_parameter_types(signature:&Signature)->Vec<(Option<String>,Option<u8>)>{
    abi_parameters(Some(signature)).into_iter().map(|parameter|{
        (parameter.source_type, parameter.general_register)
    }).collect()
}

#[cfg(test)]
pub fn source_signature(name:&str,signature:&Signature)->Option<SourceSignature>{
    source_signature_with_project_types(name,signature,&[])
}

fn catalog_pointer_type(spelling:&str, project_types:&[crate::reconstruction::ProjectTypeDependency])->bool {
    let Some(normalized)=uncomment_type(spelling) else{return false};
    if !normalized.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b' '|b'*'|b'_'))
        || normalized.matches('*').count()!=1 {return false}
    let Some((base,suffix))=normalized.split_once('*') else{return false};
    if !suffix.trim().is_empty(){return false}
    let base=base.split_whitespace().filter(|word|!matches!(*word,"const"|"volatile"|"restrict"|"__restrict"|"__restrict__"|"_Nonnull"|"_Nullable"|"__kindof"|"struct"|"union"))
        .collect::<Vec<_>>().join(" ");
    base.split_whitespace().count()==1 && project_types.iter().any(|candidate|candidate.name==base)
}

pub fn source_signature_with_project_types(name:&str,signature:&Signature,
    project_types:&[crate::reconstruction::ProjectTypeDependency])->Option<SourceSignature>{
    if signature.declaration.contains("...")||signature.language=="C++ / Objective-C++"{return None}
    let objc=signature.language=="Objective-C / Objective-C++";
    let name=if objc {format!("hs_recovered_imp_{}",name.bytes().map(|b|if b.is_ascii_alphanumeric(){b as char}else{'_'}).collect::<String>())}
        else{name.strip_prefix('_').unwrap_or(name).to_owned()};
    if !c_identifier(&name){return None}
    let scalar=|spelling:&str|{
        let spelling=uncomment_type(spelling)?;
        // These are declarations, not arbitrary C expressions. Complex
        // declarators, block pointers and unvalidated typedefs stay unresolved.
        if !spelling.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b' '|b'*')){return None}
        let bare=spelling.split(|c:char|c.is_whitespace()||c=='*').filter(|s|!s.is_empty()&&!matches!(*s,"const"|"volatile"|"restrict"|"__restrict"|"__restrict__"|"_Nonnull"|"_Nullable"|"__kindof")).collect::<Vec<_>>().join(" ");
        let native=["void","char","signed char","unsigned char","short","short int","signed short","unsigned short","unsigned short int","int","signed","signed int","unsigned","unsigned int","long","long int","unsigned long","unsigned long int","long long","long long int","unsigned long long","unsigned long long int","int8_t","uint8_t","int16_t","uint16_t","int32_t","uint32_t","int64_t","uint64_t","intptr_t","uintptr_t","size_t","ssize_t","ptrdiff_t","id","Class","SEL","NSInteger","NSUInteger","CFIndex","CFTypeID","CFOptionFlags","CFStringEncoding","CFNumberType","CFURLPathStyle","CFPropertyListFormat","dispatch_once_t","CFTypeRef","CFAllocatorRef","CFStringRef","CFArrayRef","CFDictionaryRef","CFBooleanRef","CFNumberRef","CFDataRef","CFBundleRef","CFURLRef","CFPropertyListRef","CFErrorRef","CFRunLoopRef"];
        let catalog_pointer=catalog_pointer_type(&spelling,project_types);
        if !native.contains(&bare.as_str()) && !catalog_pointer && !matches!(bare.as_str(),"bool"|"_Bool"|"signed int"|"signed short int"|"signed long"|"signed long int"|"signed long long"|"signed long long int"){return None}
        let AbiType::Integer(bytes)=abi_type(&spelling)? else{return None};
        let kind=abi_integer_kind(&spelling)?;
        if kind==AbiIntegerKind::Boolean && !matches!(bare.as_str(),"bool"|"_Bool"){return None}
        Some((spelling.trim().to_owned(),bytes*8,kind==AbiIntegerKind::Signed,kind==AbiIntegerKind::Pointer))
    };
    let result_type=signature_return_type(signature)?;
    let (result_spelling,result_bits,result_signed,result_pointer)=if result_type.trim()=="void" {
        ("void".to_owned(),0,false,false)
    }else{scalar(&result_type)?};
    let mut parameters=Vec::new();let mut names=std::collections::BTreeSet::new();
    for parameter in abi_parameters(Some(signature)){
        let (spelling,bits,signed,pointer)=scalar(parameter.source_type.as_deref()?)?;
        if !c_identifier(&parameter.name)||!names.insert(parameter.name.clone()){return None}
        parameters.push(SourceParameter{name:parameter.name,spelling,bits,signed,pointer,register:parameter.general_register?});
    }
    Some(SourceSignature{signature:signature.clone(),name,parameters,result_spelling,result_bits,result_signed,result_pointer})
}

/// All exact declaration candidates for one admitted target start. Conflicts
/// remain separate; callers must not silently choose one for source export.
pub fn source_candidates(start:u64,names:&[&str],objc_records:&str,external:&BTreeMap<String,Signature>)->Vec<(String,Signature)>{
    let mut candidates=Vec::new();
    for &name in names {
        if let Some(signature)=external.get(name).or_else(||external.get(name.strip_prefix('_').unwrap_or(name))){candidates.push((name.to_owned(),signature.clone()));}
        if let Some(signature)=symbol(name){candidates.push((name.to_owned(),signature));}
    }
    // This is the existing bounded metadata parser, not pseudocode parsing.
    for (address,(name,signature)) in objc_signature_candidates(objc_records){
        if address==start{candidates.push((name,signature));}
    }
    candidates.sort_by(|a,b|a.0.cmp(&b.0).then_with(||a.1.declaration.cmp(&b.1.declaration)));
    candidates.dedup_by(|a,b|{
        let same=a.0==b.0&&a.1.language==b.1.language&&a.1.declaration==b.1.declaration;
        if same&&a.1.source!=b.1.source{
            let sdk="hash-bound Apple SDK declaration catalog";
            let analyst="hash-bound analyst C prototype catalog";
            b.1.source=if (a.1.source==sdk&&b.1.source==analyst)||(b.1.source==sdk&&a.1.source==analyst){
                "hash-bound Apple SDK declaration catalog corroborated by hash-bound analyst C prototype catalog"
            }else if a.1.source==sdk||b.1.source==sdk{
                "hash-bound Apple SDK declaration catalog corroborated by another exact declaration source"
            }else{"exact SDK ABI catalog corroborated by hash-bound analyst C prototype catalog"};
        }
        same
    });candidates
}

fn objc_signature_candidates(records:&str)->Vec<(u64,(String,Signature))>{
    let mut result=Vec::new();
    for line in records.lines().filter(|line|line.contains(" method ")&&line.contains("confidence=validated")){
        let Some(imp)=line.split(" imp=").nth(1).and_then(|s|s.split_whitespace().next()).and_then(|s|u64::from_str_radix(s.trim_start_matches("0x"),16).ok())else{continue};
        let Some(name)=line.split(" resolved=").nth(1).and_then(|s|s.split(" confidence=").next())else{continue};
        let Some(encoding)=line.split(" types=").nth(1).and_then(|s|s.split(" imp=").next())else{continue};
        if imp!=0{if let Some(signature)=objc_method(name.trim(),encoding.trim()){result.push((imp,(name.trim().to_owned(),signature)));}}
    }result
}

fn parse_prototype_database(contents: &str, source_sha256: &str) -> Result<BTreeMap<String,Signature>, &'static str> {
    if contents.len()>12*1024*1024||contents.lines().count()>20_000{return Err("External prototype catalog exceeds byte/record budget")}
    let mut bound=false;let mut saw_declaration=false;let mut metadata=BTreeMap::<&str,&str>::new();let mut result=BTreeMap::new();
    for line in contents.lines() {
        if let Some(expected)=line.strip_prefix("# source_sha256="){
            if saw_declaration||bound||expected.len()!=64||!expected.bytes().all(|byte|byte.is_ascii_hexdigit())||!expected.eq_ignore_ascii_case(source_sha256){return Err("External prototype catalog source SHA-256 mismatch")}
            bound=true;continue
        }
        if let Some((key,value))=line.strip_prefix("# ").and_then(|value|value.split_once('=')) {
            if matches!(key,"producer"|"target"|"sdk"|"sdk_build"|"compiler"|"header_sha256")
                && (saw_declaration||value.is_empty()||value.len()>4096||metadata.insert(key,value).is_some()) {
                return Err("External prototype catalog metadata is invalid, misplaced or duplicated")
            }
            continue
        }
        let line=line.trim();if line.is_empty()||line.starts_with('#'){continue}
        if line.len()>2048{return Err("External prototype catalog record exceeds length budget")}
        let Some((symbol,declaration))=line.split_once('\t')else{return Err("External prototype records require a tab between symbol and declaration")};
        saw_declaration=true;
        let symbol=symbol.trim();let declaration=declaration.trim();
        if !clean(symbol)||!declaration.ends_with(';')||declaration.len()>1024||declaration.chars().any(|ch|ch.is_control()||matches!(ch,'\\'|'{'|'}'|'#'|'='|'"')){return Err("External prototype catalog contains an unsafe declaration")}
        let Some((head,arguments))=declaration[..declaration.len()-1].split_once('(')else{return Err("External prototype declaration is malformed")};
        if !arguments.ends_with(')')||arguments[..arguments.len()-1].contains(['(',')']){return Err("External prototype declaration has unsupported nesting")}
        let declared=head.split_whitespace().last().map(|name|name.trim_start_matches('*')).unwrap_or("");
        if declared.is_empty()||!declared.bytes().all(|byte|byte.is_ascii_alphanumeric()||byte==b'_')||!(symbol==declared||symbol.strip_prefix('_')==Some(declared)){return Err("External prototype symbol and declaration name differ")}
        if result.get(symbol).is_some_and(|old:&Signature|old.declaration!=declaration){return Err("External prototype catalog contains conflicting declarations for one symbol")}
        let source=if metadata.get("producer")==Some(&"hindsight-apple-clang-declarations-v1") {
            "hash-bound Apple SDK declaration catalog"
        }else{"hash-bound analyst C prototype catalog"};
        result.insert(symbol.to_owned(),Signature{language:"C",declaration:declaration.to_owned(),source});
    }
    if !bound{return Err("External prototype catalog requires source_sha256 binding")}
    if metadata.get("producer")==Some(&"hindsight-apple-clang-declarations-v1") {
        let target=metadata.get("target").copied().unwrap_or("");
        let sdk=metadata.get("sdk").copied().unwrap_or("");
        let sdk_build=metadata.get("sdk_build").copied().unwrap_or("");
        let compiler=metadata.get("compiler").copied().unwrap_or("");
        let header=metadata.get("header_sha256").copied().unwrap_or("");
        let token=|value:&str|!value.is_empty()&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'.'|b'_'|b'-'));
        if !valid_apple_sdk_target(target, sdk)||sdk.len()>4096||sdk.chars().any(char::is_control)
            ||!token(sdk_build)||compiler.is_empty()||compiler.len()>512||compiler.chars().any(char::is_control)
            ||header.len()!=64||!header.bytes().all(|byte|byte.is_ascii_hexdigit()) {
            return Err("Apple SDK prototype catalog lacks valid target/toolchain provenance")
        }
    }
    Ok(result)
}

fn valid_apple_sdk_target(target: &str, sdk_path: &str) -> bool {
    let Some((architecture, platform)) = target.split_once("-apple-") else { return false };
    if !matches!(architecture, "arm64" | "arm64e" | "x86_64") || platform.contains("-apple-") {
        return false;
    }
    let (platform, simulator) = platform.strip_suffix("-simulator")
        .map_or((platform, false), |value| (value, true));
    let (family, version) = if let Some(version) = platform.strip_prefix("macos") {
        if simulator { return false; }
        ("macos", version)
    } else if let Some(version) = platform.strip_prefix("ios") {
        if !simulator && architecture == "x86_64" { return false; }
        ("ios", version)
    } else {
        return false;
    };
    let parts: Vec<&str> = version.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit())) {
        return false;
    }
    let expected_sdk = match (family, simulator) {
        ("macos", false) => "MacOSX",
        ("ios", false) => "iPhoneOS",
        ("ios", true) => "iPhoneSimulator",
        _ => return false,
    };
    let Some(version) = sdk_path.rsplit('/').next().and_then(|leaf| leaf.strip_prefix(expected_sdk)).and_then(|leaf| leaf.strip_suffix(".sdk")) else { return false };
    sdk_path.starts_with('/') && (version.is_empty() || version.split('.').all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())))
}

pub fn external_prototype_database(source_sha256:&str)->io::Result<BTreeMap<String,Signature>>{
    let Some(path)=std::env::var_os("HINDSIGHT_PROTOTYPE_DB")else{return Ok(BTreeMap::new())};let path=std::path::PathBuf::from(path);
    if !path.is_absolute()||path.to_string_lossy().len()>16*1024{return Err(io::Error::new(io::ErrorKind::InvalidInput,"HINDSIGHT_PROTOTYPE_DB must be an absolute path"))}
    let mut contents=String::new();std::fs::File::open(path)?.take(12*1024*1024+1).read_to_string(&mut contents)?;
    if contents.len()>12*1024*1024{return Err(io::Error::new(io::ErrorKind::InvalidData,"External prototype catalog exceeds byte budget"))}
    parse_prototype_database(&contents,source_sha256).map_err(|message|io::Error::new(io::ErrorKind::InvalidData,message))
}

/// Read project type dependencies embedded in the same source-bound request
/// catalog as external prototypes. These declarations are presentation/source
/// dependencies only; no ABI layout is inferred from their spelling.
pub fn external_project_type_dependencies(source_sha256: &str, architecture: &str) -> io::Result<Vec<crate::reconstruction::ProjectTypeDependency>> {
    let Some(path)=std::env::var_os("HINDSIGHT_PROTOTYPE_DB") else { return Ok(Vec::new()) };
    let path=std::path::PathBuf::from(path);
    if !path.is_absolute()||path.to_string_lossy().len()>16*1024 { return Err(io::Error::new(io::ErrorKind::InvalidInput,"HINDSIGHT_PROTOTYPE_DB must be an absolute path")) }
    let mut contents=String::new();
    std::fs::File::open(path)?.take(12*1024*1024+1).read_to_string(&mut contents)?;
    if contents.len()>12*1024*1024 { return Err(io::Error::new(io::ErrorKind::InvalidData,"Project type catalog exceeds byte budget")) }
    parse_project_type_dependencies(&contents, source_sha256, architecture)
}

#[derive(Debug)]
enum LayoutJSONValue { String(String), Number(i64), Bool(bool), Null, Array(Vec<LayoutJSONValue>), Object(BTreeMap<String,LayoutJSONValue>) }
struct LayoutJSON<'a> { bytes: &'a [u8], at: usize }
impl LayoutJSON<'_> {
    fn whitespace(&mut self) { while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) { self.at += 1; } }
    fn take(&mut self, byte: u8) -> bool { self.whitespace(); if self.bytes.get(self.at)==Some(&byte) { self.at+=1; true } else { false } }
    fn string(&mut self) -> Option<String> {
        self.whitespace(); if !self.take_raw(b'"') { return None; }
        let mut out=Vec::new();
        loop {
            let byte=*self.bytes.get(self.at)?; self.at+=1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escaped=*self.bytes.get(self.at)?; self.at+=1;
                    match escaped {
                        b'"'|b'\\'|b'/' => out.push(escaped), b'b'=>out.push(8),b'f'=>out.push(12),b'n'=>out.push(10),b'r'=>out.push(13),b't'=>out.push(9),
                        b'u' => {
                            let first=self.hex4()?;
                            let scalar=if (0xd800..=0xdbff).contains(&first) {
                                if self.bytes.get(self.at..self.at+2)!=Some(b"\\u") { return None; }
                                self.at+=2; let second=self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&second) { return None; }
                                0x10000+((u32::from(first)-0xd800)<<10)+(u32::from(second)-0xdc00)
                            } else if (0xdc00..=0xdfff).contains(&first) { return None; } else { u32::from(first) };
                            out.extend(char::from_u32(scalar)?.encode_utf8(&mut [0;4]).as_bytes());
                        }
                        _=>return None,
                    }
                }
                0..=31 => return None,
                _=>out.push(byte),
            }
        }
    }
    fn take_raw(&mut self, byte:u8)->bool { if self.bytes.get(self.at)==Some(&byte) {self.at+=1;true}else{false} }
    fn hex4(&mut self)->Option<u16>{
        let chunk=self.bytes.get(self.at..self.at+4)?; self.at+=4;
        chunk.iter().try_fold(0u16,|value,byte|Some(value*16+match byte{b'0'..=b'9'=>u16::from(byte-b'0'),b'a'..=b'f'=>u16::from(byte-b'a'+10),b'A'..=b'F'=>u16::from(byte-b'A'+10),_=>return None}))
    }
    fn value(&mut self, depth:u8)->Option<LayoutJSONValue> {
        if depth>32{return None} self.whitespace();
        if self.bytes.get(self.at)==Some(&b'"'){return self.string().map(LayoutJSONValue::String)}
        if self.take(b'{') {
            let mut object=BTreeMap::new(); if self.take(b'}'){return Some(LayoutJSONValue::Object(object))}
            loop { let key=self.string()?; if !self.take(b':'){return None} let value=self.value(depth+1)?;
                if object.insert(key,value).is_some(){return None}
                if self.take(b'}'){return Some(LayoutJSONValue::Object(object))} if !self.take(b','){return None} }
        }
        if self.take(b'[') {
            let mut array=Vec::new(); if self.take(b']'){return Some(LayoutJSONValue::Array(array))}
            loop { array.push(self.value(depth+1)?); if self.take(b']'){return Some(LayoutJSONValue::Array(array))} if !self.take(b','){return None} }
        }
        for (literal,value) in [(&b"true"[..],LayoutJSONValue::Bool(true)),(&b"false"[..],LayoutJSONValue::Bool(false)),(&b"null"[..],LayoutJSONValue::Null)] {
            if self.bytes.get(self.at..self.at+literal.len())==Some(literal){self.at+=literal.len();return Some(value)}
        }
        let start=self.at;
        if self.bytes.get(self.at)==Some(&b'-'){self.at+=1}
        if self.bytes.get(self.at)==Some(&b'0'){self.at+=1}else{
            let digits=self.at; while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){self.at+=1} if self.at==digits{return None}
        }
        if self.bytes.get(self.at).is_some_and(|byte|matches!(byte,b'.'|b'e'|b'E')){return None}
        std::str::from_utf8(&self.bytes[start..self.at]).ok()?.parse::<i64>().ok().map(LayoutJSONValue::Number)
    }
    fn document(mut self)->Option<LayoutJSONValue> {
        let value=self.value(0)?;self.whitespace();(self.at==self.bytes.len()).then_some(value)
    }
}

fn layout_document(json:&str)->Option<LayoutJSONValue> { LayoutJSON{bytes:json.as_bytes(),at:0}.document() }
fn layout_bindings(json:&str)->Option<BTreeMap<String,String>> {
    match layout_document(json)? { LayoutJSONValue::Object(object)=>Some(object.into_iter().filter_map(|(key,value)| {
        matches!(key.as_str(),"producer"|"target"|"declarationsSHA256").then(||match value {LayoutJSONValue::String(value)=>Some((key,value)),_=>None}).flatten()
    }).collect()),_=>None }
}

fn parse_layout_record(json:&str)->Option<crate::reconstruction::ProjectTypeLayout> {
    let LayoutJSONValue::Object(root)=layout_document(json)? else{return None};
    let LayoutJSONValue::Object(record)=root.get("record")? else{return None};
    let text=|key:&str|match record.get(key)? {LayoutJSONValue::String(value)=>Some(value.clone()),_=>None};
    let number=|key:&str|match record.get(key)? {LayoutJSONValue::Number(value) if *value>0=>Some(*value as u64),_=>None};
    let LayoutJSONValue::Bool(fields_complete)=record.get("fieldOffsetsComplete")? else{return None};
    let LayoutJSONValue::Array(fields)=record.get("fields")? else{return None};
    let fields=fields.iter().map(|field| {
        let LayoutJSONValue::Object(field)=field else{return None};
        let get_text=|key:&str|match field.get(key)? {LayoutJSONValue::String(value)=>Some(value.clone()),_=>None};
        let get_num=|key:&str|match field.get(key)? {LayoutJSONValue::Number(value) if *value>=0=>Some(*value as u64),_=>None};
        let bit_width=match field.get("bitWidth") {None|Some(LayoutJSONValue::Null)=>None,Some(LayoutJSONValue::Number(value)) if *value>0=>Some(*value as u64),_=>return None};
        Some(crate::reconstruction::ProjectTypeLayoutField {
            name:get_text("name")?, declaration:get_text("declaration")?, offset_bits:get_num("offsetBits")?, bit_width,
        })
    }).collect::<Option<Vec<_>>>()?;
    Some(crate::reconstruction::ProjectTypeLayout {
        name:text("name")?,kind:text("kind")?,size:number("size")?,alignment:number("alignment")?,
        fields_complete:*fields_complete,fields,
    })
}

fn valid_project_layout(layout:&crate::reconstruction::ProjectTypeLayout,candidate_name:&str)->bool {
    let Some(record_bits)=layout.size.checked_mul(8) else{return false};
    layout.name==candidate_name && matches!(layout.kind.as_str(),"struct"|"union")
        && layout.size>0 && layout.size<=1_048_576 && layout.alignment.is_power_of_two()
        && layout.fields_complete && layout.fields.len()<=1024
        && layout.fields.iter().all(|field|field.name.len()<=128
            && field.name.bytes().next().is_some_and(|byte|byte.is_ascii_alphabetic()||byte==b'_')
            && field.name.bytes().all(|byte|byte.is_ascii_alphanumeric()||byte==b'_')
            && !field.declaration.is_empty() && field.declaration.len()<=512 && field.declaration.is_ascii()
            && !field.declaration.bytes().any(|byte|byte.is_ascii_control()) && field.offset_bits<record_bits
            && field.bit_width.map_or(true,|width|width>0 && width<=record_bits-field.offset_bits))
        && layout.fields.iter().map(|field|field.name.as_str()).collect::<std::collections::BTreeSet<_>>().len()==layout.fields.len()
}

fn parse_project_type_dependencies(contents: &str, source_sha256: &str, architecture: &str) -> io::Result<Vec<crate::reconstruction::ProjectTypeDependency>> {
    let compatible_architecture = |target: &str| {
        let target_arch = target.split('-').next().unwrap_or("");
        match architecture { "arm64" => matches!(target_arch,"arm64"|"aarch64"), "arm64e" => matches!(target_arch,"arm64"|"arm64e"|"aarch64"), _ => false }
    };
    let mut result = BTreeMap::<String, crate::reconstruction::ProjectTypeDependency>::new();
    for line in contents.lines().filter_map(|line| line.strip_prefix("# project_type\t")) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len()!=5 { return Err(io::Error::new(io::ErrorKind::InvalidData,"Malformed project type catalog record")) }
        let (record_digest,target,name,encoded,layout_encoded)=(fields[0],fields[1],fields[2],fields[3],fields[4]);
        if record_digest!=source_sha256||target.len()>128||!target.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'.'|b'-'))||!compatible_architecture(target)
            ||name.is_empty()||name.len()>256||!name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')
            ||encoded.is_empty()||encoded.len()>32*1024||encoded.len()%2!=0||!encoded.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"Invalid project type catalog identity or bounds"))
        }
        let bytes: Result<Vec<u8>,_> = encoded.as_bytes().chunks_exact(2).map(|pair| {
            let digit=|b:u8| match b { b'0'..=b'9'=>Some(b-b'0'),b'a'..=b'f'=>Some(b-b'a'+10),b'A'..=b'F'=>Some(b-b'A'+10),_=>None };
            Some(digit(pair[0])?*16+digit(pair[1])?)
        }).collect::<Option<Vec<_>>>().ok_or(());
        let declaration=String::from_utf8(bytes.map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"Invalid project type hex encoding"))?)
            .map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"Project type declaration is not UTF-8"))?;
        if declaration.len()>16*1024||!declaration.is_ascii()||!declaration.trim_end().ends_with(';')
            ||!(["typedef ","struct ","union ","enum "].iter().any(|prefix|declaration.trim_start().starts_with(prefix)))
            ||declaration.chars().any(|ch|ch.is_control()&&!matches!(ch,'\n'|'\t')||matches!(ch,'#'|'\\'|'"'|'\''))
            ||!declaration.split(|character: char| !character.is_ascii_alphanumeric() && character!='_').any(|token|token==name) {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"Unsafe or mismatched project type declaration"))
        }
        let (layout_evidence,layout_record) = if layout_encoded == "-" { (None,None) } else {
            if layout_encoded.len() > 128 * 1024 || layout_encoded.is_empty() || layout_encoded.len() % 2 != 0
                || !layout_encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(io::Error::new(io::ErrorKind::InvalidData,"Invalid project type layout encoding or bounds"));
            }
            let layout_bytes: Result<Vec<u8>,_> = layout_encoded.as_bytes().chunks_exact(2).map(|pair| {
                let digit=|b:u8| match b { b'0'..=b'9'=>Some(b-b'0'),b'a'..=b'f'=>Some(b-b'a'+10),b'A'..=b'F'=>Some(b-b'A'+10),_=>None };
                Some(digit(pair[0])?*16+digit(pair[1])?)
            }).collect::<Option<Vec<_>>>().ok_or(());
            let layout = String::from_utf8(layout_bytes.map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"Invalid project type layout encoding"))?)
                .map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"Project type layout is not UTF-8"))?;
            let declaration_hash = crate::reconstruction::sha256_hex_bytes(declaration.as_bytes())?;
            let bindings=layout_bindings(&layout);
            if !bindings.as_ref().is_some_and(|fields| fields.get("producer").is_some_and(|value|value=="hindsight-apple-clang-record-layout-v1")
                && fields.get("declarationsSHA256").is_some_and(|value|value.eq_ignore_ascii_case(&declaration_hash))
                && fields.get("target").is_some_and(|value|value==target)) {
                return Err(io::Error::new(io::ErrorKind::InvalidData,"Project type layout evidence is not bound to its declaration and target"));
            }
            let record=parse_layout_record(&layout).filter(|record|valid_project_layout(record,name))
                .ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"Project type compiler layout record is invalid or conflicts with its catalog candidate"))?;
            (Some(layout),Some(record))
        };
        let dependency=crate::reconstruction::ProjectTypeDependency{name:name.into(),declaration,target:target.into(),source_sha256:record_digest.into(),layout_evidence,layout:layout_record};
        if result.get(name).is_some_and(|old| old.declaration != dependency.declaration || old.target != dependency.target || old.layout_evidence != dependency.layout_evidence) {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"Conflicting project type declarations or target provenance reached the worker"))
        }
        result.insert(name.into(),dependency);
    }
    Ok(result.into_values().collect())
}

fn clean(text: &str) -> bool {
    !text.is_empty() && text.len() <= 1024 && !text.chars().any(|c| c.is_control() || matches!(c, '\\' | ';' | '{' | '}'))
}

fn objc_object_type(annotation: &str) -> Option<String> {
    let valid=|value:&str|!value.is_empty()&&value.len()<=256&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'$'|b'.'));
    let class=annotation.split('<').next().unwrap_or("");let mut rest=&annotation[class.len()..];let mut protocols=Vec::new();
    while !rest.is_empty(){let body=rest.strip_prefix('<')?;let end=body.find('>')?;for protocol in body[..end].split(','){if !valid(protocol)||protocols.len()>=32{return None}protocols.push(protocol);}rest=&body[end+1..];}
    if !class.is_empty()&&!valid(class){return None};if protocols.is_empty(){return (!class.is_empty()).then(||format!("{class} *"));}
    let qualified=protocols.join(", ");Some(if class.is_empty(){format!("id<{qualified}>")}else{format!("{class}<{qualified}> *")})
}

fn objc_type(bytes: &[u8], at: &mut usize, depth: usize) -> Option<String> {
    if depth > 8 { return None }
    while *at < bytes.len() && b"rnNoORV".contains(&bytes[*at]) { *at += 1; }
    let code = *bytes.get(*at)?; *at += 1;
    let ty = match code {
        b'v' => "void".into(), b'c' => "char".into(), b'C' => "unsigned char".into(),
        b's' => "short".into(), b'S' => "unsigned short".into(),
        b'i' => "int".into(), b'I' => "unsigned int".into(),
        b'l' => "long".into(), b'L' => "unsigned long".into(),
        b'q' => "long long".into(), b'Q' => "unsigned long long".into(),
        b'f' => "float".into(), b'd' => "double".into(), b'B' => "bool".into(),
        b'@' => {
            if bytes.get(*at) == Some(&b'?') { *at += 1; "id /* block */".into() }
            else if bytes.get(*at) == Some(&b'"') {
                *at += 1; let start = *at;
                while bytes.get(*at).is_some_and(|b| *b != b'"') { *at += 1; }
                if bytes.get(*at) != Some(&b'"') { return None }
                let name = std::str::from_utf8(&bytes[start..*at]).ok()?; *at += 1;
                objc_object_type(name)?
            } else { "id".into() }
        }
        b'#' => "Class".into(), b':' => "SEL".into(), b'*' => "char *".into(),
        b'^' => format!("{} *", objc_type(bytes, at, depth + 1)?),
        b'{' => {
            // Require the complete Apple LP64 encoding, not just a familiar
            // struct name. Anonymous/custom/quoted-field layouts stay unknown.
            let start = *at - 1;
            let known = [
                ("{CGPoint=dd}", "CGPoint"), ("{CGSize=dd}", "CGSize"),
                ("{CGRect={CGPoint=dd}{CGSize=dd}}", "CGRect"),
                ("{_NSRange=QQ}", "NSRange"), ("{CFRange=qq}", "CFRange"),
                ("{CGAffineTransform=dddddd}", "CGAffineTransform"),
            ].into_iter().find(|(encoding, _)| bytes[start..].starts_with(encoding.as_bytes()))?;
            *at = start + known.0.len();
            known.1.into()
        }
        // Aggregates and unknown encodings cannot safely be rendered as a
        // pointer: their ABI passing convention may be different.
        b'?' | b'(' | b'[' => return None,
        _ => return None,
    };
    Some(ty)
}

fn skip_number(bytes: &[u8], at: &mut usize) {
    if bytes.get(*at) == Some(&b'-') { *at += 1 }
    while bytes.get(*at).is_some_and(u8::is_ascii_digit) { *at += 1 }
}

fn foundation_method(name: &str) -> Option<&'static str> {
    Some(match name {
        "-[NSObject class]" => "- (Class)class;",
        "-[NSObject superclass]" => "- (Class)superclass;",
        "-[NSObject description]" => "- (NSString *)description;",
        "-[NSObject respondsToSelector:]" => "- (BOOL)respondsToSelector:(SEL)aSelector;",
        "-[NSObject isKindOfClass:]" => "- (BOOL)isKindOfClass:(Class)aClass;",
        "-[NSString length]" => "- (NSUInteger)length;",
        "-[NSString UTF8String]" => "- (const char *)UTF8String;",
        "-[NSString characterAtIndex:]" => "- (unichar)characterAtIndex:(NSUInteger)index;",
        "-[NSString isEqualToString:]" => "- (BOOL)isEqualToString:(NSString *)aString;",
        "-[NSString substringFromIndex:]" => "- (NSString *)substringFromIndex:(NSUInteger)from;",
        "-[NSString substringToIndex:]" => "- (NSString *)substringToIndex:(NSUInteger)to;",
        "-[NSString stringByAppendingString:]" => "- (NSString *)stringByAppendingString:(NSString *)aString;",
        "+[NSString stringWithUTF8String:]" => "+ (instancetype)stringWithUTF8String:(const char *)bytes;",
        "-[NSArray count]" => "- (NSUInteger)count;",
        "-[NSArray objectAtIndex:]" => "- (id)objectAtIndex:(NSUInteger)index;",
        "-[NSArray containsObject:]" => "- (BOOL)containsObject:(id)anObject;",
        "-[NSArray indexOfObject:]" => "- (NSUInteger)indexOfObject:(id)anObject;",
        "+[NSArray array]" => "+ (instancetype)array;",
        "-[NSDictionary count]" => "- (NSUInteger)count;",
        "-[NSDictionary objectForKey:]" => "- (id)objectForKey:(id)aKey;",
        "-[NSDictionary allKeys]" => "- (NSArray *)allKeys;",
        "-[NSDictionary allValues]" => "- (NSArray *)allValues;",
        "+[NSDictionary dictionary]" => "+ (instancetype)dictionary;",
        "-[NSData length]" => "- (NSUInteger)length;",
        "-[NSData bytes]" => "- (const void *)bytes;",
        "-[NSURL absoluteString]" => "- (NSString *)absoluteString;",
        "-[NSURL path]" => "- (NSString *)path;",
        "-[NSBundle bundleIdentifier]" => "- (NSString *)bundleIdentifier;",
        "-[NSBundle pathForResource:ofType:]" => "- (NSString *)pathForResource:(NSString *)name ofType:(NSString *)extension;",
        "-[NSBundle URLForResource:withExtension:]" => "- (NSURL *)URLForResource:(NSString *)name withExtension:(NSString *)extension;",
        "-[NSError code]" => "- (NSInteger)code;",
        "-[NSError domain]" => "- (NSString *)domain;",
        "-[NSError userInfo]" => "- (NSDictionary *)userInfo;",
        "-[NSNumber boolValue]" => "- (BOOL)boolValue;",
        "-[NSNumber integerValue]" => "- (NSInteger)integerValue;",
        "-[NSNumber doubleValue]" => "- (double)doubleValue;",
        "+[NSNumber numberWithBool:]" => "+ (NSNumber *)numberWithBool:(BOOL)value;",
        "-[NSSet count]" => "- (NSUInteger)count;",
        "-[NSSet containsObject:]" => "- (BOOL)containsObject:(id)anObject;",
        "-[NSFileManager fileExistsAtPath:]" => "- (BOOL)fileExistsAtPath:(NSString *)path;",
        "-[NSFileManager contentsAtPath:]" => "- (NSData *)contentsAtPath:(NSString *)path;",
        "-[NSUserDefaults objectForKey:]" => "- (id)objectForKey:(NSString *)defaultName;",
        "-[NSUserDefaults setObject:forKey:]" => "- (void)setObject:(id)value forKey:(NSString *)defaultName;",
        _ => return None,
    })
}

fn objc_selector_syntax(selector: &str) -> bool {
    if selector.is_empty() { return false; }
    let count = selector.bytes().filter(|b| *b == b':').count();
    if count > 32 || (count > 0 && !selector.ends_with(':')) { return false; }
    selector.split(':').all(|part| part.is_empty() ||
        (part.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_') &&
         part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
}

pub fn objc_method(name: &str, encoding: &str) -> Option<Signature> {
    if !clean(name) || encoding.is_empty() || encoding.len() > 1024 || !encoding.is_ascii() || encoding.bytes().any(|b| b.is_ascii_control()) || !(name.starts_with("-[") || name.starts_with("+[")) || !name.ends_with(']') { return None }
    let bytes = encoding.as_bytes(); let mut at = 0;
    let result = objc_type(bytes, &mut at, 0)?; skip_number(bytes, &mut at);
    let self_type = objc_type(bytes, &mut at, 0)?; skip_number(bytes, &mut at);
    let selector_type = objc_type(bytes, &mut at, 0)?; skip_number(bytes, &mut at);
    if !matches!(self_type.as_str(), "id" | "Class") || selector_type != "SEL" { return None }
    let body = &name[2..name.len()-1]; let (_, selector) = body.split_once(' ')?;
    if !objc_selector_syntax(selector) { return None }
    let count = selector.bytes().filter(|b| *b == b':').count();
    let mut args = Vec::new();
    for _ in 0..count { args.push(objc_type(bytes, &mut at, 0)?); skip_number(bytes, &mut at); }
    if at != bytes.len() { return None }
    let mut declaration = format!("{} ({result})", &name[..1]);
    if count == 0 { declaration.push_str(selector); }
    else {
        let labels = selector.split(':').take(count);
        for (i, (label, ty)) in labels.zip(args).enumerate() {
            if i > 0 { declaration.push(' '); }
            declaration.push_str(&format!("{label}:({ty})arg{i}"));
        }
    }
    declaration.push(';');
    if let Some(sdk_declaration) = foundation_method(name) {
        let catalog = Signature { language: "Objective-C / Objective-C++", declaration: sdk_declaration.into(), source: "exact Foundation SDK method declaration; encoding ABI validated" };
        let mut encoded = Signature { language: "Objective-C / Objective-C++", declaration, source: "validated Objective-C method type encoding" };
        let expected_parameters = objc_parameters(&catalog);
        let actual_parameters = objc_parameters(&encoded);
        let compatible = signature_return_type(&catalog).zip(signature_return_type(&encoded))
            .is_some_and(|(expected, actual)| abi_types_agree(&expected, &actual))
            && expected_parameters.len() == actual_parameters.len()
            && expected_parameters.iter().zip(&actual_parameters).all(|((expected,_),(actual,_))| abi_types_agree(expected,actual));
        if compatible { return Some(catalog); }
        encoded.source = "validated Objective-C encoding; exact SDK declaration conflicts";
        return Some(encoded);
    }
    Some(Signature { language: "Objective-C / Objective-C++", declaration, source: "validated Objective-C method type encoding" })
}

fn objc_value_type(encoding: &str) -> Option<String> {
    let mut at = 0;
    let result = objc_type(encoding.as_bytes(), &mut at, 0)?;
    (at == encoding.len()).then_some(result)
}

fn object_declarations(layouts: &[super::objc::ClassLayout]) -> String {
    let mut output = String::new();
    for layout in layouts.iter().take(64).filter(|layout|super::objc::identifier(&layout.name)) {
        let source = if layout.runtime { "libobjc-runtime (target initializers executed)" } else { "macho-objc-metadata" };
        let mut declaration = format!("// target class metadata={:#x}; layout source={source}; instance size={} bytes\n@interface {} {{\n", layout.address, layout.instance_size, layout.name);
        if !layout.ivars_complete||!layout.properties_complete{declaration.push_str("    /* partial member metadata; missing members/layout remain unresolved */\n")}
        for ivar in &layout.ivars {
            let Some(ty) = objc_value_type(&ivar.encoding) else { continue };
            if layout.runtime {
                declaration.push_str(&format!("    {ty} {}; /* runtime ivar; offset={:#x}; size={}; alignment={} */\n",
                    ivar.name, ivar.offset, ivar.size, ivar.alignment));
            } else {
                declaration.push_str(&format!("    {ty} {}; /* ivar={:#x}; offset_slot={:#x}; declared_offset={:#x}; size={}; alignment={} */\n",
                    ivar.name, ivar.record, ivar.offset_pointer, ivar.offset, ivar.size, ivar.alignment));
            }
        }
        declaration.push_str("}\n");
        for property in &layout.properties {
            let Some(ty) = objc_value_type(&property.encoding) else { continue };
            let mut attributes = Vec::new();
            if property.readonly { attributes.push("readonly".to_owned()); }
            if property.nonatomic { attributes.push("nonatomic".to_owned()); }
            if let Some(ownership) = property.ownership { attributes.push(ownership.to_owned()); }
            if property.getter != property.name { attributes.push(format!("getter={}",property.getter)); }
            if let Some(setter) = &property.setter { attributes.push(format!("setter={setter}")); }
            let attributes = if attributes.is_empty() { String::new() } else { format!("({}) ",attributes.join(", ")) };
            let note=if layout.runtime { "runtime property".to_owned() } else { format!("property={:#x}",property.record) };
            declaration.push_str(&format!("@property {attributes}{ty} {}; /* {note}; backing_ivar={} */\n",
                property.name,property.ivar.as_deref().unwrap_or("unresolved")));
        }
        declaration.push_str("@end\n\n");
        if output.len()+declaration.len()>12_000 { break; }
        output.push_str(&declaration);
    }
    output
}

/// A target-local class review header, never an assertion of complete layout
/// or implementation. Only one unambiguous IMP owner may supply its members.
pub fn objc_header_for_imp(address:u64, records:&str, layouts:&[super::objc::ClassLayout])->Option<String>{
    let mut owners=layouts.iter().filter(|layout|layout.methods.iter().any(|method|method.implementation==address));
    let owner=owners.next()?;
    if owners.next().is_some()||!super::objc::identifier(&owner.name){return None}
    let declaration=object_declarations(std::slice::from_ref(owner));
    let mut declaration=declaration.strip_suffix("@end\n\n")?.to_owned();
    let mut relationships=std::collections::BTreeSet::new();
    for line in records.lines().take(8192).filter(|line|line.starts_with("reconstructed class ")&&line.len()<=4096){
        let fields=line.split_whitespace().collect::<Vec<_>>();
        if fields.get(2).and_then(|value|u64::from_str_radix(value.trim_start_matches("0x"),16).ok())!=Some(owner.address)
            ||!fields.iter().any(|field|*field==format!("name={}",owner.name)){continue}
        let parent=fields.iter().find_map(|field|field.strip_prefix("super_name="))
            .filter(|parent|super::objc::identifier(parent)&&*parent!=owner.name).map(str::to_owned);
        let protocols=fields.iter().find_map(|field|field.strip_prefix("conforms="))
            .map(|values|values.split(',').filter(|value|super::objc::identifier(value)).take(32).map(str::to_owned).collect::<std::collections::BTreeSet<_>>()).unwrap_or_default();
        relationships.insert((parent,protocols));
    }
    if relationships.len()==1{
        let (parent,protocols)=relationships.iter().next()?;
        let inheritance=parent.as_ref().map(|name|format!(" : {name}")).unwrap_or_default();
        let adoption=if protocols.is_empty(){String::new()}else{format!(" <{}>",protocols.iter().cloned().collect::<Vec<_>>().join(", "))};
        declaration=declaration.replacen(&format!("@interface {} {{",owner.name),&format!("@interface {}{inheritance}{adoption} {{",owner.name),1);
        if !protocols.is_empty(){declaration=format!("@protocol {};\n{declaration}",protocols.iter().cloned().collect::<Vec<_>>().join(", "));}
    }else if relationships.len()>1{declaration.insert_str(0,"/* Conflicting superclass/conformance records: relationships withheld. */\n");}
    let mut methods=BTreeMap::<String,std::collections::BTreeSet<String>>::new();
    for (imp,(name,signature)) in objc_signature_candidates(records){
        let Some((class,selector))=name.get(2..name.len().saturating_sub(1)).and_then(|body|body.split_once(' '))else{continue};
        let class_method=name.starts_with('+');
        if class!=owner.name||!owner.methods.iter().any(|method|method.implementation==imp&&method.class_method==class_method)
            ||!selector.trim_end_matches(':').split(':').all(super::objc::identifier){continue}
        methods.entry(name).or_default().insert(signature.declaration);
    }
    for declarations in methods.values(){
        if declarations.len()==1{declaration.push_str(declarations.iter().next()?);declaration.push('\n');}
        else{declaration.push_str("/* Conflicting method type encodings: declaration withheld. */\n");}
        if declaration.len()>60*1024{return None}
    }
    declaration.push_str("@end\n");
    Some(format!("#pragma once\n/* Reconstructed target-local Objective-C declaration evidence.\n * REVIEW ONLY: superclass, adopted protocols and unsupported types may be unresolved.\n * Field offsets are evidence, not a compilable layout or implementation guarantee.\n * Restore the actual superclass/type dependencies before compiling. */\n#if !defined(__OBJC__)\n#error This reconstructed header requires Objective-C or Objective-C++\n#endif\n#import <Foundation/Foundation.h>\n#import <CoreFoundation/CoreFoundation.h>\n#import <CoreGraphics/CoreGraphics.h>\n{declaration}"))
}

fn object_layout_for_imp<'a>(layouts:&'a [super::objc::ClassLayout], address:u64, name:&str, signature:Option<&Signature>)->Option<&'a super::objc::ClassLayout>{
    if !signature.is_some_and(|signature|signature.language=="Objective-C / Objective-C++"){return None}
    let class_name=name.strip_prefix("-[")?.split_once(' ')?.0;
    let mut owners=layouts.iter().filter(|layout|layout.methods.iter().any(|method|method.implementation==address));
    let owner=owners.next()?;
    if owners.next().is_some()||owner.name!=class_name||!owner.methods.iter().any(|method|!method.class_method&&method.implementation==address){return None}
    Some(owner)
}

fn refine_property_signatures(signatures:&mut BTreeMap<u64,(String,Signature)>, layouts:&[super::objc::ClassLayout]) {
    for (address,(name,signature)) in signatures.iter_mut() {
        let Some(layout)=object_layout_for_imp(layouts,*address,name,Some(signature))else{continue};
        if !layout.properties_complete{continue}
        let mut proposals=std::collections::BTreeSet::new();
        let mut incompatible=false;
        for property in &layout.properties {
            let relevant=name==&format!("-[{} {}]",layout.name,property.getter)
                ||property.setter.as_ref().is_some_and(|setter|name==&format!("-[{} {setter}]",layout.name));
            if !relevant{continue}
            let Some(ty)=objc_value_type(&property.encoding)else{incompatible=true;continue};
            if !matches!(abi_type(&ty),Some(AbiType::Integer(8)))||abi_integer_kind(&ty)!=Some(AbiIntegerKind::Pointer){incompatible=true;continue}
            if name==&format!("-[{} {}]",layout.name,property.getter)
                &&signature_return_type(signature).as_deref()==Some("id")&&objc_parameters(signature).is_empty(){
                proposals.insert(format!("- ({ty}){};",property.getter));
            }else if property.setter.as_ref().is_some_and(|setter|name==&format!("-[{} {setter}]",layout.name))
                &&signature_return_type(signature).as_deref()==Some("void"){
                let parameters=objc_parameters(signature);
                if parameters.len()==1&&parameters[0].0=="id"{
                    let setter=property.setter.as_ref().unwrap().trim_end_matches(':');
                    proposals.insert(format!("- (void){setter}:({ty})arg0;"));
                }
            }
        }
        if !incompatible&&proposals.len()==1 {
            signature.declaration=proposals.into_iter().next().unwrap();
            signature.source="validated target property object type and accessor method encoding";
        }
    }
}

fn object_member_statements(code:&[u8],base:u64,start:u64,end:u64,file_offset:u64,data_ranges:&[(u64,u64,u16)],layout:&super::objc::ClassLayout) -> BTreeMap<u64,String> {
    let slots=layout.ivars.iter().map(|ivar|ivar.offset_pointer).collect();
    super::semantic::object_member_accesses(code,base,start,end,file_offset,data_ranges,&slots).into_iter().filter_map(|(site,access)|{
        let offset=if let Some(slot)=access.offset_pointer {
            let mut candidates=layout.ivars.iter().filter(|ivar|ivar.offset_pointer==slot);
            let field=candidates.next()?;if candidates.next().is_some(){return None}u64::from(field.offset)
        }else{access.offset?};
        let field=layout.member(offset,access.bytes,access.floating)?;
        let ty=objc_value_type(&field.encoding)?;
        let expected=match abi_type(&ty)? { AbiType::Integer(width)=>width,AbiType::Float(width)=>width,_=>return None };
        if expected!=access.bytes{return None}
        let expression=format!("self->{}",field.name);
        let statement=if access.write {format!("{expression} = ({ty}){};",access.register)}
            else if access.signed&&access.result_bits==64&&access.bytes<8 {
                if !matches!(abi_integer_kind(&ty),Some(AbiIntegerKind::Signed)){return None}
                format!("{} = (int64_t){expression};",access.register)
            }else{format!("{} = {expression};",access.register)};
        Some((site,format!("{statement} /* ivar metadata={:#x}; offset_slot={:#x}; type={}; receiver proven from instance IMP x0 */",field.record,field.offset_pointer,field.encoding)))
    }).collect()
}

/// Protocol declarations carry selector/type evidence but never prove an IMP,
/// a receiver class, or that an optional method is implemented.
pub fn protocol_signatures(records: &str) -> String {
    let mut owner = None::<(String, Vec<String>)>;
    let mut output = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in records.lines().take(8192) {
        let line = line.trim();
        if line.len() > 4096 { owner = None; continue; }
        if line.starts_with("protocol ") {
            owner = line.split(" name=").nth(1).and_then(|tail| {
                let (fields, confidence)=tail.rsplit_once(" confidence=")?;
                let mut tokens=fields.split_whitespace();let name=tokens.next()?;
                let valid=|value:&str|!value.is_empty()&&value.len()<=256&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'$'|b'.'));
                if confidence!="validated"||!valid(name){return None}
                let adopted=tokens.find_map(|field|field.strip_prefix("adopts=")).map(|parents|parents.split(',').filter(|parent|valid(parent)&&*parent!=name).take(32).map(str::to_owned).collect()).unwrap_or_default();
                Some((name.to_string(),adopted))
            });
            continue;
        }
        if line.starts_with("category ") || line.starts_with("reference ") { owner = None; }
        let Some((owner,adopted)) = owner.as_ref() else { continue; };
        let Some((kind, _)) = line.split_once(" method ") else { continue; };
        if !matches!(kind, "required-instance" | "required-class" | "optional-instance" | "optional-class") || !line.ends_with("confidence=validated") { continue; }
        let Some((selector, tail)) = line.split(" name=").nth(1).and_then(|tail| tail.split_once("  types=")) else { continue; };
        if selector.is_empty() || selector.len() > 512 || !selector.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':')) { continue; }
        let Some((encoding, _)) = tail.split_once("  imp=") else { continue; };
        let class_method = kind.ends_with("-class");
        // Deliberately avoid the Foundation class-name catalog for protocols.
        let synthetic = format!("{}[__protocol__{owner} {selector}]", if class_method { '+' } else { '-' });
        let Some(signature) = objc_method(&synthetic, encoding) else { continue; };
        let key = (owner.to_owned(), kind.to_owned(), selector.to_owned(), encoding.to_owned());
        if !seen.insert(key) { continue; }
        let inheritance=if adopted.is_empty(){String::new()}else{format!(" <{}>",adopted.join(", "))};
        let adoption_evidence=if adopted.is_empty(){String::new()}else{format!(" adopts={} inheritance=validated",adopted.join(","))};
        let row = format!("protocol={owner}{adoption_evidence} membership={kind} selector={selector} source=validated-protocol-method-encoding implementation=unproven\n@protocol {owner}{inheritance}\n@{}\n{}\n@end\n    ABI profile=Apple-arm64-LP64 bindings=[{}] result={}\n",
            if kind.starts_with("optional-") { "optional" } else { "required" }, signature.declaration,
            abi_parameter_bindings(Some(&signature), &[]).join(", "), render_signature_return(Some(&signature)));
        if output.len() + row.len() > 20_000 || seen.len() > 128 {
            output.push_str("Protocol prototype output truncated at declaration/byte budget.\n");
            break;
        }
        output.push_str(&row);
    }
    if !output.is_empty() { output.insert_str(0, "Protocol prototype evidence; declarations do not establish implementations or dispatch targets.\n"); }
    output
}

/// A class RO or category protocol list proves a declared conformance. It does not prove
/// that any required method has a local implementation or resolve dispatch.
pub fn objc_conformance_declarations(records: &str) -> String {
    let valid=|value:&str|!value.is_empty()&&value.len()<=256&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'$'|b'.'));
    let mut seen=std::collections::BTreeSet::new();let mut output=String::new();let mut parents=std::collections::BTreeMap::new();let mut direct=std::collections::BTreeMap::<String,Vec<String>>::new();
    for line in records.lines().take(8192).map(str::trim).filter(|line|line.starts_with("reconstructed class ")&&line.len()<=4096) {
        let Some(name)=line.split("  name=").nth(1).and_then(|tail|tail.split_whitespace().next()).filter(|name|valid(name))else{continue};
        if let Some(parent)=line.split_whitespace().find_map(|field|field.strip_prefix("super_name=")).filter(|parent|valid(parent)){parents.insert(name.to_owned(),parent.to_owned());}
        let protocols=line.split_whitespace().find_map(|field|field.strip_prefix("conforms=")).map(|values|values.split(',').filter(|protocol|valid(protocol)).take(32).map(str::to_owned).collect()).unwrap_or_default();direct.insert(name.to_owned(),protocols);
    }
    for line in records.lines().take(8192) {
        let line=line.trim();if line.len()>4096{continue}
        let (name,category,source)=if line.starts_with("reconstructed class ") {
            let Some(name)=line.split("  name=").nth(1).and_then(|tail|tail.split_whitespace().next())else{continue};(name,None,"validated-class-ro-protocol-list")
        } else if line.starts_with("category ") {
            let Some(category)=line.split_whitespace().find_map(|field|field.strip_prefix("name="))else{continue};let Some(name)=line.split_whitespace().find_map(|field|field.strip_prefix("class_name="))else{continue};(name,Some(category),"validated-category-protocol-list")
        } else {continue};
        let Some(protocols)=line.split_whitespace().find_map(|field|field.strip_prefix("conforms="))else{continue};
        if !valid(name)||category.is_some_and(|value|!valid(value)){continue}let conformances=protocols.split(',').filter(|protocol|valid(protocol)).take(32).collect::<Vec<_>>();
        if conformances.is_empty(){continue}let key=(name.to_owned(),category.unwrap_or("").to_owned(),conformances.join(","));if !seen.insert(key){continue}
        let category_declaration=category.map(|value|format!(" ({value})")).unwrap_or_default();let category_evidence=category.map(|value|format!(" category={value}")).unwrap_or_default();
        let row=format!("class={name}{category_evidence} conforms={} source={source} implementation-coverage=unproven dispatch-targets=unresolved\n@interface {name}{category_declaration} <{}>\n@end\n",conformances.join(","),conformances.join(", "));
        if output.len()+row.len()>12_000||seen.len()>128{output.push_str("Objective-C conformance output truncated at declaration/byte budget.\n");break}output.push_str(&row);
    }
    for (name,parent) in &parents {let mut inherited=std::collections::BTreeSet::new();let mut chain=std::collections::BTreeSet::new();let mut ancestor=Some(parent.as_str());for _ in 0..64 {let Some(current)=ancestor else{break};if !chain.insert(current.to_owned()){break};if let Some(protocols)=direct.get(current){inherited.extend(protocols.iter().cloned());}ancestor=parents.get(current).map(String::as_str);}if let Some(own)=direct.get(name){for protocol in own{inherited.remove(protocol);}};if inherited.is_empty(){continue}let protocols=inherited.into_iter().collect::<Vec<_>>();let row=format!("class={name} inherits-conformance={} via={parent} source=validated-superclass-chain implementation-coverage=unproven dispatch-targets=unresolved\n@interface {name} : {parent} /* inherited protocols: {} */\n@end\n",protocols.join(","),protocols.join(", "));if output.len()+row.len()>12_000||seen.len()>128{output.push_str("Objective-C conformance output truncated at declaration/byte budget.\n");break}output.push_str(&row);}
    if !output.is_empty(){output.insert_str(0,"Objective-C class/category conformance evidence; conformance does not establish method implementations.\n")}output
}

// Exact SDK ABI declarations. No prefix or substring matching: unknown APIs stay unknown.
fn api(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "_CFRelease" => ("void CFRelease(CFTypeRef cf);", "CoreFoundation"),
        "_CFRetain" => ("CFTypeRef CFRetain(CFTypeRef cf);", "CoreFoundation"),
        "_CFGetTypeID" => ("CFTypeID CFGetTypeID(CFTypeRef cf);", "CoreFoundation"),
        "_CFStringGetLength" => ("CFIndex CFStringGetLength(CFStringRef theString);", "CoreFoundation"),
        "_CFStringGetTypeID" => ("CFTypeID CFStringGetTypeID(void);", "CoreFoundation"),
        "_CFStringCreateWithCString" => ("CFStringRef CFStringCreateWithCString(CFAllocatorRef alloc, const char *cStr, CFStringEncoding encoding);", "CoreFoundation"),
        "_CFStringCreateWithBytes" => ("CFStringRef CFStringCreateWithBytes(CFAllocatorRef alloc, const UInt8 *bytes, CFIndex numBytes, CFStringEncoding encoding, Boolean isExternalRepresentation);", "CoreFoundation"),
        "_CFStringGetCString" => ("Boolean CFStringGetCString(CFStringRef theString, char *buffer, CFIndex bufferSize, CFStringEncoding encoding);", "CoreFoundation"),
        "_CFStringGetCStringPtr" => ("const char *CFStringGetCStringPtr(CFStringRef theString, CFStringEncoding encoding);", "CoreFoundation"),
        "_CFStringCompare" => ("CFComparisonResult CFStringCompare(CFStringRef theString1, CFStringRef theString2, CFStringCompareFlags compareOptions);", "CoreFoundation"),
        "_CFStringGetCharacterAtIndex" => ("UniChar CFStringGetCharacterAtIndex(CFStringRef theString, CFIndex idx);", "CoreFoundation"),
        "_CFArrayCreate" => ("CFArrayRef CFArrayCreate(CFAllocatorRef allocator, const void **values, CFIndex numValues, const CFArrayCallBacks *callBacks);", "CoreFoundation"),
        "_CFArrayGetTypeID" => ("CFTypeID CFArrayGetTypeID(void);", "CoreFoundation"),
        "_CFArrayGetCount" => ("CFIndex CFArrayGetCount(CFArrayRef theArray);", "CoreFoundation"),
        "_CFArrayGetValueAtIndex" => ("const void *CFArrayGetValueAtIndex(CFArrayRef theArray, CFIndex idx);", "CoreFoundation"),
        "_CFArrayContainsValue" => ("Boolean CFArrayContainsValue(CFArrayRef theArray, CFRange range, const void *value);", "CoreFoundation"),
        "_CFDictionaryCreate" => ("CFDictionaryRef CFDictionaryCreate(CFAllocatorRef allocator, const void **keys, const void **values, CFIndex numValues, const CFDictionaryKeyCallBacks *keyCallBacks, const CFDictionaryValueCallBacks *valueCallBacks);", "CoreFoundation"),
        "_CFDictionaryGetTypeID" => ("CFTypeID CFDictionaryGetTypeID(void);", "CoreFoundation"),
        "_CFDictionaryGetCount" => ("CFIndex CFDictionaryGetCount(CFDictionaryRef theDict);", "CoreFoundation"),
        "_CFDictionaryGetValue" => ("const void *CFDictionaryGetValue(CFDictionaryRef theDict, const void *key);", "CoreFoundation"),
        "_CFDictionaryGetValueIfPresent" => ("Boolean CFDictionaryGetValueIfPresent(CFDictionaryRef theDict, const void *key, const void **value);", "CoreFoundation"),
        "_CFBooleanGetValue" => ("Boolean CFBooleanGetValue(CFBooleanRef boolean);", "CoreFoundation"),
        "_CFNumberGetValue" => ("Boolean CFNumberGetValue(CFNumberRef number, CFNumberType theType, void *valuePtr);", "CoreFoundation"),
        "_CFNumberCreate" => ("CFNumberRef CFNumberCreate(CFAllocatorRef allocator, CFNumberType theType, const void *valuePtr);", "CoreFoundation"),
        "_CFDataCreate" => ("CFDataRef CFDataCreate(CFAllocatorRef allocator, const UInt8 *bytes, CFIndex length);", "CoreFoundation"),
        "_CFDataGetTypeID" => ("CFTypeID CFDataGetTypeID(void);", "CoreFoundation"),
        "_CFDataGetLength" => ("CFIndex CFDataGetLength(CFDataRef theData);", "CoreFoundation"),
        "_CFDataGetBytePtr" => ("const UInt8 *CFDataGetBytePtr(CFDataRef theData);", "CoreFoundation"),
        "_CFDataGetBytes" => ("void CFDataGetBytes(CFDataRef theData, CFRange range, UInt8 *buffer);", "CoreFoundation"),
        "_CFBundleGetMainBundle" => ("CFBundleRef CFBundleGetMainBundle(void);", "CoreFoundation"),
        "_CFBundleGetValueForInfoDictionaryKey" => ("CFTypeRef CFBundleGetValueForInfoDictionaryKey(CFBundleRef bundle, CFStringRef key);", "CoreFoundation"),
        "_CFURLCreateWithString" => ("CFURLRef CFURLCreateWithString(CFAllocatorRef allocator, CFStringRef URLString, CFURLRef baseURL);", "CoreFoundation"),
        "_CFURLCopyFileSystemPath" => ("CFStringRef CFURLCopyFileSystemPath(CFURLRef anURL, CFURLPathStyle pathStyle);", "CoreFoundation"),
        "_CFPropertyListCreateWithData" => ("CFPropertyListRef CFPropertyListCreateWithData(CFAllocatorRef allocator, CFDataRef data, CFOptionFlags options, CFPropertyListFormat *format, CFErrorRef *error);", "CoreFoundation"),
        "_CFErrorCreate" => ("CFErrorRef CFErrorCreate(CFAllocatorRef allocator, CFStringRef domain, CFIndex code, CFDictionaryRef userInfo);", "CoreFoundation"),
        "_CFErrorGetDomain" => ("CFStringRef CFErrorGetDomain(CFErrorRef err);", "CoreFoundation"),
        "_CFErrorGetCode" => ("CFIndex CFErrorGetCode(CFErrorRef err);", "CoreFoundation"),
        "_CFRunLoopGetCurrent" => ("CFRunLoopRef CFRunLoopGetCurrent(void);", "CoreFoundation"),
        "_CFRunLoopRun" => ("void CFRunLoopRun(void);", "CoreFoundation"),
        "_objc_getClass" => ("Class objc_getClass(const char *name);", "Objective-C runtime"),
        "_objc_getMetaClass" => ("Class objc_getMetaClass(const char *name);", "Objective-C runtime"),
        "_objc_lookUpClass" => ("Class objc_lookUpClass(const char *name);", "Objective-C runtime"),
        "_NSGetExecutablePath" => ("int _NSGetExecutablePath(char *buf, uint32_t *bufsize);", "Foundation / dyld"),
        "_malloc" => ("void *malloc(size_t size);", "C library"),
        "_calloc" => ("void *calloc(size_t count, size_t size);", "C library"),
        "_realloc" => ("void *realloc(void *ptr, size_t size);", "C library"),
        "_free" => ("void free(void *ptr);", "C library"),
        "_memcpy" => ("void *memcpy(void *restrict dest, const void *restrict src, size_t n);", "C library"),
        "_memmove" => ("void *memmove(void *dest, const void *src, size_t n);", "C library"),
        "_memset" => ("void *memset(void *dest, int value, size_t n);", "C library"),
        "_memcmp" => ("int memcmp(const void *left, const void *right, size_t n);", "C library"),
        "_bzero" => ("void bzero(void *dest, size_t n);", "C library"),
        "_strlen" => ("size_t strlen(const char *s);", "C library"),
        "_strnlen" => ("size_t strnlen(const char *s, size_t maxlen);", "C library"),
        "_strcmp" => ("int strcmp(const char *left, const char *right);", "C library"),
        "_strncmp" => ("int strncmp(const char *left, const char *right, size_t n);", "C library"),
        "_strdup" => ("char *strdup(const char *s);", "C library"),
        "_printf" => ("int printf(const char *format, ...);", "C library"),
        "_fprintf" => ("int fprintf(FILE *stream, const char *format, ...);", "C library"),
        "_sprintf" => ("int sprintf(char *buffer, const char *format, ...);", "C library"),
        "_snprintf" => ("int snprintf(char *buffer, size_t size, const char *format, ...);", "C library"),
        "_NSLog" => ("void NSLog(NSString *format, ...);", "Foundation"),
        "_abort" => ("void abort(void);", "C library"),
        "_exit" => ("void _exit(int status);", "C library"),
        "_dispatch_async" => ("void dispatch_async(dispatch_queue_t queue, dispatch_block_t block);", "libdispatch"),
        "_dispatch_sync" => ("void dispatch_sync(dispatch_queue_t queue, dispatch_block_t block);", "libdispatch"),
        "_dispatch_once" => ("void dispatch_once(dispatch_once_t *predicate, dispatch_block_t block);", "libdispatch"),
        "_objc_retain" => ("id objc_retain(id object);", "Objective-C runtime"),
        "_objc_release" => ("void objc_release(id object);", "Objective-C runtime"),
        "_objc_autoreleaseReturnValue" => ("id objc_autoreleaseReturnValue(id object);", "Objective-C runtime"),
        _ => return None,
    })
}

fn cpp_type(bytes: &[u8], at: &mut usize, depth: usize) -> Option<String> {
    if depth > 8 { return None }
    let code = *bytes.get(*at)?; *at += 1;
    Some(match code {
        b'v' => "void".into(), b'b' => "bool".into(), b'c' => "char".into(),
        b'a' => "signed char".into(), b'h' => "unsigned char".into(),
        b's' => "short".into(), b't' => "unsigned short".into(),
        b'i' => "int".into(), b'j' => "unsigned int".into(),
        b'l' => "long".into(), b'm' => "unsigned long".into(),
        b'x' => "long long".into(), b'y' => "unsigned long long".into(),
        b'f' => "float".into(), b'd' => "double".into(), b'e' => "long double".into(),
        b'w' => "wchar_t".into(), b'n' => "__int128".into(), b'o' => "unsigned __int128".into(),
        b'D' => { let suffix = *bytes.get(*at)?; *at += 1; match suffix {
            b's' => "char16_t".into(), b'i' => "char32_t".into(), b'u' => "char8_t".into(),
            _ => return None,
        } },
        b'P' => {
            // A pointer to a reference is not a C++ type. In particular, do
            // not turn malformed input into a plausible-looking declaration.
            if matches!(bytes.get(*at), Some(b'R' | b'O')) { return None }
            format!("{} *", cpp_type(bytes, at, depth + 1)?)
        },
        b'R' | b'O' => {
            // C++ cannot form references to void or references. Reject these
            // malformed encodings rather than reporting a fabricated type.
            if matches!(bytes.get(*at), Some(b'v' | b'R' | b'O')) { return None }
            let ty = cpp_type(bytes, at, depth + 1)?;
            if ty.split_whitespace().last() == Some("void") { return None }
            format!("{ty} {}", if code == b'R' { "&" } else { "&&" })
        },
        b'K' | b'V' | b'r' => {
            let qualifier = match code { b'K' => "const", b'V' => "volatile", _ => "restrict" };
            // Skip other qualifiers to identify the type they all apply to.
            let mut underlying = *at;
            while matches!(bytes.get(underlying), Some(b'K' | b'V' | b'r')) { underlying += 1; }
            let kind = *bytes.get(underlying)?;
            if matches!(kind, b'R' | b'O') { return None }
            let ty = cpp_type(bytes, at, depth + 1)?;
            if kind == b'P' { format!("{ty} {qualifier}") } else { format!("{qualifier} {ty}") }
        },
        b'N' => {let mut names=Vec::new();while bytes.get(*at)!=Some(&b'E'){names.push(cpp_component(bytes,at)?);if names.len()>16{return None}}*at+=1;names.join("::")},
        digit if digit.is_ascii_digit() => {*at-=1;cpp_component(bytes,at)?},
        _ => return None,
    })
}

fn cpp_component(bytes: &[u8], at: &mut usize) -> Option<String> {
    let start = *at;
    while bytes.get(*at).is_some_and(u8::is_ascii_digit) { *at += 1 }
    if start == *at { return None }
    let len = std::str::from_utf8(&bytes[start..*at]).ok()?.parse::<usize>().ok()?;
    if len == 0 || len > 256 || *at + len > bytes.len() { return None }
    let value = std::str::from_utf8(&bytes[*at..*at+len]).ok()?; *at += len;
    if !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { return None }
    Some(value.into())
}

fn cpp_operator(bytes: &[u8], at: &mut usize) -> Option<&'static str> {
    let spelling = match bytes.get(*at..*at + 2)? {
        b"nw" => "operator new", b"na" => "operator new[]",
        b"dl" => "operator delete", b"da" => "operator delete[]",
        b"ps" | b"pl" => "operator+", b"ng" | b"mi" => "operator-",
        b"ad" | b"an" => "operator&", b"de" | b"ml" => "operator*",
        b"co" => "operator~", b"nt" => "operator!", b"dv" => "operator/",
        b"rm" => "operator%", b"or" => "operator|", b"eo" => "operator^",
        b"aS" => "operator=", b"pL" => "operator+=", b"mI" => "operator-=",
        b"mL" => "operator*=", b"dV" => "operator/=", b"rM" => "operator%=",
        b"aN" => "operator&=", b"oR" => "operator|=", b"eO" => "operator^=",
        b"ls" => "operator<<", b"rs" => "operator>>", b"lS" => "operator<<=", b"rS" => "operator>>=",
        b"eq" => "operator==", b"ne" => "operator!=", b"lt" => "operator<", b"gt" => "operator>",
        b"le" => "operator<=", b"ge" => "operator>=", b"ss" => "operator<=>",
        b"aa" => "operator&&", b"oo" => "operator||", b"pp" => "operator++", b"mm" => "operator--",
        b"cm" => "operator,", b"pm" => "operator->*", b"pt" => "operator->", b"ix" => "operator[]",
        _ => return None,
    };
    *at += 2;
    Some(spelling)
}

pub fn symbol(name: &str) -> Option<Signature> {
    if !clean(name) { return None }
    if let Some((declaration, language)) = api(name) {
        return Some(Signature { language, declaration: declaration.into(), source: "exact SDK ABI catalog" });
    }
    let mangled = name.strip_prefix("__Z").or_else(|| name.strip_prefix("_Z"))?;
    let bytes = mangled.as_bytes(); let mut at = 0; let mut names: Vec<String> = Vec::new();
    let mut member_qualifiers = Vec::new();
    let mut ref_qualifier = None;
    let mut destructor = false;
    if bytes.first() == Some(&b'N') {
        at += 1;
        // Itanium encodes member-function cv-qualifiers in r, V, K order,
        // followed by an optional ref-qualifier, before the nested name.
        for (code, spelling) in [(b'r', "restrict"), (b'V', "volatile"), (b'K', "const")] {
            if bytes.get(at) == Some(&code) { member_qualifiers.push(spelling); at += 1; }
        }
        if let Some(code @ (b'R' | b'O')) = bytes.get(at).copied() {
            ref_qualifier = Some(if code == b'R' { "&" } else { "&&" });
            at += 1;
        }
        while bytes.get(at) != Some(&b'E') {
            if let Some(operator) = cpp_operator(bytes, &mut at) {
                if names.is_empty() || bytes.get(at) != Some(&b'E') { return None }
                names.push(operator.into());
                break;
            }
            if matches!((bytes.get(at), bytes.get(at + 1)),
                (Some(b'C'), Some(b'1' | b'2' | b'3')) | (Some(b'D'), Some(b'0' | b'1' | b'2'))) {
                if names.is_empty() || !member_qualifiers.is_empty() || ref_qualifier.is_some() { return None }
                destructor = bytes[at] == b'D';
                let class = names.last()?.clone();
                names.push(if destructor { format!("~{class}") } else { class });
                at += 2;
                // Constructor/destructor names are terminal components.
                if bytes.get(at) != Some(&b'E') { return None }
                break;
            }
            names.push(cpp_component(bytes, &mut at)?);
            if names.len() > 16 { return None }
        }
        at += 1;
        if names.len() < 2 { return None }
    } else if let Some(operator) = cpp_operator(bytes, &mut at) { names.push(operator.into()); }
    else { names.push(cpp_component(bytes, &mut at)?); }
    // A function encoding must contain a bare-function-type. A name alone
    // can instead designate data and must not become a no-argument function.
    if at == bytes.len() { return None }
    if destructor && &bytes[at..] != b"v" { return None }
    let mut args = Vec::new();
    if bytes.get(at) == Some(&b'v') && at + 1 == bytes.len() { at += 1; }
    else { while at < bytes.len() {
        if bytes[at] == b'z' {
            if at + 1 != bytes.len() { return None }
            args.push("...".into()); at += 1; break;
        }
        let arg = cpp_type(bytes, &mut at, 0)?; if arg.split_whitespace().last() == Some("void") { return None } args.push(arg); if args.len() > 32 { return None }
    } }
    if at != bytes.len() { return None }
    let mut qualifiers = member_qualifiers.join(" ");
    if let Some(reference) = ref_qualifier {
        if !qualifiers.is_empty() { qualifiers.push(' '); }
        qualifiers.push_str(reference);
    }
    if !qualifiers.is_empty() { qualifiers.insert(0, ' '); }
    let declaration = format!("/* return type unresolved */ {}({}){};", names.join("::"), args.iter().enumerate().map(|(i,t)| if t == "..." { t.clone() } else { format!("{t} arg{i}") }).collect::<Vec<_>>().join(", "), qualifiers);
    Some(Signature { language: "C++ / Objective-C++", declaration, source: "Itanium mangled parameter types; return type absent" })
}

pub fn recover(starts: &[u64], symbols: &[(u64, String)], objc_records: &str, calls: &[(u64,u64,u64)], scan_end: u64) -> (String, String) {
    recover_internal(starts, symbols, objc_records, calls, scan_end, &BTreeMap::new(), None)
}

struct Arm64RenderContext<'a> {
    code: &'a [u8],
    base: u64,
    file_offset: u64,
    data_ranges: &'a [(u64,u64,u16)],
    parameter_evidence: &'a str,
    tail_calls: &'a [(u64,u64,u64)],
    jump_tables: &'a str,
    objc_selectors: &'a BTreeMap<u64,String>,
    objc_layouts: &'a [super::objc::ClassLayout],
}

#[allow(clippy::too_many_arguments)]
pub fn recover_arm64(
    starts: &[u64], symbols: &[(u64, String)], objc_records: &str,
    calls: &[(u64,u64,u64)], scan_end: u64, code: &[u8], base: u64,
    file_offset: u64, data_ranges: &[(u64,u64,u16)], parameter_evidence: &str,
    tail_calls: &[(u64,u64,u64)], jump_tables: &str, objc_selectors: &BTreeMap<u64,String>, external_prototypes:&BTreeMap<String,Signature>, objc_layouts:&[super::objc::ClassLayout],
) -> (String, String) {
    recover_internal(starts, symbols, objc_records, calls, scan_end, external_prototypes,
        Some(Arm64RenderContext { code, base, file_offset, data_ranges, parameter_evidence, tail_calls, jump_tables, objc_selectors, objc_layouts }))
}

#[derive(Default)]
struct SwitchEvidence {
    branch_register: u8,
    index_register: Option<u8>,
    range: Option<String>,
    normalization: Option<String>,
    cases: Vec<(u64,Option<u64>,u64)>,
}

fn parse_switch_evidence(report: &str) -> BTreeMap<u64,SwitchEvidence> {
    let mut result=BTreeMap::new();
    let mut current=None;
    for line in report.lines() {
        if line.contains(" BR x") && line.contains("bounded indexed-table evidence") {
            let mut fields=line.split_whitespace();
            let address=fields.next().and_then(|s|u64::from_str_radix(s.trim_start_matches("0x"),16).ok());
            let register=fields.clone().collect::<Vec<_>>().windows(2).find_map(|pair|{
                (pair[0]=="BR").then(||pair[1].strip_prefix('x').and_then(|n|n.parse::<u8>().ok())).flatten()
            });
            current=address.zip(register).map(|(address,register)|{
                result.insert(address,SwitchEvidence{branch_register:register,..SwitchEvidence::default()});address
            });
            continue;
        }
        let Some(address)=current else{continue};
        if let Some(register)=line.split("table_index=x").nth(1).and_then(|s|s.split(|c:char|!c.is_ascii_digit()).next()).and_then(|s|s.parse::<u8>().ok()) {
            if let Some(evidence)=result.get_mut(&address){
                evidence.index_register=Some(register);
                if let Some(range)=line.split("case-range=").nth(1).and_then(|s|s.split(" proven-entry-count=").next()) {
                    evidence.range=Some(range.to_owned());
                }
                if let Some(normalization)=line.split("index-normalization=").nth(1).and_then(|s|s.split(" modulo ").next()) {
                    evidence.normalization=Some(normalization.to_owned());
                }
            }
        }
        if line.trim_start().starts_with("case-index=") {
            let case=line.split("case-index=").nth(1).and_then(|s|s.split_whitespace().next()).and_then(|s|s.parse::<u64>().ok());
            let target=line.split("target=").nth(1).and_then(|s|s.split_whitespace().next()).and_then(|s|u64::from_str_radix(s.trim_start_matches("0x"),16).ok());
            let selector=line.split("selector-bits=").nth(1).and_then(|s|s.split_whitespace().next()).and_then(|s|u64::from_str_radix(s.trim_start_matches("0x"),16).ok());
            if let (Some(case),Some(target))=(case,target){if let Some(evidence)=result.get_mut(&address){evidence.cases.push((case,selector,target));}}
        }
        if line.starts_with("validated_entries=") { current=None; }
    }
    result.retain(|_,evidence|evidence.index_register.is_some()&&!evidence.cases.is_empty());
    result
}

fn callable_name(name: &str, signature: Option<&Signature>) -> String {
    if signature.is_some_and(|sig| sig.language == "Objective-C / Objective-C++") {
        let safe = name.chars().map(|c| if matches!(c, '[' | ']' | ' ' | ':' | '+' | '-') { '_' } else { c }).collect::<String>();
        return format!("objc_imp_{}", safe.trim_matches('_'));
    }
    if let Some(sig) = signature {
        if sig.language == "C++ / Objective-C++" {
            if let Some(head) = sig.declaration.strip_prefix("/* return type unresolved */ ").and_then(|decl| decl.split_once('(').map(|(head, _)| head)) {
                if !head.is_empty() && head.bytes().all(|b| b.is_ascii_alphanumeric() || b"_ :~+-*/%&|^=!<>[],".contains(&b)) {
                    return head.into();
                }
            }
        }
        if let Some((head, _)) = sig.declaration.split_once('(') {
            let candidate = head.trim().trim_end_matches('*').split_whitespace().last().unwrap_or("");
            if !candidate.is_empty() && candidate.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b':') {
                return candidate.into();
            }
        }
    }
    let candidate = name.strip_prefix('_').unwrap_or(name);
    if candidate.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b':') && !candidate.is_empty() {
        candidate.into()
    } else {
        format!("sub_{}", name.bytes().fold(String::new(), |mut out, byte| { use std::fmt::Write; let _ = write!(out, "{byte:02x}"); out }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AbiParameter { name: String, register: String, general_register: Option<u8>, stack_scalar: Option<(usize, u8)>, source_type: Option<String> }

fn split_declarators(arguments: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (index, byte) in arguments.bytes().enumerate() {
        match byte { b'(' | b'[' | b'<' => depth += 1, b')' | b']' | b'>' => depth -= 1,
            b',' if depth == 0 => { result.push(arguments[start..index].trim()); start = index + 1; }, _ => {} }
    }
    let last = arguments[start..].trim();
    if !last.is_empty() { result.push(last); }
    result
}

fn named_declarator(value: &str, fallback: usize) -> (String, String) {
    // Only a single scalar-array dimension has the flat pointer ABI modeled
    // here. Never discard row extents, another pointer level, VLA effects or
    // trailing syntax and then present the result as an exact declaration.
    if let Some(open) = value.find('[') {
        let head = value[..open].trim_end();
        let bound = value[open + 1..].strip_suffix(']').map(str::trim);
        let supported = bound.is_some_and(|bound| bound.is_empty() ||
            (!bound.starts_with('0') && bound.bytes().all(|b| b.is_ascii_digit()) && bound.parse::<u64>().is_ok()));
        if let Some(split) = head.rfind(char::is_whitespace).filter(|_| supported) {
            let name = head[split..].trim();
            if c_identifier(name) {
                return (format!("{} *", head[..split].trim()), name.into());
            }
        }
        return (value.into(), format!("arg{fallback}"));
    }
    for marker in ["(*", "(^", "(&"] {
        if let Some(open) = value.find(marker) {
            let start = open + marker.len();
            if let Some(end) = value[start..].find(')') {
                let name = value[start..start + end].trim();
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    return (format!("{}{}", &value[..start], &value[start + end..]), name.into());
                }
            }
        }
    }
    let token = value.split_whitespace().last().unwrap_or("").trim_start_matches('*');
    let token = token.trim_end_matches("[]");
    if !token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        let name_start = value.rfind(token).unwrap_or(0);
        if name_start > 0 { (value[..name_start].trim().into(), token.into()) }
        else { (value.into(), format!("arg{fallback}")) }
    } else { (value.into(), format!("arg{fallback}")) }
}

fn c_parameters(signature: &Signature) -> Vec<(String, String)> {
    let Some(open) = signature.declaration.find('(') else { return Vec::new() };
    let mut depth = 0i32;
    let mut close = None;
    for (offset, ch) in signature.declaration[open..].char_indices() {
        if ch == '(' { depth += 1; }
        if ch == ')' { depth -= 1; if depth == 0 { close = Some(open + offset); break; } }
    }
    let Some(close) = close else { return Vec::new() };
    let body = signature.declaration[open + 1..close].trim();
    if body.is_empty() || body == "void" { return Vec::new(); }
    split_declarators(body).into_iter().enumerate().filter_map(|(index, declaration)| {
        if declaration == "..." { return None; }
        let (ty, name) = named_declarator(declaration, index);
        Some((ty, name))
    }).collect()
}

fn objc_parameters(signature: &Signature) -> Vec<(String, String)> {
    let Some((_, selector)) = signature.declaration.split_once(')') else { return Vec::new() };
    let mut result = Vec::new();
    let mut rest = selector;
    while let Some(colon) = rest.find(':') {
        let after_colon = rest[colon + 1..].trim_start();
        let Some(after_open) = after_colon.strip_prefix('(') else { break };
        let Some(type_end) = after_open.find(')') else { break };
        let ty = after_open[..type_end].trim();
        let after_type = after_open[type_end + 1..].trim_start();
        let name = after_type.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_').next().unwrap_or("");
        if name.is_empty() || ty.is_empty() { break; }
        result.push((ty.into(), name.into()));
        rest = after_type.get(name.len()..).unwrap_or("");
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AbiType { Integer(u8), WideInteger, Float(u8), IntegerAggregate(u8), FloatAggregate(u8), HiddenResultPointer(u8) }

/// Apple arm64 LP64 scalar ABI. Unknown typedefs/aggregates do not receive
/// guessed locations: their layout can change both banks and the stack cursor.
fn uncomment_type(ty: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut rest = ty;
    while let Some((head, tail)) = rest.split_once("/*") {
        if head.contains("*/") { return None; }
        normalized.push_str(head);
        // In C a comment is replaced by whitespace during translation.
        // Deleting it outright can join distinct tokens into a different type.
        normalized.push(' ');
        rest = tail.split_once("*/")?.1;
    }
    if rest.contains("*/") { return None; }
    normalized.push_str(rest);
    Some(normalized)
}

fn abi_type(ty: &str) -> Option<AbiType> {
    let normalized = uncomment_type(ty)?;
    let ty = normalized.trim();
    if ty.contains('{') || ty.contains('}') || ty.contains('[') { return None; }
    if ty.contains('*') || ty.contains('&') || ty.contains("(^") || ty.starts_with("id<")&&ty.ends_with('>') { return Some(AbiType::Integer(8)); }
    let ty = ty.split_whitespace().filter(|word| !matches!(*word,
        "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Nonnull" | "_Nullable" | "__kindof"
    )).collect::<Vec<_>>().join(" ");
    Some(match ty.as_str() {
        "CFRange" | "NSRange" => AbiType::IntegerAggregate(2),
        "CGPoint" | "CGSize" => AbiType::FloatAggregate(2), "CGRect" => AbiType::FloatAggregate(4),
        // Six doubles: 48 bytes, and not a homogeneous aggregate of at most
        // four members, so it is passed and returned indirectly.
        "CGAffineTransform" => AbiType::FloatAggregate(6),
        "__int128" | "unsigned __int128" => AbiType::WideInteger,
        "float" => AbiType::Float(4), "double" | "long double" | "CGFloat" => AbiType::Float(8),
        "bool" | "_Bool" | "BOOL" | "Boolean" | "char" | "char8_t" | "signed char" | "unsigned char" | "int8_t" | "uint8_t" | "Int8" | "UInt8" => AbiType::Integer(1),
        "short" | "short int" | "signed short" | "signed short int" | "unsigned short" | "unsigned short int" | "int16_t" | "uint16_t" | "Int16" | "UInt16" | "unichar" | "char16_t" => AbiType::Integer(2),
        "int" | "signed" | "signed int" | "unsigned" | "unsigned int" | "int32_t" | "uint32_t" | "Int32" | "UInt32" | "CFStringEncoding" | "wchar_t" | "char32_t" => AbiType::Integer(4),
        "long" | "long int" | "signed long" | "signed long int" | "unsigned long" | "unsigned long int" | "long long" | "long long int" | "signed long long" | "signed long long int" | "unsigned long long" | "unsigned long long int" |
        "int64_t" | "uint64_t" | "Int64" | "UInt64" | "intptr_t" | "uintptr_t" | "size_t" | "ssize_t" | "ptrdiff_t" | "NSInteger" | "NSUInteger" |
        "CFIndex" | "CFTypeID" | "CFOptionFlags" | "CFNumberType" | "CFURLPathStyle" | "CFPropertyListFormat" | "dispatch_once_t" => AbiType::Integer(8),
        "id" | "Class" | "SEL" | "instancetype" | "dispatch_queue_t" | "dispatch_block_t" |
        "CFTypeRef" | "CFAllocatorRef" | "CFStringRef" | "CFArrayRef" | "CFDictionaryRef" | "CFBooleanRef" | "CFNumberRef" | "CFDataRef" |
        "CFBundleRef" | "CFURLRef" | "CFPropertyListRef" | "CFErrorRef" | "CFRunLoopRef" => AbiType::Integer(8),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AbiIntegerKind { Pointer, Boolean, Signed, Unsigned }

fn abi_integer_kind(ty: &str) -> Option<AbiIntegerKind> {
    if !matches!(abi_type(ty), Some(AbiType::Integer(_))) { return None; }
    let normalized = uncomment_type(ty)?;
    let simple = normalized.split_whitespace().filter(|word| !matches!(*word,
        "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Nonnull" | "_Nullable" | "__kindof"
    )).collect::<Vec<_>>().join(" ");
    let pointer = normalized.contains('*') || normalized.contains('&') || normalized.contains("(^") || simple.starts_with("id<")&&simple.ends_with('>') || matches!(simple.as_str(),
        "id" | "Class" | "SEL" | "instancetype" | "dispatch_queue_t" | "dispatch_block_t" |
        "CFTypeRef" | "CFAllocatorRef" | "CFStringRef" | "CFArrayRef" | "CFDictionaryRef" | "CFBooleanRef" | "CFNumberRef" | "CFDataRef" |
        "CFBundleRef" | "CFURLRef" | "CFPropertyListRef" | "CFErrorRef" | "CFRunLoopRef");
    if pointer { return Some(AbiIntegerKind::Pointer); }
    if matches!(simple.as_str(), "bool" | "_Bool" | "BOOL") { return Some(AbiIntegerKind::Boolean); }
    let signed = matches!(simple.as_str(), "char" | "wchar_t" | "signed char" | "short" | "short int" | "signed short" | "signed short int" |
        "int" | "signed" | "signed int" | "long" | "long int" | "signed long" | "signed long int" | "long long" | "long long int" | "signed long long" | "signed long long int" |
        "int8_t" | "int16_t" | "int32_t" | "int64_t" | "Int8" | "Int16" | "Int32" | "Int64" |
        "intptr_t" | "ssize_t" | "ptrdiff_t" | "NSInteger" | "CFIndex" | "dispatch_once_t" |
        "CFNumberType" | "CFURLPathStyle" | "CFPropertyListFormat");
    Some(if signed { AbiIntegerKind::Signed } else { AbiIntegerKind::Unsigned })
}

fn abi_types_agree(expected: &str, actual: &str) -> bool {
    if expected == "void" || actual == "void" { return expected == actual; }
    let (Some(left), Some(right)) = (abi_type(expected), abi_type(actual)) else { return false; };
    if left != right { return false; }
    if matches!(left, AbiType::IntegerAggregate(_) | AbiType::FloatAggregate(_)) { return expected == actual; }
    if matches!(left, AbiType::WideInteger) { return expected == actual; }
    if matches!(left, AbiType::Integer(_)) {
        // Recover the interpretation class independently of width. Pointer,
        // boolean, signed and unsigned encodings must not silently coerce.
        return abi_integer_kind(expected).zip(abi_integer_kind(actual)).is_some_and(|(left,right)|left==right);
    }
    true
}

fn abi_parameters(signature: Option<&Signature>) -> Vec<AbiParameter> {
    let Some(signature) = signature else { return Vec::new() };

    let is_objc = signature.language == "Objective-C / Objective-C++";
    let parameters = if is_objc { objc_parameters(signature) } else { c_parameters(signature) };
    
    let mut result = Vec::new();
    
    // Add Objective-C implicit parameters. A result that comes back indirectly
    // does not change this: the indirect result location is the dedicated x8
    // register, so it never shifts the argument registers.
    if is_objc {
        result.push(AbiParameter { name: "self".into(), register: "x0".into(), general_register: Some(0), stack_scalar: None, source_type: Some("id".into()) });
        result.push(AbiParameter { name: "_cmd".into(), register: "x1".into(), general_register: Some(1), stack_scalar: None, source_type: Some("SEL".into()) });
    }
    
    // The first general-purpose argument register: x2 after the implicit
    // Objective-C self and _cmd, x0 otherwise.
    let mut gp = if is_objc { 2u8 } else { 0u8 };
    let mut fp = 0u8;
    let mut stack_slot = 0usize;
    let mut stack_offset = 0usize;
    let mut blocked = signature.language == "C++ / Objective-C++" && signature.declaration.contains("::");
    
    for (ty, name) in parameters {
        let scalar = abi_type(&ty);
        if blocked || scalar.is_none() {
            let reason = if blocked { "earlier layout or implicit C++ receiver unresolved" } else { "type layout unresolved" };
            result.push(AbiParameter { name, register: format!("unresolved_argument /* {reason}: {ty} */"), general_register: None, stack_scalar: None, source_type: Some(ty) });
            blocked = true;
            continue;
        }
        let scalar = scalar.unwrap_or(AbiType::Integer(8));
        if scalar == AbiType::WideInteger {
            if gp <= 6 {
                result.push(AbiParameter { name, register: format!("wide_integer<{ty}>(x{gp}, x{})", gp + 1), general_register: None, stack_scalar: None, source_type: Some(ty) });
                gp += 2;
            } else {
                gp = 8;
                stack_offset = (stack_offset + 15) & !15;
                result.push(AbiParameter { name, register: format!("stack argument (slot {stack_slot}, sp+0x{stack_offset:x}, 16 bytes; Apple arm64 wide integer {ty})"), general_register: None, stack_scalar: Some((stack_offset, 16)), source_type: Some(ty) });
                stack_offset += 16;
                stack_slot += 1;
            }
            continue;
        }
        if let AbiType::IntegerAggregate(words) | AbiType::FloatAggregate(words) = scalar {
            // A homogeneous floating-point aggregate of at most four members
            // travels in the floating-point registers, whatever its size. Every
            // other composite larger than 16 bytes is passed by pointer: the
            // caller copies it and passes its address in the next
            // general-purpose register.
            let hfa = matches!(scalar, AbiType::FloatAggregate(words) if words <= 4);
            let bytes = words as usize * 8;
            if !hfa && bytes > 16 {
                if gp < 8 {
                    result.push(AbiParameter { name, register: format!("by-pointer aggregate<{ty}>({bytes} bytes) in x{gp}; the caller copies the composite and passes its address"), general_register: Some(gp), stack_scalar: None, source_type: Some(ty) });
                    gp += 1;
                    continue;
                }
                stack_offset = (stack_offset + 7) & !7;
                result.push(AbiParameter { name, register: format!("stack argument (slot {stack_slot}, sp+0x{stack_offset:x}, 8-byte pointer to {bytes}-byte Apple arm64 composite {ty})"), general_register: None, stack_scalar: Some((stack_offset, 8)), source_type: Some(ty) });
                stack_offset += 8;
                stack_slot += 1;
                continue;
            }
            let (bank, prefix) = if hfa { (&mut fp, "d") } else { (&mut gp, "x") };
            if *bank + words <= 8 {
                let registers = (*bank..*bank + words).map(|index| format!("{prefix}{index}")).collect::<Vec<_>>().join(", ");
                result.push(AbiParameter { name, register: format!("aggregate<{ty}>({registers})"), general_register: None, stack_scalar: None, source_type: Some(ty) });
                *bank += words;
            } else {
                // Darwin does not split these SDK aggregates between registers
                // and memory. A failed allocation exhausts the relevant bank.
                *bank = 8;
                stack_offset = (stack_offset + 7) & !7;
                let bytes = words as usize * 8;
                result.push(AbiParameter { name, register: format!("stack argument (slot {stack_slot}, sp+0x{stack_offset:x}, {bytes} bytes; Apple arm64 SDK aggregate {ty})"), general_register: None, stack_scalar: Some((stack_offset, bytes as u8)), source_type: Some(ty) });
                stack_offset += bytes;
                stack_slot += 1;
            }
            continue;
        }
        if let AbiType::Float(bytes) = scalar {
            if fp < 8 {
            let register = format!("{}{fp}", if bytes == 4 { "s" } else { "d" });
            result.push(AbiParameter { name, register, general_register: None, stack_scalar: None, source_type: Some(ty) });
            fp += 1;
            continue;
            }
        } else if gp < 8 {
            result.push(AbiParameter { name, register: format!("x{gp}"), general_register: Some(gp), stack_scalar: None, source_type: Some(ty) });
            gp += 1;
            continue;
        }
        let bytes = match scalar { AbiType::Integer(bytes) | AbiType::Float(bytes) => bytes as usize, _ => continue };
        stack_offset = (stack_offset + bytes - 1) & !(bytes - 1);
        result.push(AbiParameter { name, register: format!("stack argument (slot {stack_slot}, sp+0x{stack_offset:x}, {bytes} bytes; Apple arm64)"), general_register: None, stack_scalar: Some((stack_offset, bytes as u8)), source_type: Some(ty) });
        stack_offset += bytes;
        stack_slot += 1;
    }
    result
}

// Render indirect composites as values loaded through the ABI pointer, and
// wide stack values as two-word objects. A location description alone is not
// the source-language argument value.
fn render_abi_argument_location(argument: &AbiParameter) -> String {
    if let (Some((offset, 16)), Some(ty)) = (argument.stack_scalar, argument.source_type.as_deref()) {
        if abi_type(ty) == Some(AbiType::WideInteger) {
            return format!("wide_integer_from_stack<{ty}>(sp + {offset:#x})");
        }
    }
    if let (Some((offset, 8)), Some(ty)) = (argument.stack_scalar, argument.source_type.as_deref()) {
        if matches!(abi_type(ty), Some(AbiType::IntegerAggregate(3..) | AbiType::FloatAggregate(5..))) {
            return format!("aggregate_from_pointer<{ty}>(load64(sp + {offset:#x}))");
        }
    }
    if let (Some((offset, width)), Some(ty)) = (argument.stack_scalar, argument.source_type.as_deref()) {
        if matches!(abi_type(ty), Some(AbiType::IntegerAggregate(_) | AbiType::FloatAggregate(_))) {
            return format!("aggregate_from_stack<{ty}>(sp + {offset:#x}, {width})");
        }
    }
    if let (Some(register), Some(ty)) = (argument.general_register, argument.source_type.as_deref()) {
        if matches!(abi_type(ty), Some(AbiType::IntegerAggregate(_) | AbiType::FloatAggregate(_))) {
            return format!("aggregate_from_pointer<{ty}>(x{register})");
        }
    }
    if let (Some((offset, width)), Some(ty)) = (argument.stack_scalar, argument.source_type.as_deref()) {
        if matches!(abi_type(ty), Some(AbiType::Integer(_) | AbiType::Float(_))) {
            return format!("load_stack_argument<{ty}>(sp + {offset:#x}, {width})");
        }
    }
    argument.register.clone()
}

fn signature_return_type(signature: &Signature) -> Option<String> {
    if signature.declaration.starts_with("/*") { return None; }
    if signature.language == "Objective-C / Objective-C++" {
        return Some(signature.declaration.split_once('(')?.1.split_once(')')?.0.trim().into());
    }
    let head = signature.declaration.split_once('(')?.0.trim();
    let name = head.split_whitespace().last()?.trim_start_matches('*');
    Some(head.strip_suffix(name)?.trim().into())
}

/// Interpret low ABI bits, not a C arithmetic conversion of the full X view.
/// Casts describe source-type evidence; they do not assert pointer validity or
/// invent aggregate/enum layouts that the prototype cannot establish.
fn typed_gp_argument(value: u64, ty: Option<&str>) -> (String, Option<String>) {
    let raw = format!("{value:#x}");
    let Some(ty) = ty else { return (raw, None); };
    let Some(AbiType::Integer(bytes)) = abi_type(ty) else { return (raw, None); };
    let Some(kind) = abi_integer_kind(ty) else { return (raw, None); };
    if kind == AbiIntegerKind::Pointer {
        let rendered = if value == 0 { format!("({ty})0") } else { format!("({ty}){raw}") };
        return (rendered, Some(format!("source-cast=pointer<{ty}>; raw-register={raw}; pointee validity unresolved")));
    }
    let width = u32::from(bytes) * 8;
    let low = if width == 64 { value } else { value & ((1u64 << width) - 1) };
    if kind == AbiIntegerKind::Boolean {
        if low <= 1 {
            return (format!("({ty}){}", if low == 0 { "false" } else { "true" }),
                Some(format!("source-cast=boolean<{ty}>; low-bits={width}; raw-register={raw}")));
        }
        // Invalid ABI bool representations must not silently become `true`.
        return (format!("bitcast<{ty}>({low:#x})"), Some(format!("source-cast=boolean<{ty}>; noncanonical representation; raw-register={raw}")));
    }
    let signed = kind == AbiIntegerKind::Signed;
    let literal = if signed {
        let shift = 64 - width;
        let signed_value = ((low << shift) as i64) >> shift;
        if signed_value < 0 { signed_value.to_string() } else { format!("{low:#x}") }
    } else { format!("{low:#x}") };
    (format!("({ty}){literal}"), Some(format!("source-cast={}<{ty}>; low-bits={width}; raw-register={raw}", if signed { "signed" } else { "unsigned" })))
}

fn signature_returns_void(signature: Option<&Signature>) -> bool {
    signature.and_then(signature_return_type).as_deref() == Some("void")
}

fn scalar_call_result(signature: Option<&Signature>) -> Option<&'static str> {
    let ty = signature.and_then(signature_return_type)?;
    match abi_result_type(&ty)? {
        AbiType::Integer(bytes) => Some(if bytes <= 4 { "w0" } else { "x0" }),
        AbiType::Float(4) => Some("s0"),
        AbiType::Float(8) => Some("d0"),
        _ => None,
    }
}

fn call_result_destination(signature: Option<&Signature>) -> Option<String> {
    if let Some(register) = scalar_call_result(signature) { return Some(register.into()); }
    let ty = signature.and_then(signature_return_type)?;
    let (prefix, count) = match abi_result_type(&ty)? {
        AbiType::WideInteger => ("x", 2),
        AbiType::IntegerAggregate(words) => ("x", words),
        AbiType::FloatAggregate(words) => ("d", words),
        _ => return None,
    };
    Some(format!("({})", (0..count).map(|i|format!("{prefix}{i}")).collect::<Vec<_>>().join(", ")))
}

fn indirect_call_result_argument(signature: Option<&Signature>) -> Option<String> {
    let ty = signature.and_then(signature_return_type)?;
    matches!(abi_result_type(&ty), Some(AbiType::HiddenResultPointer(_)))
        .then(||format!("/* hidden result buffer */ result_buffer<{ty}>(x8)"))
}

/// The Apple arm64 result ABI of a declared return type.
///
/// A homogeneous floating-point aggregate of at most four members is returned
/// in the floating-point registers, whatever its size: `CGRect` is 32 bytes
/// and comes back in d0-d3. Every other composite larger than 16 bytes is
/// returned indirectly: the caller passes the result location in x8. No
/// pointer return in x0 is assumed. The result location is a dedicated
/// register, so it never shifts the argument registers.
fn abi_result_type(ty: &str) -> Option<AbiType> {
    match abi_type(ty) {
        Some(AbiType::IntegerAggregate(words)) if words > 2 => Some(AbiType::HiddenResultPointer(words)),
        Some(AbiType::FloatAggregate(words)) if words > 4 => Some(AbiType::HiddenResultPointer(words)),
        other => other,
    }
}

fn render_signature_return(signature: Option<&Signature>) -> String {
    let Some(ty) = signature.and_then(signature_return_type) else {
        return "return x0; /* inferred ABI result; source return type unresolved */".into();
    };
    if ty == "void" { return "return; /* void return supported by recovered signature */".into(); }
    let register = match abi_result_type(&ty) {
        Some(AbiType::Float(4)) => "s0", Some(AbiType::Float(_)) => "d0",
        Some(AbiType::Integer(bytes)) if bytes <= 4 => "w0", Some(AbiType::Integer(_)) => "x0",
        Some(AbiType::WideInteger) => return format!("return wide_integer<{ty}>(x0, x1); /* Apple arm64 result */"),
        Some(AbiType::IntegerAggregate(words) | AbiType::FloatAggregate(words)) => {
            let prefix = if matches!(abi_type(&ty), Some(AbiType::FloatAggregate(_))) { "d" } else { "x" };
            let registers = (0..words).map(|index| format!("{prefix}{index}")).collect::<Vec<_>>().join(", ");
            return format!("return aggregate<{ty}>({registers}); /* Apple arm64 known SDK aggregate result */");
        }
        None => return format!("return /* result ABI unresolved for {ty}; aggregate/HFA/indirect-result layout required */;"),
        Some(AbiType::HiddenResultPointer(_)) => return format!("return /* sret result via hidden pointer in x8 for {ty} */;"),
    };
    format!("return {register}; /* Apple arm64 result for recovered type {ty} */")
}

fn abi_parameter_bindings(signature: Option<&Signature>, candidates: &[String]) -> Vec<String> {
    let mut bindings = abi_parameters(signature).into_iter().map(|parameter| {
        format!("{}={}", parameter.name, parameter.register)
    }).collect::<Vec<_>>();
    if bindings.is_empty() {
        bindings.extend(candidates.iter().enumerate().map(|(index, register)| format!("arg{index}={register} (candidate; type unresolved)")));
    }
    bindings
}

fn render_fp_argument(prefix: u8, bits: u128) -> String {
    if prefix == b's' {
        let raw = bits as u32;
        let value = f32::from_bits(raw);
        if value.is_finite() {
            if raw & 0x7fff_ffff == 0 { if raw >> 31 == 0 { "0.0".into() } else { "-0.0".into() } } else { let text=value.to_string(); if text.contains('.')||text.contains('e')||text.contains('E'){text}else{format!("{text}.0")} }
        } else {
            format!("bitcast<float>(0x{raw:08x})")
        }
    } else {
        let raw = bits as u64;
        let value = f64::from_bits(raw);
        if value.is_finite() {
            if raw & 0x7fff_ffff_ffff_ffff == 0 { if raw >> 63 == 0 { "0.0".into() } else { "-0.0".into() } } else { let text=value.to_string(); if text.contains('.')||text.contains('e')||text.contains('E'){text}else{format!("{text}.0")} }
        } else {
            format!("bitcast<double>(0x{raw:016x})")
        }
    }
}

fn incoming_parameter_candidates(report: &str, address: u64) -> Vec<String> {
    let prefix = format!("function {address:#018x} ");
    report.lines().find_map(|line| {
        let line = line.strip_prefix(&prefix)?;
        let registers = line.split("parameter-registers=[").nth(1)?.split(']').next()?;
        Some(registers.split(',').map(str::trim).filter(|r| !r.is_empty()).map(str::to_string).collect())
    }).unwrap_or_default()
}

fn candidate_abi_parameters(report: &str, address: u64) -> Vec<AbiParameter> {
    let candidates = incoming_parameter_candidates(report, address);
    let highest = candidates.iter().filter_map(|register| register.strip_prefix('x')?.parse::<u8>().ok()).max();
    highest.map(|highest| (0..=highest.min(7)).map(|index| AbiParameter {
        name: format!("arg{index}"), register: format!("x{index}"), general_register: Some(index), stack_scalar: None, source_type: None,
    }).collect()).unwrap_or_default()
}

/// Presentation-only substitution. Shared IR determines input/output identities;
/// token positions merely select those maps. Publish writes at each semicolon
/// so pre/post-writeback statements cannot reuse an old base name.
fn replace_instruction_register_names(statement: &str, names: &BTreeMap<String, String>, outputs: Option<&BTreeMap<String, String>>) -> String {
    let bytes = statement.as_bytes();
    let tuple_end = statement.strip_prefix('(').and_then(|body| body.split_once(") = "))
        .filter(|(head, _)| head.split(", ").all(|register| register.strip_prefix('x').or_else(||register.strip_prefix('d')).and_then(|n|n.parse::<u8>().ok()).is_some_and(|n|n<32)))
        .map(|(head, _)| head.len()+1);
    let mut names = names.clone();
    let mut written = Vec::<String>::new();
    let mut output = String::with_capacity(statement.len());
    let mut cursor = 0;
    let mut in_block_comment = false;
    while cursor < bytes.len() {
        // Evidence comments keep the raw machine registers: they document the
        // decoded instruction, not a source-level expression.
        if in_block_comment {
            let end = statement[cursor..].find("*/").map(|offset| cursor + offset + 2).unwrap_or(bytes.len());
            output.push_str(&statement[cursor..end]);
            cursor = end;
            in_block_comment = false;
            continue;
        }
        if bytes[cursor..].starts_with(b"/*") {
            in_block_comment = true;
            continue;
        }
        if bytes[cursor..].starts_with(b"//") {
            output.push_str(&statement[cursor..]);
            break;
        }
        if matches!(bytes[cursor], b'\"' | b'\'') {
            let start = cursor;
            let quote = bytes[cursor];
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == b'\\' { cursor = (cursor + 2).min(bytes.len()); }
                else if bytes[cursor] == quote { cursor += 1; break; }
                else { cursor += 1; }
            }
            output.push_str(&statement[start..cursor]);
        } else if bytes[cursor].is_ascii_digit() {
            // Consume the entire numeric token, including the x in 0x0 and
            // suffixes, before looking for register-shaped identifiers.
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'_' | b'.')) { cursor += 1; }
            output.push_str(&statement[start..cursor]);
        } else if bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_') { cursor += 1; }
            let token = &statement[start..cursor];
            let tail = statement[cursor..].trim_start();
            let prefix = statement[..start].trim_end();
            if tail.starts_with('(') || prefix.ends_with('.') || prefix.ends_with("->") {
                output.push_str(token); continue;
            }
            let definition = outputs.is_some() && (tuple_end.is_some_and(|end|start<end) || tail.starts_with('=') && !tail.starts_with("=="));
            if definition { written.push(token.to_owned()); }
            let map = if definition { outputs.unwrap() } else { &names };
            output.push_str(map.get(token).map(String::as_str).unwrap_or(token));
        } else {
            let ch = statement[cursor..].chars().next().unwrap();
            output.push(ch);
            cursor += ch.len_utf8();
            if ch == ';' {
                if let Some(outputs) = outputs {
                    for register in written.drain(..) {
                        let physical = register.get(1..).unwrap_or("");
                        let prefixes: &[char] = match register.as_bytes().first() {
                            Some(b'x' | b'w') => &['x', 'w'],
                            Some(b's' | b'd' | b'q' | b'v' | b'b' | b'h') => &['s', 'd', 'q', 'v', 'b', 'h'],
                            _ => &[],
                        };
                        for prefix in prefixes {
                            let key = format!("{prefix}{physical}");
                            if let Some(name) = outputs.get(&key) { names.insert(key, name.clone()); }
                            else { names.remove(&key); }
                        }
                    }
                }
            }
        }
    }
    output
}

/// Seed source identities from supported ABI locations and source widths.
/// Aggregates/stack arguments need their own field/object model, not fake registers.
fn parameter_name_seeds(signature: Option<&Signature>, report: &str, address: u64) -> Vec<super::semantic::ParameterNameSeed> {
    let parameters = if signature.is_some() { abi_parameters(signature) } else { candidate_abi_parameters(report, address) };
    parameters.into_iter().filter_map(|parameter| {
        if matches!(parameter.source_type.as_deref().and_then(abi_type), Some(AbiType::IntegerAggregate(_) | AbiType::FloatAggregate(_))) { return None; }
        if let Some(register) = parameter.general_register {
            let bits = match parameter.source_type.as_deref().and_then(abi_type) {
                Some(AbiType::Integer(bytes)) if bytes <= 4 => 32,
                Some(_) => 64,
                None => 0, // Infer an untyped candidate's view from entry-reaching IR uses.
            };
            Some(super::semantic::ParameterNameSeed { register, floating: false, bits, name: parameter.name })
        } else {
            let prefix = parameter.register.get(..1)?;
            let index = parameter.register.get(1..)?;
            let bits = match prefix { "s" => 32, "d" => 64, _ => return None };
            let register = index.parse::<u8>().ok()?;
            Some(super::semantic::ParameterNameSeed { register, floating: true, bits, name: parameter.name })
        }
    }).collect()
}

fn branch_target_statement(statement:&str)->Option<u64>{
    let target=statement.strip_prefix("goto 0x")
        .or_else(||statement.strip_prefix("if (").and_then(|rest|rest.split_once(") goto 0x").map(|(_,target)|target)))?;
    u64::from_str_radix(target.trim_end_matches(';'),16).ok()
}

fn is_linear_statement(statement:&str)->bool{
    !statement.starts_with("if (")
        && !statement.starts_with("goto 0x")
        && !statement.starts_with("return ")
}

fn stack_slot_name(offset:i64)->String{
    if offset<0{format!("stack_slot_sp_m{:x}",offset.unsigned_abs())}else{format!("stack_slot_sp_p{offset:x}")}
}

fn localize_stack_slot_accesses(statement:&str)->String{
    let mut output=String::with_capacity(statement.len());
    let mut cursor=0usize;
    while let Some(relative)=statement[cursor..].find("sp"){
        let sp=cursor+relative;
        let prior=statement[..sp].chars().next_back();
        let after=statement[sp+2..].chars().next();
        if prior.is_some_and(|ch|ch.is_ascii_alphanumeric()||ch=='_')||after.is_some_and(|ch|ch.is_ascii_alphanumeric()||ch=='_'){
            output.push_str(&statement[cursor..sp+2]);cursor=sp+2;continue;
        }
        let Some(access_start)=statement[..sp].rfind("*(") else{
            output.push_str(&statement[cursor..sp+2]);cursor=sp+2;continue;
        };
        let prefix=&statement[access_start+2..sp];
        if !prefix.is_empty()&&!prefix.ends_with(")("){
            output.push_str(&statement[cursor..sp+2]);cursor=sp+2;continue;
        }
        let Some(relative_end)=statement[sp..].find(')') else{
            output.push_str(&statement[cursor..]);return output;
        };
        let expression_end=sp+relative_end;
        let expression=&statement[sp..expression_end];
        let offset_text=expression.strip_prefix("sp").unwrap_or("");
        let offset=if offset_text.is_empty(){Some(0)}else if let Some(value)=offset_text.strip_prefix(" + 0x"){
            i64::from_str_radix(value,16).ok()
        }else if let Some(value)=offset_text.strip_prefix(" - 0x"){
            i64::from_str_radix(value,16).ok().and_then(i64::checked_neg)
        }else{None};
        let Some(offset)=offset else{
            output.push_str(&statement[cursor..sp+2]);cursor=sp+2;continue;
        };
        output.push_str(&statement[cursor..access_start]);
        let name=stack_slot_name(offset);
        if prefix.is_empty(){output.push_str(&name)}else{
            let access_type=prefix.strip_suffix(")(").unwrap_or(prefix).trim().trim_end_matches('*').trim();
            output.push_str(&format!("{name} /* stack access as {access_type}; type unresolved */"));
        }
        cursor=expression_end+1;
    }
    output.push_str(&statement[cursor..]);
    output
}

fn render_call_arguments(
    args: &[AbiParameter],
    known: &[(u8, u64, String)],
    known_float: &[(u8, u128, String)],
    known_stack: &[(usize, u8, u64, String)],
) -> (Vec<String>, Vec<String>) {
    let values=known.iter().map(|(register,value,provenance)|(*register,(*value,provenance.as_str()))).collect::<BTreeMap<_,_>>();
    let float_values=known_float.iter().map(|(register,value,provenance)|(*register,(*value,provenance.as_str()))).collect::<BTreeMap<_,_>>();
    let stack_values=known_stack.iter().map(|(offset,bytes,value,provenance)|(*offset,(*bytes,*value,provenance.as_str()))).collect::<BTreeMap<_,_>>();
    let mut evidence=Vec::new();
    let rendered=args.iter().map(|argument|{
        if let Some(register)=argument.general_register.filter(|_| !matches!(argument.source_type.as_deref().and_then(abi_type), Some(AbiType::IntegerAggregate(_) | AbiType::FloatAggregate(_)))) {
            if let Some((value,provenance))=values.get(&register){
                let (rendered, cast) = typed_gp_argument(*value, argument.source_type.as_deref());
                evidence.push(format!("{}({})={value:#x} {provenance}{}",argument.name,argument.register,
                    cast.map(|cast| format!(" [{cast}]")).unwrap_or_default()));
                return rendered;
            }
        }
        if let (Some((offset, width)), Some(ty))=(argument.stack_scalar,argument.source_type.as_deref()) {
            if let Some(AbiType::Float(bytes @ (4 | 8))) = abi_type(ty) {
                if let Some((_, value, provenance)) = stack_values.get(&offset).filter(|(stored_width, _, _)| *stored_width >= width) {
                    let rendered = render_fp_argument(if bytes == 4 { b's' } else { b'd' }, u128::from(*value));
                    evidence.push(format!("{}(sp+0x{offset:x}, {width} bytes)={rendered} {provenance}", argument.name));
                    return rendered;
                }
            }
            if matches!(abi_type(ty),Some(AbiType::Integer(_))) {
                if let Some((stored_width,value,provenance))=stack_values.get(&offset).filter(|(stored_width,_,_)|*stored_width>=width) {
                    let (rendered,cast)=typed_gp_argument(*value,Some(ty));
                    evidence.push(format!("{}(sp+0x{offset:x}, {width} bytes)={value:#x} {provenance}; store-width={stored_width}{}",
                        argument.name,cast.map(|cast|format!(" [{cast}]")).unwrap_or_default()));
                    return rendered;
                }
            }
        }
        if let Some((prefix, index)) = argument.register.as_bytes().split_first().and_then(|(prefix, rest)| std::str::from_utf8(rest).ok()?.parse::<u8>().ok().map(|index| (*prefix, index))) {
            if matches!(prefix, b's' | b'd') {
                if let Some((bits, provenance)) = float_values.get(&index) {
                    let value = render_fp_argument(prefix, *bits);
                    evidence.push(format!("{}({})={value} {provenance}", argument.name, argument.register));
                    return value;
                }
            }
        }
        render_abi_argument_location(argument)
    }).collect::<Vec<_>>();
    (rendered, evidence)
}

#[allow(clippy::too_many_arguments)]
fn render_linear_statement(
    statement:&str,address:u64,start:u64,
    symbols:&[(u64,String)],signatures:&BTreeMap<u64,(String,Signature)>,
    signature:Option<&Signature>,parameter_evidence:&str,code:&[u8],base:u64,stable_stack:bool,objc_selectors:&BTreeMap<u64,String>,
)->String{
    if let Some(rest)=statement.strip_prefix("call(0x"){
        if let Some((hex,_))=rest.split_once(')'){
            if let Ok(target)=u64::from_str_radix(hex,16){
                let indirect = statement.contains("indirect via");
                let target_name=signatures.get(&target).map(|(name,_)|name.as_str())
                    .or_else(||(!indirect).then(||symbols.iter().find(|(candidate,_)|*candidate==target).map(|(_,name)|name.as_str())).flatten());
                if let Some(name) = target_name {
                    let dispatch = name.strip_prefix('_').unwrap_or(name);
                    if matches!(dispatch, "objc_msgSendSuper" | "objc_msgSendSuper2") {
                        let target_evidence = if indirect { statement.split_once("/*").and_then(|(_, tail)| tail.split_once("*/")).map(|(evidence, _)| format!(" /*{}*/", evidence)).unwrap_or_default() } else { String::new() };
                        return format!("{dispatch}(x0, x1, /* message arguments unresolved */); /* {} {target:#x}; x0 points to objc_super; x1 is the selector; receiver, class, and argument types unresolved */{target_evidence}", if indirect { "indirect target resolved through a register" } else { "direct call target" });
                    }
                }
                let known=super::semantic::known_call_argument_values(code,base,start,address);
                let known_float=super::semantic::known_call_float_argument_values(code,base,start,address);
                let known_stack=super::semantic::known_call_stack_argument_values(code,base,start,address);
                if let Some(selector)=objc_selectors.get(&address){
                    let parts=selector.split(':').collect::<Vec<_>>();
                    let argument_count=parts.len().saturating_sub(1);
                    if objc_selector_syntax(selector) {
                        let values=known.iter().map(|(register,value,_)|(*register,*value)).collect::<BTreeMap<_,_>>();
                        let receiver=values.get(&0).map(|value|format!("{value:#x}")).unwrap_or_else(||"x0".into());
                        let mut message=format!("[{receiver}");
                        if argument_count==0{message.push(' ');message.push_str(selector);}
                        // Selector arity does not establish argument types or ABI
                        // register banks. An argument may be FP or an aggregate.
                        else{for (index,part) in parts.iter().take(argument_count).enumerate(){message.push(' ');message.push_str(part);message.push(':');message.push_str(&format!("argument_{index} /* ABI location unresolved */"));}}
                        message.push(']');
                        return format!("{message}; /* objc_msgSend selector validated through __objc_selrefs at {address:#x}; receiver and argument types unresolved */");
                    }
                }
                let known_values=known.iter().map(|(register,value,provenance)|format!("x{register}={value:#x} {provenance}")).collect::<Vec<_>>();
                return if let Some(name)=target_name{
                    let target_signature=signatures.get(&target).map(|(_,signature)|signature);
                    let args=target_signature.map(|signature|abi_parameters(Some(signature))).unwrap_or_else(||candidate_abi_parameters(parameter_evidence,target));
                    let bindings=args.iter().map(|argument|format!("{}={}",argument.name,argument.register)).collect::<Vec<_>>().join(", ");
                    let (mut rendered, evidence) = render_call_arguments(&args, &known, &known_float, &known_stack);
                    if target_signature.is_some_and(|sig| sig.language == "C++ / Objective-C++" && sig.declaration.contains("::")) {
                        rendered.insert(0, "/* possible implicit C++ receiver unresolved */".into());
                    }
                    if target_signature.is_some_and(|sig| sig.declaration.contains("...")) {
                        rendered.push("/* variadic arguments unresolved; Apple arm64 stack ABI */".into());
                    }
                    if let Some(buffer) = indirect_call_result_argument(target_signature) { rendered.push(buffer); }
                    let arguments=if args.is_empty()&&target_signature.is_none(){if known_values.is_empty(){"/* prototype and ABI arguments unresolved */".into()}else{format!("/* prototype unresolved; known ABI register values: {} */",known_values.join(", "))}}else{rendered.join(", ")};
                    let resolved=evidence.join(", ");
                    let binding_note=if bindings.is_empty(){"argument register bindings unresolved".into()}else if target_signature.is_some(){format!("argument bindings: {bindings}")}else{format!("candidate ABI argument bindings: {bindings}; callee types unresolved")};
                    let evidence_note=if resolved.is_empty(){"no argument constants proven in the straight-line call block".to_string()}else{format!("constant argument evidence: {resolved}")};
                    let target_evidence = if indirect { statement.split_once("/*").and_then(|(_, tail)| tail.split_once("*/")).map(|(evidence, _)| format!(" /*{}*/", evidence)).unwrap_or_default() } else { String::new() };
                    let result = call_result_destination(target_signature).map(|register|format!("{register} = ")).unwrap_or_default();
                    format!("{result}{}({}); /* {} {target:#x}; {}; {binding_note}; {evidence_note} */{target_evidence}",callable_name(name,target_signature),arguments,if indirect{"indirect target resolved through a register"}else{"direct call target"},if target_signature.is_some(){"arguments follow recovered prototype arity"}else if args.is_empty(){"callee prototype and entry-use arity unavailable"}else{"arity candidate from callee entry-register uses"})
                }else{
                    let target_evidence = if indirect { statement.split_once("/*").and_then(|(_, tail)| tail.split_once("*/")).map(|(evidence, _)| format!(" /*{}*/", evidence)).unwrap_or_default() } else { String::new() };
                    format!("call(/* unnamed target {target:#x}; {}; {} */);{target_evidence}",if indirect{"indirect target recovered from a register"}else{"direct BL"},if known_values.is_empty(){"arguments unresolved".to_string()}else{format!("known ABI register values: {}",known_values.join(", "))})
                };
            }
        }
    }
    if statement.starts_with("return /* ABI result register x0"){
        return render_signature_return(signature);
    }
    let _=(address,start);
    if stable_stack{localize_stack_slot_accesses(statement)}else{statement.to_string()}
}

#[allow(clippy::too_many_arguments)]
fn render_arm64_body(
    start: u64, end: u64, display_name: &str, signature: Option<&Signature>,
    symbols: &[(u64,String)], signatures: &BTreeMap<u64,(String,Signature)>,
    code: &[u8], base: u64, file_offset: u64, data_ranges: &[(u64,u64,u16)], parameter_evidence: &str,
    tail_calls: &[(u64,u64,u64)], jump_tables: &str, objc_selectors: &BTreeMap<u64,String>, object_layout:Option<&super::objc::ClassLayout>,
) -> Option<String> {
    if start < base || start >= end || (start - base) % 4 != 0 { return None; }
    let lo = usize::try_from(start - base).ok()?;
    let hi = usize::try_from(end.min(base.saturating_add(code.len() as u64)) - base).ok()?;
    if lo >= hi || hi > code.len() { return None; }
    let body_code = &code[lo..hi];
    let stable_stack=super::semantic::stack_pointer_is_stable(code,base,start,end);
    let machine = super::semantic::render(body_code, start, file_offset.saturating_add(lo as u64), &[start], data_ranges);
    let mut instructions = Vec::<(u64,String)>::new();
    for line in machine.lines() {
        let Some(line) = line.trim_start().strip_prefix("0x") else { continue };
        let Some((address, rest)) = line.split_once("  ") else { continue };
        let Ok(address) = u64::from_str_radix(address, 16) else { continue };
        let statement = rest.split(" // word=").next().unwrap_or(rest).trim();
        if !statement.is_empty() { instructions.push((address, statement.into())); }
    }
    let instruction_addresses = instructions.iter().map(|(address,_)| *address).collect::<std::collections::BTreeSet<_>>();
    let name_seeds = parameter_name_seeds(signature, parameter_evidence, start);
    let parameter_names: super::semantic::ParameterNameAnalysis = super::semantic::parameter_value_names(code, base, start, end, file_offset, data_ranges, &name_seeds);
    let object_statements=object_layout.map(|layout|object_member_statements(code,base,start,end,file_offset,data_ranges,layout)).unwrap_or_default();
    let no_aliases = BTreeMap::new();
    // Structured regions render their statements through
    // `render_linear_statement`, which keeps the raw registers; apply the same
    // recovered names (and stack-slot localization) the linear path applies so
    // branches do not silently drop the identities.
    let substitute_at = |address: u64, text: &str| {
        if let Some(statement)=object_statements.get(&address){return statement.clone()}
        let substituted = replace_instruction_register_names(text, parameter_names.inputs.get(&address).unwrap_or(&no_aliases),
            Some(parameter_names.outputs.get(&address).unwrap_or(&no_aliases)));
        if stable_stack { localize_stack_slot_accesses(&substituted) } else { substituted }
    };
    let switch_evidence=parse_switch_evidence(jump_tables);
    let mut labels = instructions.iter().filter_map(|(_,statement)| {
        let target = statement.strip_prefix("goto 0x")
            .or_else(|| statement.strip_prefix("if (").and_then(|rest| rest.split_once(") goto 0x").map(|(_,target)|target)))?;
        let digits = target.split(|c: char| !c.is_ascii_hexdigit()).next()?;
        u64::from_str_radix(digits,16).ok().filter(|target| instruction_addresses.contains(target))
    }).collect::<std::collections::BTreeSet<_>>();
    for evidence in switch_evidence.values(){for &(_,_,target) in &evidence.cases{if instruction_addresses.contains(&target){labels.insert(target);}}}
    let incoming_branches=instructions.iter().filter_map(|(source,statement)|branch_target_statement(statement).map(|target|(target,*source))).collect::<Vec<_>>();

    let mut out = String::new();
    out.push_str(&format!("// {start:#018x}..{end:#018x}; decoded machine operations translated to C-like statements\n"));
    if let Some(blocker) = parameter_names.blocker { out.push_str(&format!("// Source parameter identity substitution withheld: {blocker}.\n")); }
    for note in parameter_names.notes.values() { out.push_str(&format!("// Source identity evidence: {note}\n")); }
    if !stable_stack && instructions.iter().any(|(_, statement)| statement.contains("(sp")) {
        out.push_str("// SP-relative accesses keep machine expressions because this function does not prove an unchanged stack pointer.\n");
    }
    let mut opened_body = false;
    if let Some(signature) = signature {
        if signature.language == "Objective-C / Objective-C++" {
            let class = display_name.split_once('[').and_then(|(_,tail)| tail.split_once(' ').map(|(class,_)|class)).unwrap_or("UnknownClass");
            out.push_str(&format!("// @implementation {class}; Objective-C IMP ABI: x0=self, x1=_cmd\n"));
            out.push_str(signature.declaration.trim_end_matches(';'));
            out.push_str(" {\n");
            opened_body = true;
        } else if signature.source == "exact SDK ABI catalog" {
            out.push_str(signature.declaration.trim_end_matches(';'));
            out.push_str(" {\n");
            opened_body = true;
        } else {
            out.push_str(&format!("// {} [{}]\n", signature.declaration.trim_end_matches(';'), signature.source));
            out.push_str(&format!("/* body for {} */\n", callable_name(display_name, Some(signature))));
        }
    } else {
        out.push_str(&format!("/* {}: source signature unresolved; ABI arguments remain machine registers */\n", callable_name(display_name, None)));
    }
    let parameters = incoming_parameter_candidates(parameter_evidence, start);
    let bindings = abi_parameter_bindings(signature, &parameters);
    if !bindings.is_empty() {
        out.push_str(&format!("/* AAPCS64 parameter bindings: {}; names/types come from the signature evidence or remain candidates */\n", bindings.join(", ")));
    }
    if !parameters.is_empty() {
        out.push_str(&format!("/* observed entry-block uses: {}; uses before a modeled definition; this is not a complete source-variable analysis */\n", parameters.join(", ")));
    }
    if !opened_body { out.push_str("{\n"); }
    let mut instruction_index = 0usize;
    while instruction_index < instructions.len() {
        let (address, statement) = &instructions[instruction_index];
        if let Some(evidence)=switch_evidence.get(address).filter(|evidence|statement.starts_with("goto x")&&!evidence.cases.is_empty()){
            let index_register=evidence.index_register?;
            let bound=evidence.range.as_ref().map(|range|format!(" proven unsigned range: {range};")).unwrap_or_else(||" index range unresolved;".into());
            let normalization=evidence.normalization.as_ref().map(|value|format!(" normalized index: {value};")).unwrap_or_default();
            out.push_str(&format!("    {address:#018x}: switch (x{index_register}) {{ /* bounded jump-table candidate;{bound}{normalization} case labels are table indices, source switch remains unproven */\n"));
            for &(case,selector_bits,target) in &evidence.cases {
                let destination=if instruction_addresses.contains(&target){format!("goto loc_{target:x};")}else{format!("goto {target:#x}; /* target outside this recovered function */")};
                let selector=selector_bits.map(|bits|format!("; normalized selector bits {bits:#x}")).unwrap_or_default();
                out.push_str(&format!("        case {case}: {destination} /* table target {target:#x}{selector} */\n"));
            }
            out.push_str(&format!("        default: /* unresolved indirect target in x{}; no edge inferred */\n    }}\n",evidence.branch_register));
            instruction_index+=1;
            continue;
        }
        // Structure a header-tested loop only when its false exit is the
        // instruction after one explicit backedge and the body is linear.
        if let Some(rest)=statement.strip_prefix("if ("){
            if let Some((condition,exit_text))=rest.split_once(") goto 0x"){
                if let Ok(exit)=u64::from_str_radix(exit_text.trim_end_matches(';'),16){
                    let header=*address;
                    let backedge=(instruction_index+1..instructions.len()).find(|&index|{
                        instructions[index].1.starts_with("goto 0x")&&branch_target_statement(&instructions[index].1)==Some(header)
                    });
                    if let Some(backedge)=backedge{
                        let tail_address=instructions[backedge].0;
                        let exit_follows_backedge=tail_address.checked_add(4)==Some(exit)
                            &&(instructions.get(backedge+1).is_some_and(|(pc,_)|*pc==exit)||exit==end);
                        let no_body_entries=incoming_branches.iter().all(|(target,_)|! (header<*target&&*target<tail_address));
                        let incoming_to_header=incoming_branches.iter().filter(|(target,_)|*target==header).collect::<Vec<_>>();
                        let single_backedge=incoming_to_header.len()==1&&incoming_to_header[0].1==tail_address;
                        if exit_follows_backedge&&no_body_entries&&single_backedge
                            &&(instruction_index+1..backedge).all(|index|is_linear_statement(&instructions[index].1)){
                            out.push_str(&format!("    {header:#018x}: while (!({})) {{ /* natural loop */\n",substitute_at(header,condition)));
                            for (body_address,body_statement) in &instructions[instruction_index+1..backedge]{
                                let rendered=render_linear_statement(body_statement,*body_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
                                let rendered=substitute_at(*body_address,&rendered);
                                out.push_str(&format!("        {body_address:#018x}: {rendered}\n"));
                            }
                            out.push_str(&format!("    }} /* backedge at {tail_address:#x}; exit at {exit:#x} */\n"));
                            instruction_index=backedge+1;
                            continue;
                        }
                    }
                }
            }
        }
        // A conditional backedge at the end of a straight-line region proves a
        // natural do/while loop. Require this branch to be the sole explicit
        // edge into the header and reject any edge into the loop interior.
        if !statement.starts_with("if ("){
            let loop_tail=(instruction_index+1..instructions.len()).find(|&index|{
                instructions[index].1.starts_with("if (")&&branch_target_statement(&instructions[index].1)==Some(*address)
            });
            if let Some(loop_tail)=loop_tail{
                let tail_statement=instructions[loop_tail].1.as_str();
                let condition=tail_statement.strip_prefix("if (").and_then(|rest|rest.split_once(") goto 0x").map(|(condition,_)|condition));
                let loop_header=*address;
                let incoming_to_header=incoming_branches.iter().filter(|(target,_)|*target==loop_header).collect::<Vec<_>>();
                let single_backedge=incoming_to_header.len()==1&&incoming_to_header[0].1==instructions[loop_tail].0;
                let no_interior_entries=incoming_branches.iter().all(|(target,_)|! (loop_header<*target&&*target<instructions[loop_tail].0));
                let linear_body=(instruction_index..loop_tail).all(|index|is_linear_statement(&instructions[index].1));
                if let Some(condition)=condition.filter(|_|single_backedge&&no_interior_entries&&linear_body){
                    out.push_str(&format!("    {loop_header:#018x}: do {{ /* natural loop */\n"));
                    for (body_address,body_statement) in &instructions[instruction_index..loop_tail]{
                                let rendered=render_linear_statement(body_statement,*body_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
                                let rendered=substitute_at(*body_address,&rendered);
                        out.push_str(&format!("        {body_address:#018x}: {rendered}\n"));
                    }
                    out.push_str(&format!("    }} while ({}); /* backedge at {:#x} */\n",substitute_at(instructions[loop_tail].0,condition),instructions[loop_tail].0));
                    instruction_index=loop_tail+1;
                    continue;
                }
            }
        }
        // Structure only a closed diamond whose arms contain no control-flow
        // transfers and whose entry has no competing branch into its middle.
        if let Some(rest)=statement.strip_prefix("if ("){
            if let Some((condition,target_text))=rest.split_once(") goto 0x"){
                if let Ok(else_start)=u64::from_str_radix(target_text.trim_end_matches(';'),16){
                    let else_index=instructions.iter().position(|(pc,_)|*pc==else_start);
                    if let Some(else_index)=else_index.filter(|index|*index>instruction_index+1){
                        let then_branch=else_index.checked_sub(1).filter(|index|instructions[*index].1.starts_with("goto 0x"));
                        if let Some(then_branch)=then_branch{
                            if let Some(join)=branch_target_statement(&instructions[then_branch].1){
                                let join_index=instructions.iter().position(|(pc,_)|*pc==join);
                                if let Some(join_index)=join_index.filter(|index|*index>else_index){
                                    let arm_is_linear=|range:std::ops::Range<usize>|range.into_iter().all(|index|is_linear_statement(&instructions[index].1));
                                    let branch_enters_else_once=incoming_branches.iter().filter(|(target,_)|*target==else_start).all(|(_,source)|*source==*address)
                                        &&incoming_branches.iter().any(|(target,source)|*target==else_start&&*source==*address);
                                    let branch_skips_then=incoming_branches.iter().any(|(target,source)|*target==join&&*source==instructions[then_branch].0);
                                    let no_mid_arm_entries=incoming_branches.iter().all(|(target,source)|{
                                        let then_middle=instructions[instruction_index+1].0<*target&&*target<instructions[then_branch].0;
                                        let else_middle=else_start<*target&&*target<join;
                                        (!then_middle&&!else_middle)||(*source==*address&&*target==else_start)
                                    });
                                    if branch_enters_else_once&&branch_skips_then&&no_mid_arm_entries
                                        &&arm_is_linear(instruction_index+1..then_branch)
                                        &&arm_is_linear(else_index..join_index){
                                        out.push_str(&format!("    {address:#018x}: if (!({})) {{ /* structured diamond; else={else_start:#x}, join={join:#x} */\n",substitute_at(*address,condition)));
                                        for (arm_address,arm_statement) in &instructions[instruction_index+1..then_branch]{
                                            let rendered=render_linear_statement(arm_statement,*arm_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
                                            let rendered=substitute_at(*arm_address,&rendered);
                                            out.push_str(&format!("        {arm_address:#018x}: {rendered}\n"));
                                        }
                                        out.push_str("    } else {\n");
                                        for (arm_address,arm_statement) in &instructions[else_index..join_index]{
                                            let rendered=render_linear_statement(arm_statement,*arm_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
                                            let rendered=substitute_at(*arm_address,&rendered);
                                            out.push_str(&format!("        {arm_address:#018x}: {rendered}\n"));
                                        }
                                        out.push_str("    }\n");
                                        instruction_index=join_index;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // A common compiler shape is `if (condition) goto after; return value;`.
        // The branch skips the return, so express the fallthrough return as a
        // structured conditional. Keep both source instruction addresses in
        // the comment so the higher-level form remains auditable.
        if let Some(rest) = statement.strip_prefix("if (") {
            if let Some((condition, target)) = rest.split_once(") goto 0x") {
                let target = target.trim_end_matches(';');
                let next_index = instruction_index + 1;
                if let (Ok(target), Some((return_address, return_statement))) =
                    (u64::from_str_radix(target, 16), instructions.get(next_index))
                {
                    let after_return = return_address.saturating_add(4);
                    if target == after_return && return_statement.starts_with("return ") {
                        let return_statement = if return_statement.starts_with("return /* ABI result register x0") {
                            render_signature_return(signature)
                        } else { return_statement.to_string() };
                        let return_statement = substitute_at(*return_address,&return_statement);
                        out.push_str(&format!("    if (!({})) {return_statement} /* structured from {address:#x} and {return_address:#x} */\n",substitute_at(*address,condition)));
                        instruction_index += 2;
                        continue;
                    }
                }
            }
        }
        let (address, statement) = (*address, statement.as_str());
        if labels.contains(&address) { out.push_str(&format!("loc_{address:x}:\n")); }
        let statement = if statement.starts_with("call(0x") {
            render_linear_statement(statement,address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors)
        } else if let Some(rest) = statement.strip_prefix("if (") {
            if let Some((condition,target)) = rest.split_once(") goto 0x") {
                if let Ok(target) = u64::from_str_radix(target.trim_end_matches(';'),16) {
                    if labels.contains(&target) { format!("if ({condition}) goto loc_{target:x}; /* retained as a branch; no supported structured region was proven */") }
                    else { format!("if ({condition}) goto {target:#x}; /* target outside this recovered function */") }
                } else { statement.to_string() }
            } else { statement.to_string() }
        } else if let Some(target) = statement.strip_prefix("goto 0x") {
            if let Ok(target) = u64::from_str_radix(target.trim_end_matches(';'),16) {
                if labels.contains(&target) { format!("goto loc_{target:x}; /* retained as a branch; no supported structured region was proven */") }
                else if tail_calls.contains(&(start, address, target)) {
                    let target_name = signatures.get(&target).map(|(name,_)| name.as_str())
                        .or_else(|| symbols.iter().find(|(candidate,_)| *candidate == target).map(|(_,name)| name.as_str()));
                    if let Some(name) = target_name {
                        let target_signature = signatures.get(&target).map(|(_,signature)| signature);
                        let args = abi_parameters(target_signature);
                        let known = super::semantic::known_call_argument_values(code, base, start, address);
                        let known_float = super::semantic::known_call_float_argument_values(code, base, start, address);
                        let known_stack = super::semantic::known_call_stack_argument_values(code, base, start, address);
                        let (mut rendered, evidence) = render_call_arguments(&args, &known, &known_float, &known_stack);
                        let evidence_note = if evidence.is_empty() { String::new() } else { format!("; constant argument evidence: {}", evidence.join(", ")) };
                        let arguments = if args.is_empty() && target_signature.is_none() {
                            "/* AAPCS64 argument registers unresolved */".into()
                        } else {
                            if target_signature.is_some_and(|sig| sig.language == "C++ / Objective-C++" && sig.declaration.contains("::")) {
                                rendered.insert(0, "/* possible implicit C++ receiver unresolved */".into());
                            }
                            if target_signature.is_some_and(|sig| sig.declaration.contains("...")) {
                                rendered.push("/* variadic arguments unresolved; Apple arm64 stack ABI */".into());
                            }
                            if let Some(buffer) = indirect_call_result_argument(target_signature) { rendered.push(buffer); }
                            rendered.join(", ")
                        };
                        let callee = callable_name(name,target_signature);
                        if signature_returns_void(target_signature) {
                            format!("{callee}({arguments}); return; /* cross-function B tail-call candidate{evidence_note} */")
                        } else {
                            format!("return {callee}({arguments}); /* cross-function B tail-call candidate; return compatibility unresolved{evidence_note} */")
                        }
                    } else { format!("goto {target:#x}; /* tail-call candidate; target name unresolved */") }
                }
                else { format!("goto {target:#x}; /* target outside this recovered function */") }
            } else { statement.to_string() }
        } else if statement.starts_with("goto x") {
            format!("{} /* indirect target unresolved; no bounded table evidence proved a structured target set */",statement.split(" /*").next().unwrap_or(statement))
        } else if statement.starts_with("return /* ABI result register x0") {
            render_signature_return(signature)
        } else { statement.to_string() };
        let statement = substitute_at(address,&statement);
        out.push_str(&format!("    {address:#018x}: {statement}\n"));
        instruction_index += 1;
    }
    if instructions_truncated(&machine) { out.push_str("    /* instruction listing truncated by the semantic preview budget */\n"); }
    out.push_str("}\n");
    Some(out)
}

fn instructions_truncated(machine: &str) -> bool {
    machine.contains("function preview truncated at instruction budget")
}

fn recover_internal(
    starts: &[u64], symbols: &[(u64, String)], objc_records: &str,
    calls: &[(u64,u64,u64)], scan_end: u64,
    external_prototypes:&BTreeMap<String,Signature>,
    arm64: Option<Arm64RenderContext<'_>>,
) -> (String, String) {
    let mut by_address = BTreeMap::<u64,(String,Signature)>::new();
    let mut name_only = BTreeMap::<String,Signature>::new();
    for (address,name) in symbols { if let Some(sig) = symbol(name).or_else(||external_prototypes.get(name).cloned()) {
        if *address == 0 { name_only.insert(name.clone(),sig); }
        else { by_address.insert(*address,(name.clone(),sig)); }
    } }
    for (imp,signature) in objc_signature_candidates(objc_records) {by_address.insert(imp,signature);}
    let object_models=arm64.as_ref().map(|context|context.objc_layouts).unwrap_or(&[]);
    refine_property_signatures(&mut by_address,object_models);
    let declarations=object_declarations(object_models);
    let mut signatures = String::from("Signature evidence: exact SDK ABI declarations, validated Objective-C type encodings, hash-bound analyst C prototypes, and supported Itanium mangled parameter types. Unknown source types remain unresolved.\n");
    signatures.push_str(&declarations);
    for (address,(name,sig)) in by_address.iter().take(700) {
        let abi = if arm64.is_some() {
            format!("    ABI profile=Apple-arm64-LP64 bindings=[{}] result={}{}\n",
                abi_parameter_bindings(Some(sig), &[]).join(", "), render_signature_return(Some(sig)),
                if sig.declaration.contains("...") { " variadic-arguments=unresolved" } else { "" })
        } else { String::new() };
        let row = format!("{address:#018x} {name} language={} source={} declaration={}\n{abi}",sig.language,sig.source,sig.declaration);
        if signatures.len()+row.len() > 60_000 { break } signatures.push_str(&row);
    }
    for (name,sig) in name_only.iter().take(200) {
        let row = format!("name-only {name} language={} source={} declaration={}\n",sig.language,sig.source,sig.declaration);
        if signatures.len()+row.len() > 60_000 { break } signatures.push_str(&row);
    }
    if by_address.is_empty() && name_only.is_empty() { signatures.push_str("No source-level signatures were supported by available metadata or the exact API catalog.\n"); }
    let mut pseudo = String::from("// Bounded C-like reconstruction. Statements come from decoded AArch64 operations; declarations use validated runtime metadata or exact symbol prototypes. Unknown types, values, and effects stay marked.\n");
    pseudo.push_str(&declarations);
    let mut emitted_functions = 0usize;
    for (i,&start) in starts.iter().take(200).enumerate() {
        let end = starts.get(i+1).copied().unwrap_or(scan_end);
        if pseudo.len() > 55_000 { break }
        let named = by_address.get(&start);
        let name = named.map(|(name,_)| name.as_str()).or_else(||symbols.iter().find(|(address,_)| *address == start).map(|(_,name)|name.as_str())).unwrap_or("<unnamed>");
        let signature = named.map(|(_,signature)| signature);
        if let Some(context) = &arm64 {
            let end = starts.get(i+1).copied().unwrap_or(scan_end);
            let layout=object_layout_for_imp(context.objc_layouts,start,name,signature);
            if let Some(body) = render_arm64_body(start, end, name, signature, symbols, &by_address, context.code, context.base, context.file_offset, context.data_ranges, context.parameter_evidence, context.tail_calls, context.jump_tables, context.objc_selectors,layout) {
                if pseudo.len() + body.len() > 58_000 { break; }
                pseudo.push_str(&body);
                pseudo.push('\n');
                emitted_functions += 1;
                continue;
            }
        }
        if let Some((name,sig)) = named {
            pseudo.push_str(&format!("// {start:#018x} {name}; {}\n{}\n",sig.source,sig.declaration.trim_end_matches(';')));
        } else {
            let name = symbols.iter().find(|(address,_)| *address == start).map(|(_,name)| name.as_str()).unwrap_or("<unnamed>");
            pseudo.push_str(&format!("// {start:#018x} {name}; signature unresolved\n/* return type unresolved */ function_{start:x}(/* parameters unresolved */)\n"));
        }
        pseudo.push_str("{\n");
        for &(_,site,target) in calls.iter().filter(|&&(caller,site,_)| caller == start && site >= start && site < end).take(24) {
            let callee = by_address.get(&target).map(|(name,_)| name.as_str()).or_else(||symbols.iter().find(|(a,_)|*a==target).map(|(_,n)|n.as_str())).unwrap_or("<unresolved>");
            let row=format!("    call_target({target:#018x}); /* BL @ {site:#018x} {callee}; arguments and result unresolved */\n");
            if pseudo.len()+row.len()>58_000 {pseudo.push_str("    /* call listing truncated */\n");break}
            pseudo.push_str(&row);
        }
        pseudo.push_str("    /* remaining machine operations and return value unresolved */\n}\n\n");
        emitted_functions += 1;
    }
    if emitted_functions < starts.len() {
        pseudo.push_str(&format!("/* {} functions omitted from this bounded preview; use per-function reconstruction for their output. */\n", starts.len() - emitted_functions));
    }
    (signatures,pseudo)
}

#[cfg(test)] mod tests {
    #[test]
    fn jump_table_evidence_retains_proven_range_and_modular_selector_bits() {
        let report = "0x0000000000001000  BR x14  bounded indexed-table evidence\n\
            table=0x2000 format=table-relative signed offsets entry_width=4 table_index=x8 index-provenance=unknown scan_limit=32 confidence=candidate case-range=CMP/B.HI x8<=3 proven-entry-count=4 index-normalization=w8=(w0-5) modulo 2^32\n\
              case-index=0 selector-bits=0x5 entry=0x2000 target=0x0000000000001100 validation=executable-aligned\n\
            validated_entries=1; case mapping remains a candidate.\n";
        let parsed = super::parse_switch_evidence(report);
        let evidence = parsed.get(&0x1000).expect("validated table candidate");
        assert_eq!(evidence.index_register, Some(8));
        assert_eq!(evidence.range.as_deref(), Some("CMP/B.HI x8<=3"));
        assert_eq!(evidence.normalization.as_deref(), Some("w8=(w0-5)"));
        assert_eq!(evidence.cases, vec![(0, Some(5), 0x1100)]);

        let branch = 0xd61f_01c0u32.to_le_bytes(); // br x14
        let body = super::render_arm64_body(0x1000, 0x1004, "_switch", None, &[],
            &std::collections::BTreeMap::new(), &branch, 0x1000, 0, &[], "", &[], report,
            &std::collections::BTreeMap::new(), None).expect("rendered candidate switch");
        assert!(body.contains("proven unsigned range: CMP/B.HI x8<=3"), "{body}");
        assert!(body.contains("normalized index: w8=(w0-5)"), "{body}");
        assert!(body.contains("normalized selector bits 0x5"), "{body}");
        assert!(body.contains("source switch remains unproven"), "{body}");
        assert!(body.contains("default: /* unresolved indirect target in x14; no edge inferred */"), "{body}");
        assert!(!body.contains("default: goto x14"), "{body}");
    }

    #[test]
    fn source_array_declarators_never_silently_lose_pointer_or_row_shape() {
        for declaration in ["uint64_t hs_test(uint64_t items[][4]);",
            "uint64_t hs_test(uint64_t items[3][4]);", "uint64_t hs_test(uint64_t *items[]);",
            "uint64_t hs_test(uint64_t items[n++]);", "uint64_t hs_test(uint64_t items[4] trailing);"] {
            let signature = Signature { language: "C", declaration: declaration.into(), source: "analyst" };
            assert!(source_signature("_hs_test", &signature).is_none(), "body flattened {declaration}");
            assert!(source_declaration("_hs_test", &signature).is_none(), "header flattened {declaration}");
        }
        for declaration in ["uint64_t hs_test(uint64_t items[]);", "uint64_t hs_test(const uint64_t items[4]);"] {
            let signature = Signature { language: "C", declaration: declaration.into(), source: "analyst" };
            let recovered = source_signature("_hs_test", &signature).expect("one-dimensional scalar array decay");
            assert!(recovered.parameters[0].pointer);
            assert_eq!(recovered.parameters[0].name, "items");
            assert!(source_declaration("_hs_test", &signature).is_some());
        }
    }
    #[test]
    fn project_catalog_types_enable_only_exact_single_level_aggregate_pointers() {
        let project_type=crate::reconstruction::ProjectTypeDependency {
            name:"Point".into(), declaration:"typedef struct Point { int x; int y; } Point;".into(),
            target:"arm64-apple-macos14.0".into(), source_sha256:"a".repeat(64), layout_evidence:None, layout:None,
        };
        let signature=Signature { language:"C", declaration:"int hs_test(Point *point);".into(), source:"hash-bound analyst C prototype catalog" };
        assert!(source_signature("_hs_test",&signature).is_none(),"unmatched project aliases must remain unresolved");
        let recovered=source_signature_with_project_types("_hs_test",&signature,std::slice::from_ref(&project_type)).unwrap();
        assert_eq!(recovered.parameters[0].spelling,"Point *");
        assert!(recovered.parameters[0].pointer);
        assert_eq!(recovered.parameters[0].bits,64);
        assert!(source_declaration_with_project_types("_hs_test",&signature,std::slice::from_ref(&project_type)).is_some());
        for declaration in ["int hs_test(Point value);","int hs_test(Point **point);"] {
            let unsupported=Signature { language:"C", declaration:declaration.into(), source:"test" };
            assert!(source_signature_with_project_types("_hs_test",&unsupported,std::slice::from_ref(&project_type)).is_none(),"{declaration}");
        }
    }
    #[test]
    fn declarations_are_independent_of_body_register_eligibility() {
        let signed = Signature { language: "C", declaration: "signed int hs_test(signed int value);".into(), source: "analyst" };
        let recovered = source_signature("_hs_test", &signed).expect("signed int is a supported scalar spelling");
        assert_eq!(recovered.result_bits, 32);
        assert!(recovered.result_signed && recovered.parameters[0].signed);
        assert_eq!(recovered.parameters[0].register, 0);
        for (spelling, bits) in [("signed char", 8), ("signed short", 16), ("signed short int", 16),
            ("signed", 32), ("signed int", 32), ("signed long", 64), ("signed long int", 64),
            ("signed long long", 64), ("signed long long int", 64)] {
            let signature = Signature { language: "C", declaration: format!("{spelling} hs_test({spelling} value);"), source: "test" };
            let recovered = source_signature("_hs_test", &signature).expect(spelling);
            assert_eq!((recovered.result_bits, recovered.parameters[0].bits), (bits, bits), "{spelling}");
            assert!(recovered.result_signed && recovered.parameters[0].signed, "{spelling}");
        }
        for declaration in ["double hs_test(double first, float second);",
            "long double hs_test(long double value);",
            "__int128 hs_test(unsigned __int128 value);",
            "NSRange hs_test(NSRange value);", "CFRange hs_test(CFRange value);",
            "CGPoint hs_test(CGPoint value);", "CGSize hs_test(CGSize value);",
            "CGRect hs_test(CGRect value);", "CGAffineTransform hs_test(CGAffineTransform value);",
            "uint64_t hs_test(uint64_t a, uint64_t b, uint64_t c, uint64_t d, uint64_t e, uint64_t f, uint64_t g, uint64_t h, uint64_t i);"] {
            let signature = Signature { language: "C", declaration: declaration.into(), source: "analyst" };
            let recovered = source_declaration("_hs_test", &signature).expect("safe scalar header");
            assert_eq!(recovered.name, "hs_test");
            assert!(!recovered.parameters.is_empty());
            assert!(source_signature("_hs_test", &signature).is_none(), "body ABI remains unsupported");
        }
        for declaration in ["Mystery hs_test(Mystery value);", "double hs_test(double switch);",
            "double hs_test(double value, double value);", "double hs_test(double value, ...);"] {
            let signature = Signature { language: "C", declaration: declaration.into(), source: "analyst" };
            assert!(source_declaration("_hs_test", &signature).is_none(), "{declaration}");
        }
    }
    use super::*;
    fn member_fixture() -> super::super::objc::ClassLayout {
        use super::super::objc::*;
        ClassLayout { address:0x2000,name:"Widget".into(),instance_start:8,instance_size:16,
            ivars:vec![IvarLayout{record:0x3000,offset_pointer:0x4000,name:"x0".into(),encoding:"q".into(),offset:8,size:8,alignment:8}],
            properties:vec![PropertyLayout::parse(0x5000,"label".into(),"T@\"NSString\",R".into()).unwrap()],
            methods:vec![MethodLayout{implementation:0x1000,class_method:false}],ivars_complete:true,properties_complete:true,runtime:false }
    }

    #[test] fn property_types_refine_only_unambiguous_generic_id_accessors() {
        let layout=member_fixture();
        let getter=objc_method("-[Widget label]","@16@0:8").unwrap();
        let mut signatures=BTreeMap::from([(0x1000,("-[Widget label]".into(),getter.clone()))]);
        refine_property_signatures(&mut signatures,std::slice::from_ref(&layout));
        assert_eq!(signatures[&0x1000].1.declaration,"- (NSString *)label;");
        let explicit=objc_method("-[Widget label]","@\"NSNumber\"16@0:8").unwrap();
        signatures.get_mut(&0x1000).unwrap().1=explicit.clone();
        refine_property_signatures(&mut signatures,std::slice::from_ref(&layout));
        assert_eq!(signatures[&0x1000].1,explicit);
        for attributes in ["T@\"NSNumber\",R,Glabel","Ti,R,Glabel"] {
            let mut conflicting=layout.clone();
            conflicting.properties.push(super::super::objc::PropertyLayout::parse(0x5008,"alternative".into(),attributes.into()).unwrap());
            signatures.get_mut(&0x1000).unwrap().1=getter.clone();
            refine_property_signatures(&mut signatures,&[conflicting]);
            assert_eq!(signatures[&0x1000].1,getter);
        }
        let mut partial=layout.clone();partial.properties_complete=false;
        signatures.get_mut(&0x1000).unwrap().1=getter.clone();
        refine_property_signatures(&mut signatures,&[partial]);
        assert_eq!(signatures[&0x1000].1,getter);
    }

    #[test] fn shared_imps_and_class_methods_do_not_supply_one_instance_receiver() {
        let layout=member_fixture();let signature=objc_method("-[Widget label]","@16@0:8").unwrap();
        assert!(object_layout_for_imp(std::slice::from_ref(&layout),0x1000,"-[Widget label]",Some(&signature)).is_some());
        assert!(object_layout_for_imp(std::slice::from_ref(&layout),0x1000,"+[Widget label]",Some(&signature)).is_none());
        let mut other=layout.clone();other.address=0x6000;other.name="OtherWidget".into();
        assert!(object_layout_for_imp(&[layout.clone(),other.clone()],0x1000,"-[Widget label]",Some(&signature)).is_none());
        let mut signatures=BTreeMap::from([(0x1000,("-[Widget label]".into(),signature.clone()))]);
        refine_property_signatures(&mut signatures,&[layout,other]);
        assert_eq!(signatures[&0x1000].1,signature);
    }

    #[test] fn field_names_that_look_like_registers_remain_source_identifiers() {
        let layout=member_fixture();
        let signature=objc_method("-[Widget value]","q16@0:8").unwrap();
        let code=[0xf9400400u32,0xd65f03c0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let body=render_arm64_body(0x1000,0x1008,"-[Widget value]",Some(&signature),&[],&BTreeMap::new(),&code,0x1000,0,&[],"",&[],"",&BTreeMap::new(),Some(&layout)).unwrap();
        assert!(body.contains("x0 = self->x0;"),"{body}");
        assert!(!body.contains("self->self"),"{body}");
    }

    #[test] fn terminal_statements_cannot_be_folded_into_structured_regions() {
        assert!(is_linear_statement("x0 = x0 + 1;"));
        assert!(!is_linear_statement("return x0;"));
        assert!(!is_linear_statement("goto 0x1000;"));
        assert!(!is_linear_statement("if (x0 == 0) goto 0x1000;"));
    }
    #[test] fn objc_encoding() {
        let empty_keyword = objc_method("-[Widget combine::]", "q32@0:8q16q24").unwrap();
        assert_eq!(empty_keyword.declaration, "- (long long)combine:(long long)arg0 :(long long)arg1;");
        assert_eq!(abi_parameters(Some(&empty_keyword)).iter().map(|arg| arg.register.as_str()).collect::<Vec<_>>(), vec!["x0", "x1", "x2", "x3"]);
        assert!(objc_method("-[Widget 1bad:]", "v24@0:8i16").is_none());
        assert!(objc_method("-[Widget bad:tail]", "v24@0:8i16").is_none());
        let s=objc_method("-[NSString substringFromIndex:]", "@\"NSString\"24@0:8Q16").unwrap();
        assert_eq!(s.declaration,"- (NSString *)substringFromIndex:(NSUInteger)from;");
        assert!(objc_method("-[A x:]", "v24@0:8").is_none());
        assert_eq!(objc_method("-[A rect:]", "v48@0:8{CGRect={CGPoint=dd}{CGSize=dd}}16").unwrap().declaration, "- (void)rect:(CGRect)arg0;");
        assert!(objc_method("-[A rect:]", "v48@0:8{CGRect=qqqq}16").is_none());
        assert_eq!(objc_method("-[A constrained:]", "@\"Widget<Primary><Tracing>\"24@0:8@\"<Primary><Tracing>\"16").unwrap().declaration,
            "- (Widget<Primary, Tracing> *)constrained:(id<Primary, Tracing>)arg0;");
        assert_eq!(abi_parameters(objc_method("-[A constrained:]", "@\"<Primary>\"24@0:8@\"<Tracing>\"16").as_ref()).iter().map(|value|value.register.as_str()).collect::<Vec<_>>(),vec!["x0","x1","x2"]);
        for encoding in ["@\"<Bad->\"16@0:8", "@\"<Primary\"16@0:8", "@\"<>\"16@0:8"] { assert!(objc_method("-[A bad]",encoding).is_none(),"{encoding}"); }
    }
    #[test] fn sdk_method_names_cannot_override_conflicting_runtime_types() {
        let pointer_conflict = objc_method("-[NSString length]", "@16@0:8").unwrap();
        assert_eq!(pointer_conflict.declaration, "- (id)length;");
        assert!(pointer_conflict.source.contains("conflicts"));
        let width_conflict = objc_method("-[NSString substringFromIndex:]", "@20@0:8i16").unwrap();
        assert_eq!(width_conflict.declaration, "- (id)substringFromIndex:(int)arg0;");
        assert!(width_conflict.source.contains("conflicts"));
        let signed_conflict = objc_method("-[NSString substringFromIndex:]", "@24@0:8q16").unwrap();
        assert_eq!(signed_conflict.declaration, "- (id)substringFromIndex:(long long)arg0;");
        assert!(signed_conflict.source.contains("conflicts"));
        let matched = objc_method("-[NSString substringFromIndex:]", "@24@0:8Q16").unwrap();
        assert!(matched.source.contains("encoding ABI validated"));
        assert_eq!(matched.declaration, "- (NSString *)substringFromIndex:(NSUInteger)from;");
    }
    #[test] fn reconstruction_preserves_sdk_and_external_prototype_disagreements() {
        let external=BTreeMap::from([("_malloc".to_owned(),Signature{language:"C",declaration:"int malloc(int count);".into(),source:"hash-bound analyst C prototype catalog"})]);
        let candidates=source_candidates(0x1000,&["_malloc"],"",&external);
        assert_eq!(candidates.len(),2);
        assert!(candidates.iter().any(|(_,signature)|signature.declaration=="int malloc(int count);"));
        assert!(candidates.iter().any(|(_,signature)|signature.source=="exact SDK ABI catalog"));
        let mut matching=symbol("_malloc").unwrap();
        matching.source="hash-bound analyst C prototype catalog";
        let matched=source_candidates(0x1000,&["_malloc"],"",&BTreeMap::from([("_malloc".into(),matching)]));
        assert_eq!(matched.len(),1,"identical declarations must not block reconstruction as a conflict");
        assert!(matched[0].1.source.contains("SDK")&&matched[0].1.source.contains("hash-bound"));
    }
    #[test] fn exact_catalog_and_cpp() {
        assert_eq!(symbol("_CFRelease").unwrap().declaration,"void CFRelease(CFTypeRef cf);");
        assert!(symbol("_CFReleaseExtra").is_none());
        assert_eq!(symbol("__ZN3Foo3barEi").unwrap().declaration,"/* return type unresolved */ Foo::bar(int arg0);");
        assert_eq!(symbol("__Z7consumeP6Widget").unwrap().declaration,"/* return type unresolved */ consume(Widget * arg0);");
        assert_eq!(symbol("__Z7consumeRN3API6WidgetE").unwrap().declaration,"/* return type unresolved */ consume(API::Widget & arg0);");
        assert!(symbol("__Z3foo").is_none());
        assert!(symbol("__Z3foovi").is_none());
        assert_eq!(symbol("__Z3fooKPi").unwrap().declaration,"/* return type unresolved */ foo(int * const arg0);");
        assert_eq!(symbol("__Z3fooPKi").unwrap().declaration,"/* return type unresolved */ foo(const int * arg0);");
        assert_eq!(symbol("__Z3fooPKPi").unwrap().declaration,"/* return type unresolved */ foo(int * const * arg0);");
        assert_eq!(symbol("__Z3fooKVPi").unwrap().declaration,"/* return type unresolved */ foo(int * volatile const arg0);");
        assert!(symbol("__Z3fooKRi").is_none());
        assert!(symbol("__Z3fooRv").is_none());
        assert!(symbol("__Z3fooRKv").is_none());
        assert!(symbol("__Z3fooRRi").is_none());
        assert!(symbol("__Z3fooPRi").is_none());
        assert_eq!(symbol("__Z3fooPv").unwrap().declaration,"/* return type unresolved */ foo(void * arg0);");
        assert_eq!(symbol("__Z3fooPKv").unwrap().declaration,"/* return type unresolved */ foo(const void * arg0);");
        assert!(symbol("__Z3fooKv").is_none());
        assert_eq!(symbol("__Z3fooiz").unwrap().declaration,"/* return type unresolved */ foo(int arg0, ...);");
        assert_eq!(symbol("__Z1fwDsDien").unwrap().declaration,"/* return type unresolved */ f(wchar_t arg0, char16_t arg1, char32_t arg2, long double arg3, __int128 arg4);");
        let typed = symbol("__Z1fwDsDien").unwrap();
        let args = abi_parameters(Some(&typed));
        assert_eq!(args.iter().take(4).map(|arg| arg.register.as_str()).collect::<Vec<_>>(), vec!["x0", "x1", "x2", "d0"]);
        assert_eq!(args[4].register, "wide_integer<__int128>(x3, x4)");
        assert!(symbol("__Z1fD").is_none());
        assert_eq!(symbol("__Z3fooz").unwrap().declaration,"/* return type unresolved */ foo(...);");
        assert!(symbol("__Z3foozi").is_none());
        assert_eq!(abi_parameters(symbol("__Z7consumeP6Widget").as_ref())[0].register,"x0");
        assert!(symbol("__ZN3Foo3barESt6vectorIiE").is_none());
    }
    #[test] fn objc_review_headers_require_one_owner_and_keep_conflicts_unresolved() {
        let layout=crate::objc::ClassLayout{address:0x2000,name:"Widget".into(),instance_start:8,instance_size:8,
            ivars:vec![],properties:vec![],methods:vec![crate::objc::MethodLayout{implementation:0x1000,class_method:false}],
            ivars_complete:true,properties_complete:true,runtime:false};
        let records=" instance method 0x3000 name=run types=v16@0:8 imp=0x1000 resolved=-[Widget run] confidence=validated\n";
        let header=objc_header_for_imp(0x1000,records,std::slice::from_ref(&layout)).unwrap();
        assert!(header.contains("@interface Widget")&&header.contains("- (void)run;"));
        assert!(header.contains("superclass")&&header.contains("REVIEW ONLY"));
        let related=format!("reconstructed class 0x2000  name=Widget  super_name=NSObject  conforms=NSCopying,Tracing\n{records}");
        let related_header=objc_header_for_imp(0x1000,&related,std::slice::from_ref(&layout)).unwrap();
        assert!(related_header.contains("@interface Widget : NSObject <NSCopying, Tracing>"));
        assert!(related_header.contains("@protocol NSCopying, Tracing;"));
        let unrelated=format!("reconstructed class 0x9999  name=Widget  super_name=WrongParent\n{records}");
        assert!(!objc_header_for_imp(0x1000,&unrelated,std::slice::from_ref(&layout)).unwrap().contains("WrongParent"));
        let disputed=format!("{related}reconstructed class 0x2000  name=Widget  super_name=OtherParent\n");
        let disputed_header=objc_header_for_imp(0x1000,&disputed,std::slice::from_ref(&layout)).unwrap();
        assert!(disputed_header.contains("relationships withheld")&&!disputed_header.contains("Widget : NSObject"));
        assert!(objc_header_for_imp(0x1004,records,std::slice::from_ref(&layout)).is_none());
        assert!(objc_header_for_imp(0x1000,records,&[layout.clone(),layout.clone()]).is_none());
        let conflict=format!("{records} instance method 0x3001 name=run types=q16@0:8 imp=0x1000 resolved=-[Widget run] confidence=validated\n");
        let header=objc_header_for_imp(0x1000,&conflict,&[layout]).unwrap();
        assert!(header.contains("Conflicting method type encodings"));
        assert!(!header.contains("- (void)run;")&&!header.contains("- (long long)run;"));
    }
    #[test] fn hash_bound_external_c_prototypes_supply_types_and_reject_unsafe_records() {
        assert!(valid_apple_sdk_target("arm64-apple-macos14.0", "/SDKs/MacOSX27.0.sdk"));
        assert!(valid_apple_sdk_target("arm64-apple-ios17.0-simulator", "/SDKs/iPhoneSimulator17.2.sdk"));
        assert!(!valid_apple_sdk_target("arm64-apple-macos14.0", "/SDKs/MacOSX27..0.sdk"));
        let digest="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let records=format!("# source_sha256={digest}\n_custom\tint custom(const char *name, size_t count);\n_empty\tvoid empty(void);\n");
        let parsed=parse_prototype_database(&records,digest).unwrap();
        let signature=parsed.get("_custom").unwrap();
        assert_eq!(signature.source,"hash-bound analyst C prototype catalog");
        assert_eq!(signature.declaration,"int custom(const char *name, size_t count);");
        let underscored = format!("# source_sha256={digest}\n__exit\tvoid _exit(int status);\n");
        assert!(parse_prototype_database(&underscored, digest).unwrap().contains_key("__exit"));
        let wrong_symbol = format!("# source_sha256={digest}\n__custom\tint custom(int value);\n");
        assert!(parse_prototype_database(&wrong_symbol, digest).is_err());
        let parameters=abi_parameters(Some(signature));
        assert_eq!(parameters.iter().map(|parameter|parameter.register.as_str()).collect::<Vec<_>>(),vec!["x0","x1"]);
        let (report,pseudocode)=recover_internal(&[0x1000],&[(0x1000,"_custom".into())],"",&[],0x1004,&parsed,None);
        assert!(report.contains("source=hash-bound analyst C prototype catalog declaration=int custom(const char *name, size_t count);"),"{report}");
        assert!(pseudocode.contains("int custom(const char *name, size_t count)"),"{pseudocode}");
        for invalid in [
            "_custom\tint custom(int value);\n".to_string(),
            format!("# source_sha256={}\n_custom\tint custom(int value);\n","f".repeat(64)),
            format!("# source_sha256={digest}\n_custom int custom(int value);\n"),
            format!("# source_sha256={digest}\n_other\tint custom(int value);\n"),
            format!("# source_sha256={digest}\n_custom\tint custom(int value) {{ return value; }}\n"),
            format!("# source_sha256={digest}\n_custom\tint custom(int value);\n_custom\tdouble custom(double value);\n"),
        ] {assert!(parse_prototype_database(&invalid,digest).is_err(),"{invalid}")}
    }
    #[test] fn project_type_catalog_rejects_conflicting_target_provenance() {
        let digest="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let declaration="typedef struct Widget { int value; } Widget;";
        let encoded=declaration.bytes().map(|byte|format!("{byte:02x}")).collect::<String>();
        let record=|target:&str|format!("# project_type\t{digest}\t{target}\tWidget\t{encoded}\t-\n");
        let same_target=format!("{}{}",record("arm64-apple-macos14.0"),record("arm64-apple-macos14.0"));
        assert!(parse_project_type_dependencies(&same_target,digest,"arm64").is_ok());
        let conflicting_target=format!("{}{}",record("arm64-apple-macos14.0"),record("aarch64-apple-ios17.0"));
        assert!(parse_project_type_dependencies(&conflicting_target,digest,"arm64").is_err());
        let other_declaration="typedef struct Widget { long value; } Widget;";
        let other_encoded=other_declaration.bytes().map(|byte|format!("{byte:02x}")).collect::<String>();
        let conflicting_declaration=format!("{}# project_type\t{digest}\tarm64-apple-macos14.0\tWidget\t{other_encoded}\t-\n",record("arm64-apple-macos14.0"));
        assert!(parse_project_type_dependencies(&conflicting_declaration,digest,"arm64").is_err());

        let declaration_hash=crate::reconstruction::sha256_hex_bytes(declaration.as_bytes()).unwrap();
        let layout=format!("{{\"schema\":1,\"producer\":\"hindsight-apple-clang-record-layout-v1\",\"target\":\"arm64-apple-macos14.0\",\"declarationsSHA256\":\"{declaration_hash}\",\"record\":{{\"name\":\"Widget\",\"kind\":\"struct\",\"size\":8,\"alignment\":4,\"fieldOffsetsComplete\":true,\"fields\":[{{\"name\":\"value\",\"declaration\":\"int value\",\"offsetBits\":0,\"bitWidth\":null}}]}}}}");
        assert!(layout_bindings(&layout).is_some());
        assert!(layout_bindings(&format!("{{\"nested\":{layout}}}")).unwrap().get("producer").is_none());
        assert!(layout_bindings(&format!("{{\"producer\":\"wrong\",\"producer\":\"hindsight-apple-clang-record-layout-v1\",\"target\":\"arm64-apple-macos14.0\",\"declarationsSHA256\":\"{declaration_hash}\"}}")).is_none());
        assert!(layout_bindings(&format!("not-json {layout}")).is_none());
        let layout_hex=layout.bytes().map(|byte|format!("{byte:02x}")).collect::<String>();
        let with_layout=format!("# source_sha256={digest}\n# project_type\t{digest}\tarm64-apple-macos14.0\tWidget\t{encoded}\t{layout_hex}\n");
        let parsed=parse_project_type_dependencies(&with_layout,digest,"arm64").unwrap();
        assert_eq!(parsed[0].layout_evidence.as_deref(),Some(layout.as_str()));
        assert_eq!(parsed[0].layout.as_ref().unwrap().fields[0].offset_bits,0);
        for invalid_layout in [layout.replace("\"offsetBits\":0","\"offsetBits\":64"),
            layout.replace("\"name\":\"Widget\",\"kind\"","\"name\":\"Other\",\"kind\"")] {
            let invalid_hex=invalid_layout.bytes().map(|byte|format!("{byte:02x}")).collect::<String>();
            let invalid_record=format!("# source_sha256={digest}\n# project_type\t{digest}\tarm64-apple-macos14.0\tWidget\t{encoded}\t{invalid_hex}\n");
            assert!(parse_project_type_dependencies(&invalid_record,digest,"arm64").is_err());
        }
        let wrong_hash=layout.replace(&declaration_hash,&"f".repeat(64));
        let wrong_hash_hex=wrong_hash.bytes().map(|byte|format!("{byte:02x}")).collect::<String>();
        let tampered=format!("# source_sha256={digest}\n# project_type\t{digest}\tarm64-apple-macos14.0\tWidget\t{encoded}\t{wrong_hash_hex}\n");
        assert!(parse_project_type_dependencies(&tampered,digest,"arm64").is_err());
    }
    #[test]
    fn project_type_catalog_accepts_nested_record_declaration_context() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let declaration = "struct Inner { int value; }; struct Outer { struct Inner inner; int tail; };";
        let encoded = declaration.bytes().map(|byte| format!("{byte:02x}")).collect::<String>();
        let declaration_hash = crate::reconstruction::sha256_hex_bytes(declaration.as_bytes()).unwrap();
        let layout = format!(
            "{{\"schema\":1,\"producer\":\"hindsight-apple-clang-record-layout-v1\",\"target\":\"arm64-apple-macos14.0\",\"declarationsSHA256\":\"{declaration_hash}\",\"record\":{{\"name\":\"Outer\",\"kind\":\"struct\",\"size\":8,\"alignment\":4,\"fieldOffsetsComplete\":true,\"fields\":[{{\"name\":\"inner\",\"declaration\":\"struct Inner inner\",\"offsetBits\":0,\"bitWidth\":null}},{{\"name\":\"tail\",\"declaration\":\"int tail\",\"offsetBits\":32,\"bitWidth\":null}}]}}}}"
        );
        let layout_hex = layout.bytes().map(|byte| format!("{byte:02x}")).collect::<String>();
        let catalog = format!(
            "# source_sha256={digest}\n# project_type\t{digest}\tarm64-apple-macos14.0\tOuter\t{encoded}\t{layout_hex}\n"
        );
        let parsed = parse_project_type_dependencies(&catalog, digest, "arm64").unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Outer");
        assert_eq!(parsed[0].declaration, declaration);
        let fields = &parsed[0].layout.as_ref().unwrap().fields;
        assert_eq!(fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(), ["inner", "tail"]);
    }
    #[test] fn typed_argument_casts_use_prototype_width_signedness_and_pointer_evidence() {
        for (value, ty, expected) in [
            (0xffff_ffff, "int", "(int)-1"), (0x1ffff, "short", "(short)-1"),
            (0x180, "signed char", "(signed char)-128"), (0x1ff, "uint8_t", "(uint8_t)0xff"),
            (0x1_0000_0001, "uint32_t", "(uint32_t)0x1"), (u64::MAX, "CFIndex", "(CFIndex)-1"),
            (0, "BOOL", "(BOOL)false"), (1, "bool", "(bool)true"),
            (2, "BOOL", "bitcast<BOOL>(0x2)"), (0, "id", "(id)0"),
            (0x1234, "const float *", "(const float *)0x1234"),
        ] {
            let (rendered, evidence) = typed_gp_argument(value, Some(ty));
            assert_eq!(rendered, expected);
            assert!(evidence.unwrap().contains(&format!("raw-register={value:#x}")));
        }
        assert_eq!(typed_gp_argument(0xffff_ffff, None), ("0xffffffff".into(), None));
        assert_eq!(typed_gp_argument(1, Some("UnknownAggregate")), ("0x1".into(), None));
        assert_eq!(abi_integer_kind("int /* pointer * is only a comment */"), Some(AbiIntegerKind::Signed));
        assert_eq!(abi_type("unsigned/* qualifier boundary */long"), Some(AbiType::Integer(8)));
        assert_eq!(abi_type("u/* token boundary */int64_t"), None);
        assert_eq!(abi_type("int */"), None);
        assert_eq!(abi_type("int /* unterminated"), None);
        assert_eq!(typed_gp_argument(0xffff_ffff, Some("int /* pointer * is only a comment */")).0,
            "(int /* pointer * is only a comment */)-1");
        let signature = Signature { language: "C", source: "test prototype", declaration: "void consume(int negative, uint8_t byte, void *pointer);".into() };
        let signatures = BTreeMap::from([(0x2000, ("_consume".into(), signature))]);
        let code = [0x12800000u32, 0x52803fe1, 0xd2800002].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x100c, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("consume((int)-1, (uint8_t)0xff, (void *)0);"), "{call}");
        assert!(call.contains("source-cast=signed<int>; low-bits=32; raw-register=0xffffffff"), "{call}");
        assert!(call.contains("proven_by=[0x1000]"), "{call}");
    }
    #[test] fn direct_calls_use_proven_stack_scalar_values_and_discard_aliased_stores() {
        let signature=Signature { language: "C", source: "test prototype", declaration:
            "void consume(long a,long b,long c,long d,long e,long f,long g,long h,int ninth);".into() };
        let signatures=BTreeMap::from([(0x2000,("_consume".into(),signature))]);
        let words=[0x5280_1fe9u32,0xb900_03e9]; // movz w9,#0xff; str w9,[sp]
        let code=words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call=render_linear_statement("call(0x2000);",0x1008,0x1000,&[],&signatures,None,"",&code,0x1000,false,&BTreeMap::new());
        assert!(call.starts_with("consume(x0, x1, x2, x3, x4, x5, x6, x7, (int)0xff);"),"{call}");
        assert!(call.contains("ninth(sp+0x0, 4 bytes)=0xff"),"{call}");
        assert!(call.contains("stack_store=0x1004"),"{call}");
        let packed=Signature { language: "C", source: "test prototype", declaration:
            "void packed(long a,long b,long c,long d,long e,long f,long g,long h,signed char byte,short half,int word);".into() };
        let packed_signatures=BTreeMap::from([(0x2000,("_packed".into(),packed))]);
        let packed_words=[0x5280_1fe9u32,0x3900_03e9,0x529f_ffea,0x7900_07ea,0x5280_1feb,0xb900_07eb];
        let packed_code=packed_words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let packed_call=render_linear_statement("call(0x2000);",0x1018,0x1000,&[],&packed_signatures,None,"",&packed_code,0x1000,false,&BTreeMap::new());
        assert!(packed_call.starts_with("packed(x0, x1, x2, x3, x4, x5, x6, x7, (signed char)-1, (short)-1, (int)0xff);"),"{packed_call}");
        for offset in ["sp+0x0, 1 bytes", "sp+0x2, 2 bytes", "sp+0x4, 4 bytes"] {
            assert!(packed_call.contains(offset),"{packed_call}");
        }
        for (barrier, description) in [(0xf900_0020u32,"possible alias"),(0xd100_43ff,"SP update"),(0x9400_0000,"prior call")] {
            let code=[words[0],words[1],barrier].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
            let call=render_linear_statement("call(0x2000);",0x100c,0x1000,&[],&signatures,None,"",&code,0x1000,false,&BTreeMap::new());
            assert!(call.contains("load_stack_argument<int>(sp + 0x0, 4)"),"{description}: {call}");
        }
    }
    #[test] fn stack_float_arguments_use_store_bits_and_reject_invalidated_values() {
        for (ty, materialize, store, literal) in [
            ("float", 0x1e2e_1008u32, 0xbd00_03e8u32, "1.0"),
            ("double", 0x1e7e_1008u32, 0xfd00_03e8u32, "-1.0"),
            ("float", 0x52a0_000a | (0x7fc0 << 5), 0xb900_03ea, "bitcast<float>(0x7fc00000)"),
            ("double", 0xd2e0_000a | (0x8000 << 5), 0xf900_03ea, "-0.0"),
        ] {
            let signature = Signature { language: "C", source: "test", declaration: format!(
                "void consume(double a,double b,double c,double d,double e,double f,double g,double h,{ty} ninth);") };
            let signatures = BTreeMap::from([(0x2000, ("_consume".into(), signature))]);
            for barrier in [None, Some(0xf900_0020u32), Some(0xd100_43ff), Some(0x9400_0000), Some(0xb900_03e9)] {
                let code = [materialize, store].into_iter().chain(barrier).flat_map(u32::to_le_bytes).collect::<Vec<_>>();
                let call = render_linear_statement("call(0x2000);", 0x1000 + code.len() as u64, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
                if barrier.is_none() {
                    assert!(call.starts_with(&format!("consume(d0, d1, d2, d3, d4, d5, d6, d7, {literal});")), "{call}");
                    assert!(call.contains("stack_store=0x1004"), "{call}");
                } else {
                    assert!(call.contains(&format!("load_stack_argument<{ty}>")), "{call}");
                }
            }
        }
    }
    #[test] fn stack_float_arguments_follow_only_intact_stack_pointer_copies() {
        let signature = Signature { language: "C", source: "test", declaration:
            "void consume(double a,double b,double c,double d,double e,double f,double g,double h,float ninth);".into() };
        let signatures = BTreeMap::from([(0x2000, ("_consume".into(), signature))]);
        for barrier in [None, Some(0xd280_0008u32), Some(0x5280_0008), Some(0xd100_43ff), Some(0x9400_0000)] {
            // mov x8,sp; optional clobber; fmov s8,#1; str s8,[x8]
            let code = [0x9100_03e8u32].into_iter().chain(barrier).chain([0x1e2e_1008, 0xbd00_0108]).flat_map(u32::to_le_bytes).collect::<Vec<_>>();
            let call = render_linear_statement("call(0x2000);", 0x1000 + code.len() as u64, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
            if barrier.is_none() {
                assert!(call.contains(", 1.0);"), "{call}");
                assert!(call.contains("base_proven_by=[0x1000]"), "{call}");
            } else {
                assert!(call.contains("load_stack_argument<float>"), "{call}");
            }
        }
    }
    #[test] fn protocol_prototypes_keep_scope_optionality_and_implementation_uncertainty() {
        let records = "  protocol 0x4000 name=First adopts=Parent,Tracing confidence=validated\n  required-instance method 0x4100  name=accept:  types=v24@0:8i16  imp=0x0 confidence=validated\n  optional-class method 0x4200  name=factory  types=@16@0:8  imp=0x9999 confidence=validated\n  protocol 0x5000 name=Second confidence=validated\n  required-instance method 0x5100  name=accept:  types=v24@0:8d16  imp=0x0 confidence=validated\n";
        let output = protocol_signatures(records);
        assert!(output.contains("protocol=First adopts=Parent,Tracing inheritance=validated membership=required-instance selector=accept:"), "{output}");
        assert!(output.contains("- (void)accept:(int)arg0;"), "{output}");
        assert!(output.contains("@optional\n+ (id)factory;"), "{output}");
        assert!(output.contains("protocol=Second") && output.contains("- (void)accept:(double)arg0;"));
        assert!(output.contains("protocol=First adopts=Parent,Tracing inheritance=validated"), "{output}");
        assert!(output.contains("@protocol First <Parent, Tracing>"), "{output}");
        assert!(output.contains("arg0=d0"));
        assert!(output.contains("implementation=unproven"));
        assert!(!output.contains("0x9999"));
        assert_eq!(protocol_signatures(&(records.to_string()+records)).matches("protocol=First adopts=Parent,Tracing inheritance=validated membership=required-instance").count(), 1);
    }
    #[test] fn protocol_prototypes_reject_unvalidated_unsupported_and_unscoped_records() {
        for records in [
            "  protocol 0x4000 name=Unknown confidence=candidate\n  required-instance method 0x4100  name=accept:  types=v24@0:8i16  imp=0x0 confidence=validated\n",
            "  required-instance method 0x4100  name=accept:  types=v24@0:8i16  imp=0x0 confidence=validated\n",
            "  protocol 0x4000 name=Known confidence=validated\n  required-instance method 0x4100  name=accept:  types=v24@0:8{Unknown=ii}16  imp=0x0 confidence=validated\n",
            "  protocol 0x4000 name=Known confidence=validated\n  required-instance method 0x4100  name=accept:  types=v24@0:8  imp=0x0 confidence=validated\n",
            "  protocol 0x4000 name=Known confidence=validated\ncategory 0x5000 name=Other confidence=validated\n  required-instance method 0x4100  name=accept:  types=v24@0:8i16  imp=0x0 confidence=validated\n",
        ] { assert!(protocol_signatures(records).is_empty(), "{records}"); }
    }
    #[test] fn class_conformance_declarations_preserve_protocol_sets_without_implementation_claims() {
        let records="reconstructed class 0x0000000000001000  name=Worker  super=0x0  ro=0x2000  conforms=Primary,Tracing\nreconstructed class 0x0000000000003000  name=Plain  super=0x1000  super_name=Worker  ro=0x4000\ncategory 0x5000 name=Extras class=0x1000 class_name=Worker conforms=Supplemental confidence=validated\n";
        let output=objc_conformance_declarations(&(records.to_string()+records));
        assert_eq!(output.matches("class=Worker conforms=Primary,Tracing").count(),1,"{output}");
        assert!(output.contains("@interface Worker <Primary, Tracing>"),"{output}");
        assert!(output.contains("implementation-coverage=unproven dispatch-targets=unresolved"),"{output}");
        assert_eq!(output.matches("class=Worker category=Extras conforms=Supplemental").count(),1,"{output}");
        assert!(output.contains("@interface Worker (Extras) <Supplemental>"),"{output}");
        assert!(output.contains("class=Plain inherits-conformance=Primary,Tracing via=Worker source=validated-superclass-chain"),"{output}");
        assert!(output.contains("@interface Plain : Worker /* inherited protocols: Primary, Tracing */"),"{output}");
        for invalid in ["reconstructed class 0x1  name=<bad>  conforms=Good\n","reconstructed class 0x1  name=Good  conforms=<bad>\n","category 0x1 name=Good conforms=Protocol\n"] {assert!(objc_conformance_declarations(invalid).is_empty(),"{invalid}")}
    }
    #[test] fn apple_scalar_abi_keeps_fp_and_gp_banks_independent() {
        let signature = Signature { language: "C", source: "test",
            declaration: "void mixed(double *pointer, double a, double b, double c, double d, double e, double f, double g, double h, float ninth, float tenth, int count);".into() };
        let args = abi_parameters(Some(&signature));
        assert_eq!(args[0].register, "x0");
        assert_eq!(args[1].register, "d0");
        assert_eq!(args[8].register, "d7");
        assert!(args[9].register.contains("sp+0x0, 4 bytes"));
        assert!(args[10].register.contains("sp+0x4, 4 bytes"));
        assert_eq!(args[11].register, "x1");
        assert_eq!(abi_type("const float *"), Some(AbiType::Integer(8)));
        assert_eq!(abi_type("volatile CGFloat"), Some(AbiType::Float(8)));
        assert_eq!(abi_type("long double"), Some(AbiType::Float(8)));
        assert_eq!(abi_type("CFStringEncoding"), Some(AbiType::Integer(4)));
    }
    #[test] fn apple_stack_arguments_are_naturally_packed_not_eight_byte_slots() {
        let signature = Signature { language: "C", source: "test",
            declaration: "void packed(long a,long b,long c,long d,long e,long f,long g,long h,char byte,short half,int word,double *pointer);".into() };
        let args = abi_parameters(Some(&signature));
        for (index, offset, size) in [(8,0,1), (9,2,2), (10,4,4), (11,8,8)] {
            assert!(args[index].register.contains(&format!("sp+0x{offset:x}, {size} bytes")), "{:?}", args[index]);
            assert_eq!(args[index].general_register, None);
        }
    }
    #[test] fn a_large_non_hfa_result_is_returned_through_the_x8_result_location() {
        // CGAffineTransform is 48 bytes of six doubles. It is not a
        // homogeneous floating-point aggregate of at most four members, so the
        // result comes back through the indirect result location in x8 and
        // the callee returns that pointer in x0.
        let signature = Signature { language: "C", source: "test",
            declaration: "CGAffineTransform affine(CGAffineTransform value);".into() };
        let rendered = render_signature_return(Some(&signature));
        assert!(rendered.starts_with("return /* sret result via hidden pointer in x8 for CGAffineTransform */;"), "{rendered}");
        // The result location is a dedicated register, so the argument
        // registers are not shifted. The composite argument is passed by
        // pointer: the caller copies it and passes its address in x0.
        let args = abi_parameters(Some(&signature));
        assert_eq!(args.len(), 1);
        assert!(args[0].register.contains("by-pointer aggregate<CGAffineTransform>(48 bytes) in x0"), "{:?}", args[0]);
    }
    #[test] fn a_homogeneous_aggregate_result_stays_in_the_registers() {
        // CGRect is 32 bytes, but it is a homogeneous floating-point aggregate
        // of four members, so it is returned in d0-d3 and never indirectly.
        let signature = Signature { language: "C", source: "test",
            declaration: "CGRect rect(CGRect value);".into() };
        assert!(render_signature_return(Some(&signature)).starts_with("return aggregate<CGRect>(d0, d1, d2, d3);"));
        // The argument is an HFA as well, so it travels in the
        // floating-point registers too.
        let args = abi_parameters(Some(&signature));
        assert!(args[0].register.contains("aggregate<CGRect>(d0, d1, d2, d3)"), "{:?}", args[0]);
    }
    #[test] fn the_indirect_result_location_does_not_shift_the_argument_registers() {
        // The result location is the dedicated x8 register, so self stays in
        // x0 and _cmd in x1 even when the result comes back indirectly.
        let objc = objc_method("-[View affine:]", "{CGAffineTransform=dddddd}64@0:8{CGAffineTransform=dddddd}16").unwrap();
        let args = abi_parameters(Some(&objc));
        assert_eq!(args[0].register, "x0"); // self
        assert_eq!(args[1].register, "x1"); // _cmd
        assert!(args[2].register.contains("by-pointer aggregate<CGAffineTransform>"), "{:?}", args[2]);
        assert!(render_signature_return(Some(&objc)).starts_with("return /* sret result via hidden pointer in x8 for CGAffineTransform */;"));
    }
    #[test] fn callbacks_blocks_and_array_parameters_decay_to_gp_pointers() {
        let signature = Signature { language: "C", source: "test",
            declaration: "void callbacks(void (*handler)(int, double), void (^block)(float), float values[4], double ratio);".into() };
        let args = abi_parameters(Some(&signature));
        assert_eq!(args.iter().map(|arg|arg.name.as_str()).collect::<Vec<_>>(), vec!["handler", "block", "values", "ratio"]);
        assert_eq!(args.iter().map(|arg|arg.register.as_str()).collect::<Vec<_>>(), vec!["x0", "x1", "x2", "d0"]);
    }
    #[test] fn unknown_aggregate_layout_and_cpp_receivers_withhold_abi_claims() {
        let signature = Signature { language: "C", source: "test", declaration: "void aggregate(int first, CustomRange range, double after);".into() };
        let args = abi_parameters(Some(&signature));
        assert_eq!(args[0].register, "x0");
        assert!(args[1].register.contains("type layout unresolved"));
        assert!(args[2].register.contains("earlier layout"));
        let nested = symbol("__ZN3Foo3barEi").unwrap();
        assert!(abi_parameters(Some(&nested))[0].register.contains("implicit C++ receiver"));
        let global = symbol("__Z3fooi").unwrap();
        assert_eq!(abi_parameters(Some(&global))[0].register, "x0");
        let wide = symbol("__Z3foonf").unwrap();
        let args = abi_parameters(Some(&wide));
        assert_eq!(args[0].register, "wide_integer<__int128>(x0, x1)");
        assert_eq!(args[1].register, "s0");
        let spilled = Signature { language: "C", source: "test", declaration: "void f(long a,long b,long c,long d,long e,long f,long g,long h,char byte,__int128 wide,long tail);".into() };
        let args = abi_parameters(Some(&spilled));
        assert!(args[9].register.contains("sp+0x10, 16 bytes"), "{:?}", args[9]);
        assert!(args[10].register.contains("sp+0x20"), "{:?}", args[10]);
        let boundary = Signature { language: "C", source: "test", declaration: "void f(long a,long b,long c,long d,long e,long f,long g,__int128 wide,long tail);".into() };
        let args = abi_parameters(Some(&boundary));
        assert!(args[7].register.contains("sp+0x0, 16 bytes"), "{:?}", args[7]);
        assert!(args[8].register.contains("sp+0x10"), "{:?}", args[8]);
        let signatures = BTreeMap::from([(0x2000, ("_f".into(), boundary))]);
        let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        assert!(call.contains("wide_integer_from_stack<__int128>(sp + 0x0)"), "{call}");
        let result = Signature { language: "C", source: "test", declaration: "unsigned __int128 f(void);".into() };
        assert!(render_signature_return(Some(&result)).starts_with("return wide_integer<unsigned __int128>(x0, x1);"));
    }
    #[test] fn known_sdk_aggregates_use_register_groups_and_spill_without_splitting() {
        let signature = symbol("_CFArrayContainsValue").unwrap();
        let args = abi_parameters(Some(&signature));
        assert_eq!(args[0].register, "x0");
        assert_eq!(args[1].register, "aggregate<CFRange>(x1, x2)");
        assert_eq!(args[2].register, "x3");
        let signature = Signature { language: "C", source: "test",
            declaration: "void ranges(long a,long b,long c,long d,long e,long f,long g,CFRange range,long tail);".into() };
        let args = abi_parameters(Some(&signature));
        assert!(args[7].register.contains("sp+0x0, 16 bytes"), "{:?}", args[7]);
        assert!(args[8].register.contains("sp+0x10, 8 bytes"), "{:?}", args[8]);
        let signature = Signature { language: "C", source: "test",
            declaration: "void hfa(double a,double b,double c,double d,double e,CGRect rectangle,double tail,int count);".into() };
        let args = abi_parameters(Some(&signature));
        assert!(args[5].register.contains("sp+0x0, 32 bytes"));
        assert!(args[6].register.contains("sp+0x20, 8 bytes"));
        assert_eq!(args[7].register, "x0");
        let objc = objc_method("-[View bounds]", "{CGRect={CGPoint=dd}{CGSize=dd}}16@0:8").unwrap();
        assert!(render_signature_return(Some(&objc)).starts_with("return aggregate<CGRect>(d0, d1, d2, d3);"));
        let range = objc_method("-[View range:]", "{_NSRange=QQ}32@0:8{_NSRange=QQ}16").unwrap();
        assert_eq!(abi_parameters(Some(&range))[2].register, "aggregate<NSRange>(x2, x3)");
        assert!(render_signature_return(Some(&range)).starts_with("return aggregate<NSRange>(x0, x1);"));
        assert_eq!(abi_type("CustomRange /* not a pointer */"), None);
    }
    #[test] fn spilled_large_aggregate_consumes_only_a_pointer_slot() {
        let signature = Signature { language: "C", source: "test", declaration: "void f(long a,long b,long c,long d,long e,long f,long g,long h,CGAffineTransform value,long tail);".into() };
        let args = abi_parameters(Some(&signature));
        assert!(args[8].register.contains("sp+0x0, 8-byte pointer to 48-byte"), "{:?}", args[8]);
        assert!(args[9].register.contains("sp+0x8"), "{:?}", args[9]);
        let signatures = BTreeMap::from([(0x2000, ("_f".into(), signature))]);
        let code = [0x528000e9u32, 0xf90007e9].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x1008, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
        assert!(call.contains("aggregate_from_pointer<CGAffineTransform>(load64(sp + 0x0)), (long)0x7"), "{call}");
    }
    #[test] fn indirect_aggregate_pointer_is_not_a_source_value_seed() {
        let signature = Signature { language: "C", source: "test", declaration: "void f(CGAffineTransform value, long next);".into() };
        let seeds = parameter_name_seeds(Some(&signature), "", 0);
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].name, "next");
        assert_eq!(seeds[0].register, 1);
    }
    #[test] fn indirect_aggregate_pointer_is_not_rendered_as_the_argument_value() {
        let signature = Signature { language: "C", source: "test prototype", declaration: "void consume(CGAffineTransform value, long next);".into() };
        let signatures = BTreeMap::from([(0x2000, ("_consume".into(), signature))]);
        let code = [0xd2802460u32, 0xd28000e1].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x1008, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("consume(aggregate_from_pointer<CGAffineTransform>(x0),"), "{call}");
        assert!(call.contains("(long)0x7"), "{call}");
        assert!(call.contains("by-pointer aggregate<CGAffineTransform>"), "{call}");
    }
    #[test] fn recovered_returns_select_scalar_fp_gp_void_and_unresolved_layouts() {
        for (ty, expected) in [("void", "return;"), ("void *", "return x0;"), ("double", "return d0;"),
            ("float", "return s0;"), ("BOOL", "return w0;"), ("uint64_t", "return x0;"), ("CustomRange", "return /* result ABI unresolved")] {
            let signature = Signature { language: "C", source: "test", declaration: format!("{ty} result(void);") };
            assert!(render_signature_return(Some(&signature)).starts_with(expected), "{ty}: {}", render_signature_return(Some(&signature)));
        }
        let objc = objc_method("-[NSNumber doubleValue]", "d16@0:8").unwrap();
        assert!(render_signature_return(Some(&objc)).starts_with("return d0;"));
        let pointer = Signature { language: "C", source: "test", declaration: "void *pointer(void);".into() };
        assert!(!signature_returns_void(Some(&pointer)));
        let code = 0xd65f03c0u32.to_le_bytes();
        let body = render_arm64_body(0x1000, 0x1004, "_pointer", Some(&pointer), &[], &BTreeMap::new(), &code, 0x1000, 0, &[], "", &[], "", &BTreeMap::new(),None).unwrap();
        assert!(body.contains("return x0;"), "{body}");
        assert!(!body.contains("void return supported"), "{body}");
    }
    #[test] fn calls_assign_proven_scalar_results() {
        for (ty, prefix) in [("int", "w0 = "), ("void *", "x0 = "), ("double", "d0 = "), ("float", "s0 = "), ("void", ""), ("Unknown", "")] {
            let signature = Signature { language: "C", source: "test", declaration: format!("{ty} produce(void);") };
            let signatures = BTreeMap::from([(0x2000, ("_produce".into(), signature))]);
            let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
            assert!(call.starts_with(&format!("{prefix}produce();")), "{call}");
        }
        assert_eq!(scalar_call_result(Some(&objc_method("-[NSNumber doubleValue]", "d16@0:8").unwrap())), Some("d0"));
    }
    #[test] fn calls_assign_known_aggregate_result_registers() {
        for (ty, expected) in [("NSRange", "(x0, x1)"), ("CGRect", "(d0, d1, d2, d3)"), ("__int128", "(x0, x1)")] {
            let signature = Signature { language:"C", source:"test", declaration:format!("{ty} make(void);") };
            assert_eq!(call_result_destination(Some(&signature)).as_deref(), Some(expected));
        }
        let indirect = Signature { language:"C", source:"test", declaration:"CGAffineTransform make(void);".into() };
        assert_eq!(call_result_destination(Some(&indirect)), None);
        let inputs = BTreeMap::from([("x0".into(),"owner".into()),("x1".into(),"count".into())]);
        assert_eq!(replace_instruction_register_names("(x0, x1) = make(x0, x1);", &inputs, Some(&BTreeMap::new())), "(x0, x1) = make(owner, count);");
    }
    #[test] fn calls_expose_hidden_result_buffer_without_shifting_arguments() {
        let signature = Signature { language:"C", source:"test", declaration:"CGAffineTransform make(long value);".into() };
        let signatures = BTreeMap::from([(0x2000,("_make".into(), signature))]);
        let call = render_linear_statement("call(0x2000);",0x1000,0x1000,&[],&signatures,None,"",&[],0x1000,false,&BTreeMap::new());
        assert!(call.starts_with("make(x0, /* hidden result buffer */ result_buffer<CGAffineTransform>(x8));"), "{call}");
        let branch=0x14000400u32.to_le_bytes();
        let tail=render_arm64_body(0x1000,0x1004,"_caller",None,&[],&signatures,&branch,0x1000,0,&[],"",&[(0x1000,0x1000,0x2000)],"",&BTreeMap::new(),None).unwrap();
        assert!(tail.contains("make(x0, /* hidden result buffer */ result_buffer<CGAffineTransform>(x8))"), "{tail}");
    }
    #[test] fn variadic_calls_show_fixed_prototype_and_unknown_anonymous_arguments() {
        let signature = symbol("_printf").unwrap();
        let signatures = BTreeMap::from([(0x2000, ("_printf".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("w0 = printf(x0, /* variadic arguments unresolved; Apple arm64 stack ABI */);"), "{call}");
        for (mangled, expected) in [
            ("__Z3fooiz", "foo(x0, /* variadic arguments unresolved; Apple arm64 stack ABI */);"),
            ("__Z3fooz", "foo(/* variadic arguments unresolved; Apple arm64 stack ABI */);")
        ] {
            let signatures = BTreeMap::from([(0x2000, (mangled.into(), symbol(mangled).unwrap()))]);
            let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
            assert!(call.starts_with(expected), "{call}");
        }
    }
    #[test] fn itanium_member_cv_and_reference_qualifiers_are_retained() {
        // These symbols were emitted by Apple clang for out-of-line methods.
        for (mangled, expected) in [
            ("__ZNK1A1fEi", "A::f(int arg0) const;"),
            ("__ZNVR1A1gEv", "A::g() volatile &;"),
            ("__ZNKO1A1hEv", "A::h() const &&;"),
        ] {
            let signature = symbol(mangled).unwrap();
            assert!(signature.declaration.ends_with(expected), "{}", signature.declaration);
            assert!(parameter_name_seeds(Some(&signature), "", 0).is_empty());
        }
        for malformed in ["__ZNKK1A1fEv", "__ZNKV1A1fEv", "__ZNR1fEv", "__ZNRO1A1fEv"] {
            assert!(symbol(malformed).is_none(), "{malformed}");
        }
    }
    #[test] fn itanium_constructors_and_destructors_retain_class_names() {
        for (mangled, expected) in [
            ("__ZN1AC1Ei", "A::A(int arg0);"),
            ("__ZN1AC2Ei", "A::A(int arg0);"),
            ("__ZN1AC3Ev", "A::A();"),
            ("__ZN1AD0Ev", "A::~A();"),
            ("__ZN1AD1Ev", "A::~A();"),
            ("__ZN1AD2Ev", "A::~A();"),
            ("__ZN2ns1AC1Ev", "ns::A::A();"),
        ] {
            let signature = symbol(mangled).unwrap();
            assert!(signature.declaration.ends_with(expected), "{}", signature.declaration);
            assert!(parameter_name_seeds(Some(&signature), "", 0).is_empty());
        }
        for malformed in ["__ZNC1Ev", "__ZN1AC4Ev", "__ZNK1AC1Ev", "__ZN1AD1Ei", "__ZN1AC11xEv"] {
            assert!(symbol(malformed).is_none(), "{malformed}");
        }
    }
    #[test] fn itanium_operator_names_are_recovered() {
        for (mangled, expected) in [
            ("__ZNK1AplEi", "A::operator+(int arg0) const;"),
            ("__ZN1AixEi", "A::operator[](int arg0);"),
            ("__Zpl1Ai", "operator+(A arg0, int arg1);"),
            ("__Znwm", "operator new(unsigned long arg0);"),
        ] {
            assert!(symbol(mangled).unwrap().declaration.ends_with(expected), "{mangled}");
        }
        for malformed in ["__ZNplEv", "__ZN1Apl1BEv", "__Zzzv"] {
            assert!(symbol(malformed).is_none(), "{malformed}");
        }
    }
    #[test] fn cpp_calls_keep_demangled_names_and_unresolved_receivers() {
        for (mangled, expected) in [
            ("__ZNK1AplEi", "A::operator+(/* possible implicit C++ receiver unresolved */, unresolved_argument"),
            ("__ZN1AD1Ev", "A::~A(/* possible implicit C++ receiver unresolved */)"),
            ("__Znwm", "operator new(x0)"),
        ] {
            let signatures = BTreeMap::from([(0x2000, (mangled.into(), symbol(mangled).unwrap()))]);
            let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
            assert!(call.starts_with(expected), "{call}");
        }
    }
    #[test] fn tail_calls_render_proven_register_and_stack_constants() {
        for result in ["void", "double"] {
            let signature = Signature { language: "C", source: "test", declaration: format!("{result} take(int value, double scale);") };
            let signatures = BTreeMap::from([(0x2000, ("_take".into(), signature))]);
            let code = [0x5280_0540u32, 0x1e6e_1000, 0x1400_03fe].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
            let body = render_arm64_body(0x1000, 0x100c, "_caller", None, &[], &signatures, &code, 0x1000, 0, &[], "", &[(0x1000,0x1008,0x2000)], "", &BTreeMap::new(), None).unwrap();
            assert!(body.contains("take((int)0x2a, 1.0);"), "{body}");
            assert!(body.contains("constant argument evidence:"), "{body}");
            assert!(body.contains("proven_by=[0x1000]"), "{body}");
            assert!(body.contains("proven_by=[0x1004]"), "{body}");
        }
        let signature = Signature { language: "C", source: "test", declaration: "double take(double a,double b,double c,double d,double e,double f,double g,double h,double i,double j);".into() };
        let signatures = BTreeMap::from([(0x2000, ("_take".into(), signature))]);
        let code = [0x1e6e_1008u32, 0x1e7e_1009, 0x6d00_27e8, 0x1400_03fd].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let body = render_arm64_body(0x1000, 0x1010, "_caller", None, &[], &signatures, &code, 0x1000, 0, &[], "", &[(0x1000,0x100c,0x2000)], "", &BTreeMap::new(), None).unwrap();
        assert!(body.contains("return take(d0, d1, d2, d3, d4, d5, d6, d7, 1.0, -1.0);"), "{body}");
        assert!(body.contains("stack_store=0x1008"), "{body}");
    }
    #[test] fn tail_calls_retain_receiver_and_variadic_uncertainty() {
        let code = 0x14000400u32.to_le_bytes(); // b 0x2000 from 0x1000
        for (mangled, expected) in [
            ("__ZNK1AplEi", "A::operator+(/* possible implicit C++ receiver unresolved */, unresolved_argument"),
            ("__Z3fooiz", "foo(x0, /* variadic arguments unresolved; Apple arm64 stack ABI */)"),
        ] {
            let signatures = BTreeMap::from([(0x2000, (mangled.into(), symbol(mangled).unwrap()))]);
            let body = render_arm64_body(0x1000, 0x1004, "_caller", None, &[], &signatures, &code, 0x1000, 0, &[], "", &[(0x1000,0x1000,0x2000)], "", &BTreeMap::new(), None).unwrap();
            assert!(body.contains(expected), "{body}");
            assert!(body.contains("tail-call candidate"), "{body}");
        }
    }
    #[test] fn tail_calls_render_indirect_aggregate_and_wide_stack_values() {
        let code = 0x14000400u32.to_le_bytes();
        for (declaration, expected) in [
            ("void take(CGAffineTransform value);", "take(aggregate_from_pointer<CGAffineTransform>(x0))"),
            ("void take(long a, long b, long c, long d, long e, long f, long g, long h, CGAffineTransform value);", "aggregate_from_pointer<CGAffineTransform>(load64(sp + 0x0))"),
            ("void take(long a, long b, long c, long d, long e, long f, long g, __int128 value);", "wide_integer_from_stack<__int128>(sp + 0x0)"),
        ] {
            let signature = Signature { language: "C", source: "test", declaration: declaration.into() };
            let signatures = BTreeMap::from([(0x2000, ("_take".into(), signature))]);
            let body = render_arm64_body(0x1000, 0x1004, "_caller", None, &[], &signatures, &code, 0x1000, 0, &[], "", &[(0x1000,0x1000,0x2000)], "", &BTreeMap::new(), None).unwrap();
            assert!(body.contains(expected), "{body}");
        }
    }
    #[test] fn stack_scalar_arguments_render_typed_loads_in_calls_and_tail_calls() {
        let signature = Signature { language: "C", source: "test", declaration: "void take(long a,long b,long c,long d,long e,long f,long g,long h,signed char i,short j,int k,double f0,double f1,double f2,double f3,double f4,double f5,double f6,double f7,float f8);".into() };
        let signatures = BTreeMap::from([(0x2000, ("_take".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        let branch = 0x14000400u32.to_le_bytes();
        let tail = render_arm64_body(0x1000, 0x1004, "_caller", None, &[], &signatures, &branch, 0x1000, 0, &[], "", &[(0x1000,0x1000,0x2000)], "", &BTreeMap::new(), None).unwrap();
        for output in [call, tail] {
            for expected in ["load_stack_argument<signed char>(sp + 0x0, 1)", "load_stack_argument<short>(sp + 0x2, 2)", "load_stack_argument<int>(sp + 0x4, 4)", "load_stack_argument<float>(sp + 0x8, 4)"] {
                assert!(output.contains(expected), "{output}");
            }
        }
    }
    #[test] fn spilled_aggregates_render_as_direct_stack_values() {
        let signature = Signature { language: "C", source: "test", declaration: "void take(long a,long b,long c,long d,long e,long f,long g,long h,NSRange range,double f0,double f1,double f2,double f3,double f4,double f5,double f6,double f7,CGPoint point,CGAffineTransform transform);".into() };
        let signatures = BTreeMap::from([(0x2000, ("_take".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        let branch = 0x14000400u32.to_le_bytes();
        let tail = render_arm64_body(0x1000, 0x1004, "_caller", None, &[], &signatures, &branch, 0x1000, 0, &[], "", &[(0x1000,0x1000,0x2000)], "", &BTreeMap::new(), None).unwrap();
        for output in [call, tail] {
            for expected in ["aggregate_from_stack<NSRange>(sp + 0x0, 16)", "aggregate_from_stack<CGPoint>(sp + 0x10, 16)", "aggregate_from_pointer<CGAffineTransform>(load64(sp + 0x20))"] {
                assert!(output.contains(expected), "{output}");
            }
        }
    }
    #[test] fn selector_arity_does_not_prove_argument_registers() {
        let selectors = BTreeMap::from([(0x1004, "setValue:".into())]);
        let code = 0xd28000e2u32.to_le_bytes(); // x2 = 7; selector might take a double in d0 instead
        let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &[], &BTreeMap::new(), None, "", &code, 0x1000, false, &selectors);
        assert!(call.contains("setValue:argument_0 /* ABI location unresolved */"), "{call}");
        assert!(!call.contains("setValue:0x7"), "{call}");
        let long = BTreeMap::from([(0x1004, "a:b:c:d:e:f:g:".into())]);
        let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &[], &BTreeMap::new(), None, "", &code, 0x1000, false, &long);
        assert!(call.contains("g:argument_6 /* ABI location unresolved */"), "{call}");
        for (selector, expected) in [("foo::", " :argument_1"), (":", " :argument_0")] {
            let selectors = BTreeMap::from([(0x1004, selector.into())]);
            let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &[], &BTreeMap::new(), None, "", &code, 0x1000, false, &selectors);
            assert!(call.contains(expected), "{call}");
        }
        let empty = BTreeMap::from([(0x1004, String::new())]);
        let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &[], &BTreeMap::new(), None, "", &code, 0x1000, false, &empty);
        assert!(!call.contains("objc_msgSend selector validated"), "{call}");
    }
    #[test] fn super_dispatch_keeps_its_structure_pointer_explicit() {
        for name in ["_objc_msgSendSuper", "_objc_msgSendSuper2"] {
            let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[(0x2000, name.into())], &BTreeMap::new(), None, "", &[], 0x1000, false, &BTreeMap::new());
            assert!(call.contains("(x0, x1, /* message arguments unresolved */)"), "{call}");
            assert!(call.contains("x0 points to objc_super"), "{call}");
            assert!(call.contains("direct call target 0x2000"), "{call}");
        }
        let other = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[(0x2000, "__objc_msgSendSuper".into())], &BTreeMap::new(), None, "", &[], 0x1000, false, &BTreeMap::new());
        assert!(!other.contains("x0 points to objc_super"), "{other}");
        let signature = Signature { language: "C", declaration: "id objc_msgSendSuper2(void *super, SEL selector);".into(), source: "test" };
        let signatures = BTreeMap::from([(0x2000, ("_objc_msgSendSuper2".into(), signature))]);
        let call = render_linear_statement("call(0x2000); /* indirect via x9; proven_by=[0x1000] */", 0x1004, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        assert!(call.contains("indirect target resolved through a register 0x2000"), "{call}");
        assert!(call.contains("indirect via x9; proven_by=[0x1000]"), "{call}");
        assert!(call.contains("indirect via x9; proven_by=[0x1000]"), "{call}");
    }
    #[test] fn abi_parameters_resolve_objc_hidden_arguments_and_fp_register_banks() {
        let objc = objc_method("-[RoadmapScorer score:]", "q24@0:8q16").unwrap();
        assert_eq!(abi_parameter_bindings(Some(&objc), &["x2".into()]), vec!["self=x0", "_cmd=x1", "arg0=x2"]);
        let foundation = objc_method("-[NSString substringFromIndex:]", "@\"NSString\"24@0:8Q16").unwrap();
        assert_eq!(abi_parameter_bindings(Some(&foundation), &["x2".into()]), vec!["self=x0", "_cmd=x1", "from=x2"]);

        let mixed = Signature { language: "C", declaration: "void mixed(uint64_t owner, double value, int count, float ratio);".into(), source: "test prototype" };
        let args = abi_parameters(Some(&mixed));
        assert_eq!(args.iter().map(|arg|arg.register.as_str()).collect::<Vec<_>>(), vec!["x0", "d0", "x1", "s1"]);
        assert_eq!(args.iter().map(|arg|arg.name.as_str()).collect::<Vec<_>>(), vec!["owner", "value", "count", "ratio"]);
        assert_eq!(render_fp_argument(b's', 0), "0.0");
        assert_eq!(render_fp_argument(b's', 0x8000_0000), "-0.0");
        assert_eq!(render_fp_argument(b'd', 0x7ff8_0000_0000_1234), "bitcast<double>(0x7ff8000000001234)");
    }
    fn names_fixture(words: &[u32], signature: &Signature) -> super::super::semantic::ParameterNameAnalysis {
        let code = words.iter().flat_map(|word|word.to_le_bytes()).collect::<Vec<_>>();
        super::super::semantic::parameter_value_names(&code,0x1000,0x1000,0x1000+code.len() as u64,0,&[],&parameter_name_seeds(Some(signature),"",0x1000))
    }
    #[test] fn parameter_names_flow_until_the_register_is_redefined() {
        let signature = objc_method("-[Widget setObject:]", "v24@0:8@16").unwrap();
        // mov x4,x2; ldr x2,[x0,#16]; mov x5,x4; mov x3,x2; ret.
        let names = names_fixture(&[0xaa0203e4,0xf9400802,0xaa0403e5,0xaa0203e3,0xd65f03c0],&signature);
        assert_eq!(names.outputs[&0x1000]["x4"],"arg0_copy_x4");
        assert_eq!(names.inputs[&0x1004]["x2"],"arg0");
        assert!(!names.outputs[&0x1004].contains_key("x2"));
        assert_eq!(names.outputs[&0x1008]["x5"],"arg0_copy_x5");
        assert!(!names.outputs[&0x100c].contains_key("x3"));
        let text=replace_instruction_register_names("x2 = *(x0 + 0x10);",&names.inputs[&0x1004],Some(&names.outputs[&0x1004]));
        assert_eq!(text,"x2 = *(self + 0x10);");
    }
    #[test] fn narrow_register_write_invalidates_the_wide_parameter_identity() {
        let signature = Signature { language: "C", source: "test prototype",
            declaration: "unsigned long overwrite(unsigned long original);".into() };
        // mov w0,#1 zero-extends and replaces the incoming 64-bit x0 value.
        let names = names_fixture(&[0x5280_0020, 0xd65f_03c0], &signature);
        assert_eq!(names.inputs[&0x1000]["x0"], "original");
        assert!(!names.outputs[&0x1000].contains_key("x0"));
        assert!(!names.outputs[&0x1000].contains_key("w0"));
        assert!(!names.inputs[&0x1004].contains_key("x0"));
    }
    #[test] fn xor_with_zero_register_is_an_exact_parameter_copy() {
        let signature = symbol("_CFRelease").unwrap();
        // eor x2,x0,xzr; ret.
        let names = names_fixture(&[0xca1f_0002, 0xd65f_03c0], &signature);
        assert_eq!(names.outputs[&0x1000]["x2"], "cf_copy_x2");
        let shifted = names_fixture(&[0xca5f_0c02, 0xd65f_03c0], &signature);
        assert_eq!(shifted.outputs[&0x1000]["x2"], "cf_copy_x2");

        // A real XOR is not an alias, even if one operand carries a parameter.
        let real_xor = names_fixture(&[0xca01_0002, 0xd65f_03c0], &Signature {
            language: "C", source: "test prototype",
            declaration: "void mix(unsigned long left, unsigned long right);".into(),
        });
        assert!(!real_xor.outputs[&0x1000].contains_key("x2"));
    }
    #[test] fn bic_zero_register_is_an_exact_parameter_copy() {
        let signature = symbol("_CFRelease").unwrap();
        // bic x2,x0,xzr is x0 & ~0, an exact 64-bit copy.
        let names = names_fixture(&[0x8a3f_0002, 0xd65f_03c0], &signature);
        assert_eq!(names.outputs[&0x1000]["x2"], "cf_copy_x2");
        let shifted = names_fixture(&[0x8a3f_0802, 0xd65f_03c0], &signature);
        assert_eq!(shifted.outputs[&0x1000]["x2"], "cf_copy_x2");

        // A nonzero mask still changes the value and cannot carry identity.
        let masked = names_fixture(&[0x8a21_0002, 0xd65f_03c0], &Signature {
            language: "C", source: "test prototype",
            declaration: "void mask(unsigned long value, unsigned long bits);".into(),
        });
        assert!(!masked.outputs[&0x1000].contains_key("x2"));
    }
    #[test] fn shifted_orr_of_zero_does_not_change_the_other_operand() {
        let signature = symbol("_CFRelease").unwrap();
        // orr x2,x0,xzr,lsl #2 remains x0.
        let copied = names_fixture(&[0xaa1f_0802, 0xd65f_03c0], &signature);
        assert_eq!(copied.outputs[&0x1000]["x2"], "cf_copy_x2");

        // Shifting the actual source changes the value and must drop its name.
        let shifted_source = names_fixture(&[0xaa00_0802, 0xd65f_03c0], &signature);
        assert!(!shifted_source.outputs[&0x1000].contains_key("x2"));
    }
    #[test] fn zero_distance_bitfield_shift_preserves_parameter_identity() {
        let signature = symbol("_CFRelease").unwrap();
        // UBFM x2,x0,#0,#63 is LSR x2,x0,#0, an exact copy.
        let copied = names_fixture(&[0xd340_fc02, 0xd65f_03c0], &signature);
        assert_eq!(copied.outputs[&0x1000]["x2"], "cf_copy_x2");
        let arithmetic_zero = names_fixture(&[0x9340_fc02, 0xd65f_03c0], &signature);
        assert_eq!(arithmetic_zero.outputs[&0x1000]["x2"], "cf_copy_x2");

        // LSR by one changes the value.
        let shifted = names_fixture(&[0xd341_fc02, 0xd65f_03c0], &signature);
        assert!(!shifted.outputs[&0x1000].contains_key("x2"));
    }
    #[test] fn shifted_add_sub_zero_register_forms_preserve_only_exact_copies() {
        let signature = symbol("_CFRelease").unwrap();
        for word in [0x8b1f_0002, 0x8b00_03e2, 0xcb1f_0002] {
            let names = names_fixture(&[word, 0xd65f_03c0], &signature);
            assert_eq!(names.outputs[&0x1000].get("x2").map(String::as_str), Some("cf_copy_x2"), "{word:#010x}");
        }
        // 0 - x0 is not a copy of x0.
        let negate = names_fixture(&[0xcb00_03e2, 0xd65f_03c0], &signature);
        assert!(!negate.outputs[&0x1000].contains_key("x2"));
    }
    #[test] fn modeled_frame_pointer_updates_preserve_incoming_argument_names() {
        let signature=Signature{language:"C",source:"test",declaration:"int sum(int left, int right);".into()};
        let names=names_fixture(&[0xd10043ff,0xb9000fe0,0xb9000be1,0xd65f03c0],&signature);
        assert_eq!(names.inputs[&0x1004]["w0"],"left");
        assert_eq!(names.inputs[&0x1008]["w1"],"right");
        assert!(!names.inputs[&0x1008].contains_key("x0")); // Unknown upper ABI bits.
    }
    #[test] fn parameter_aliases_merge_across_control_flow_and_stop_at_calls() {
        let signature=symbol("_CFRelease").unwrap();
        let branched=names_fixture(&[0xb4000041,0x14000002,0xd2800021,0xd65f03c0],&signature);
        assert_eq!(branched.inputs[&0x100c]["x0"],"cf");
        let conflicting=names_fixture(&[0xb4000061,0xaa0003e2,0x14000002,0xaa0103e2,0xd65f03c0],&signature);
        assert!(!conflicting.inputs[&0x1010].contains_key("x2"));
        let called=names_fixture(&[0x94000400,0xd65f03c0],&signature);
        assert_eq!(called.inputs[&0x1000]["x0"],"cf");
        assert!(!called.inputs[&0x1004].contains_key("x0"));
    }
    #[test] fn conditional_select_keeps_only_an_agreeing_parameter_identity() {
        let one = symbol("_CFRelease").unwrap();
        // mov x1,x0; csel x2,x0,x1,eq; ret. Either flag outcome selects the
        // same entry parameter, so the destination remains a proven alias.
        let agreeing = names_fixture(&[0xaa00_03e1, 0x9a81_0002, 0xd65f_03c0], &one);
        assert_eq!(agreeing.inputs[&0x1004]["x0"], "cf");
        assert_eq!(agreeing.inputs[&0x1004]["x1"], "cf_copy_x1");
        assert_eq!(agreeing.outputs[&0x1004]["x2"], "cf_copy_x2");

        let two = Signature { language: "C", source: "test prototype",
            declaration: "void choose(void *left, void *right);".into() };
        // csel x2,x0,x1,eq cannot preserve either input name when they differ.
        let conflicting = names_fixture(&[0x9a81_0002, 0xd65f_03c0], &two);
        assert!(!conflicting.outputs[&0x1000].contains_key("x2"));

        // The agreeing alternatives are still transformed by these variants.
        for transformed in [0x9a81_0402, 0xda81_0002, 0xda81_0402] {
            let names = names_fixture(&[0xaa00_03e1, transformed, 0xd65f_03c0], &one);
            assert!(!names.outputs[&0x1004].contains_key("x2"), "{transformed:#010x}");
        }
    }
    #[test] fn unresolved_indirect_transfers_keep_the_body_register_explicit() {
        let signature=symbol("_CFRelease").unwrap();
        let names=names_fixture(&[0xaa0003e2,0xd61f0200,0xd65f03c0],&signature);
        assert!(names.inputs.is_empty());
        assert_eq!(names.blocker,Some("unresolved indirect successors"));
    }
    #[test] fn join_merges_record_conflicting_names_as_evidence() {
        let signature=objc_method("-[Widget setObject:]","v24@0:8@16").unwrap();
        let names=names_fixture(&[0xb4000061,0xaa0003e2,0x14000002,0xaa0103e2,0xd65f03c0],&signature);
        assert!(!names.inputs[&0x1010].contains_key("x2"));
        let note=&names.notes[&0x1010];
        assert!(note.contains("x2 reaches with conflicting names"),"{note}");
        assert!(note.contains("self_copy_x2")&&note.contains("_cmd_copy_x2"),"{note}");
        let partly=names_fixture(&[0xb4000061,0x14000003,0xd503201f,0xaa0003e2,0xd65f03c0],&signature);
        assert!(partly.notes[&0x1010].contains("arg0 | self_copy_x2"));
        // Distinct copies on different arms are the same parameter identity.
        let agreeing=names_fixture(&[0xb4000061,0xaa0003e2,0x14000002,0xaa0003e2,0xd65f03c0],&signature);
        assert_eq!(agreeing.inputs[&0x1010]["x2"],"self_copy_x2");
        assert!(agreeing.notes.is_empty(),"{:?}",agreeing.notes);
    }
    #[test] fn unclassified_instruction_barriers_kill_names_but_decoded_comparisons_keep_them() {
        let signature=symbol("_CFRelease").unwrap();
        let barrier=names_fixture(&[0xaa0003e2,0xffffffff,0xd65f03c0],&signature);
        assert!(!barrier.inputs[&0x1008].contains_key("x2"));
        let compared=names_fixture(&[0xaa0003e2,0xeb01005f,0xd65f03c0],&signature);
        assert_eq!(compared.inputs[&0x1008]["x2"],"cf_copy_x2");
    }
    #[test] fn evidence_comments_keep_raw_registers_during_substitution() {
        let names = BTreeMap::from([("x0".to_string(), "value".to_string())]);
        let substituted = replace_instruction_register_names("x0 = *(x0 + 0x8); /* load through x0 */", &names, None);
        assert_eq!(substituted, "value = *(value + 0x8); /* load through x0 */");
        let names = BTreeMap::from([("x0".into(), "self".into())]);
        assert_eq!(replace_instruction_register_names("x0 = sp + 0x0;", &names, None), "self = sp + 0x0;");
        assert_eq!(replace_instruction_register_names("x0 = x0->x0 + object.x0;", &names, None), "self = self->x0 + object.x0;");
    }
    #[test] fn instruction_destinations_and_writeback_never_reuse_input_names() {
        let signature=Signature { language:"C",source:"test",declaration:"long load(long *owner, long value);".into() };
        let names=names_fixture(&[0xf8408c01,0xd65f03c0],&signature); // ldr x1,[x0,#8]!
        let rendered=replace_instruction_register_names("x0 = x0 + 0x8; x1 = *(x0);",&names.inputs[&0x1000],Some(&names.outputs[&0x1000]));
        assert_eq!(rendered,"x0 = owner + 0x8; x1 = *(x0);");
        let body_code=[0xa9400400u32,0xd65f03c0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(); // ldp x0,x1,[x0]
        let body=render_arm64_body(0x1000,0x1008,"_load",Some(&signature),&[],&BTreeMap::new(),&body_code,0x1000,0,&[],"",&[],"",&BTreeMap::new(),None).unwrap();
        assert!(body.contains("pair_addr_1000 = owner; x0 = *(pair_addr_1000); x1 = *(pair_addr_1000 + 0x8);"),"{body}");
        let inputs=BTreeMap::from([("x0".into(),"owner".into())]);
        assert_eq!(replace_instruction_register_names("x0(\"x0 = x0;\", x0); /* x0 */",&inputs,None),"x0(\"x0 = x0;\", owner); /* x0 */");
    }
    #[test] fn structured_loop_bodies_receive_recovered_names() {
        // CBZ X0, exit; ADD X2, X0, #4; B header; RET — a header-tested loop.
        let signature = Signature { language: "C", declaration: "long count(long value);".into(), source: "test prototype" };
        let code = [0xb400_0060u32, 0x9100_1002, 0x17ff_fffe, 0xd65f_03c0]
            .into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let body = render_arm64_body(0x1000, 0x1010, "_count", Some(&signature), &[], &BTreeMap::new(), &code, 0x1000, 0, &[], "", &[], "", &BTreeMap::new(),None).unwrap();
        assert!(body.contains("while (!(value == 0))"), "{body}");
        assert!(body.contains("0x0000000000001004: x2 = value + 0x4;"), "{body}");
        assert!(!body.contains("x0 + 0x4;"), "{body}");
    }
    #[test] fn scalar_fp_parameter_names_cover_single_and_double_register_views() {
        let signature = symbol("_CFNumberGetValue").unwrap();
        let analysis = names_fixture(&[0xd65f03c0], &signature);
        let names = &analysis.inputs[&0x1000];
        assert_eq!(names.get("x1").map(String::as_str), Some("theType"));
        let float_signature = Signature { language: "C", declaration: "double scale(float factor, double value);".into(), source: "test prototype" };
        let analysis = names_fixture(&[0xd65f03c0], &float_signature);
        let names = &analysis.inputs[&0x1000];
        assert_eq!(names.get("s0").map(String::as_str), Some("factor"));
        assert_eq!(names.get("d0").map(String::as_str), None);
        assert_eq!(names.get("d1").map(String::as_str), Some("value"));

        let same_fp = Signature { language: "C", source: "test prototype",
            declaration: "double select(double value);".into() };
        let selected = names_fixture(&[0x1e60_0c02, 0xd65f_03c0], &same_fp); // fcsel d2,d0,d0,eq
        assert_eq!(selected.outputs[&0x1000]["d2"], "value_copy_d2");
        let different_fp = Signature { language: "C", source: "test prototype",
            declaration: "double choose(double left, double right);".into() };
        let selected = names_fixture(&[0x1e61_0c02, 0xd65f_03c0], &different_fp); // fcsel d2,d0,d1,eq
        assert!(!selected.outputs[&0x1000].contains_key("d2"));
    }
    #[test] fn reciprocal_results_remain_unknown_while_unrelated_call_arguments_survive() {
        let code = [0x1e6e_1000, 0x1e6e_1001, 0x5ee1_d820, 0x9400_03fd]
            .into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let signature = Signature { language: "C", declaration: "void consume(double estimate, double input);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_consume".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x100c, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
        assert!(call.contains("consume(d0, 1.0);"), "{call}");
        assert!(!call.contains("estimate(d0)=1.0"), "{call}");
    }
    #[test] fn selected_fp_call_arguments_retain_comparison_provenance() {
        let words = [0x1e6e_1001u32, 0x1e60_1002, 0x1e62_2020, 0x1e62_bc20, 0x9400_03fc];
        let code = words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let signature = Signature { language: "C", declaration: "void consume(double value);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_consume".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x1010, 0x1000, &[], &signatures, None, "", &code, 0x1000, false, &BTreeMap::new());
        assert!(call.contains("consume(1.0);"), "{call}");
        assert!(call.contains("proven_by=[0x1000,0x1004,0x1008,0x100c]"), "{call}");
    }
    #[test] fn direct_calls_render_prototype_parameter_bindings() {
        let signature = symbol("_CFRelease").unwrap();
        let signatures = BTreeMap::from([(0x2000, ("_CFRelease".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("CFRelease(x0);"), "{call}");
        assert!(call.contains("argument bindings: cf=x0"), "{call}");
    }
    #[test] fn direct_calls_render_only_proven_fp_argument_constants() {
        let signature = Signature { language: "C", declaration: "void take_values(float ratio, double scale);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_take_values".into(), signature))]);
        let movi_v0_zero = 0x6f00_e400u32.to_le_bytes();
        let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &[], &signatures, None, "", &movi_v0_zero, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("take_values(0.0, d1);"), "{call}");
        assert!(call.contains("constant argument evidence: ratio(s0)=0.0 proven_by=[0x1000]"), "{call}");

        let overwritten = [0x6f00_e400u32, 0x1e202800u32].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x1008, 0x1000, &[], &signatures, None, "", &overwritten, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("take_values(s0, d1);"), "{call}");
        assert!(call.contains("no argument constants proven"), "{call}");
    }
    #[test] fn direct_calls_recover_fp_arguments_moved_from_integer_registers() {
        let signature = Signature { language: "C", declaration: "void take_value(double value);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_take_value".into(), signature))]);
        let instructions = [0xd280_0009u32, 0x9e67_0120u32].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x1008, 0x1000, &[], &signatures, None, "", &instructions, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("take_value(0.0);"), "{call}");
        assert!(call.contains("constant argument evidence: value(d0)=0.0 proven_by=[0x1000,0x1004]"), "{call}");

        let signature = Signature { language: "C", declaration: "void take_ratio(float ratio);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_take_ratio".into(), signature))]);
        let instructions = [0x5280_0009u32, 0x1e27_0120u32].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x1008, 0x1000, &[], &signatures, None, "", &instructions, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("take_ratio(0.0);"), "{call}");
        assert!(call.contains("constant argument evidence: ratio(s0)=0.0 proven_by=[0x1000,0x1004]"), "{call}");

        let signature = Signature { language: "C", declaration: "void take_copy(double spare, double value);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_take_copy".into(), signature))]);
        let instructions = [0xd280_0009u32, 0x9e67_0120u32, 0x1e60_4001u32].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x100c, 0x1000, &[], &signatures, None, "", &instructions, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("take_copy(0.0, 0.0);"), "{call}");
        assert!(call.contains("value(d1)=0.0 proven_by=[0x1000,0x1004,0x1008]"), "{call}");
    }
    #[test] fn direct_calls_recover_scalar_fmov_immediates() {
        let signature = Signature { language: "C", declaration: "void take_literals(float single, double wide);".into(), source: "test prototype" };
        let signatures = BTreeMap::from([(0x2000, ("_take_literals".into(), signature))]);
        let instructions = [0x1e2e_1000u32, 0x1e6e_1001u32].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let call = render_linear_statement("call(0x2000);", 0x1008, 0x1000, &[], &signatures, None, "", &instructions, 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("take_literals(1.0, 1.0);"), "{call}");
        assert!(call.contains("single(s0)=1.0 proven_by=[0x1000]"), "{call}");
        assert!(call.contains("wide(d1)=1.0 proven_by=[0x1004]"), "{call}");
    }
    #[test] fn resolved_indirect_calls_use_only_function_signature_evidence() {
        let signature = symbol("_CFRelease").unwrap();
        let signatures = BTreeMap::from([(0x2000, ("_CFRelease".into(), signature))]);
        let call = render_linear_statement(
            "call(0x2000); /* indirect via x9; proven_by=[0x1000]; signature, arguments, result and side effects unresolved */",
            0x1004, 0x1000, &[(0x2000, "_CFRelease_data_symbol".into())], &signatures,
            None, "", &[], 0x1000, false, &BTreeMap::new(),
        );
        assert!(call.starts_with("CFRelease(x0);"), "{call}");
        assert!(call.contains("indirect target resolved through a register 0x2000"), "{call}");
        let data_only = render_linear_statement(
            "call(0x3000); /* indirect via x9; proven_by=[0x1000] */",
            0x1004, 0x1000, &[(0x3000, "_CFRelease_data_symbol".into())], &BTreeMap::new(),
            None, "", &[], 0x1000, false, &BTreeMap::new(),
        );
        assert!(data_only.contains("unnamed target 0x3000"), "{data_only}");
        assert!(data_only.contains("indirect via x9; proven_by=[0x1000]"), "{data_only}");
    }
    #[test] fn direct_calls_use_candidate_arity_without_prototype() {
        assert_eq!(callable_name("__exit", None), "_exit");
        let symbols = [(0x2000, "_consume".into())];
        let evidence = "function 0x0000000000002000 _consume parameter-registers=[x0, x2] highest-abi-position=3 classification=incoming-parameter-candidate\n";
        let call = render_linear_statement("call(0x2000);", 0x1004, 0x1000, &symbols, &BTreeMap::new(), None, evidence, &[], 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("consume(x0, x1, x2);"), "{call}");
        assert!(call.contains("arity candidate from callee entry-register uses"), "{call}");
        assert!(call.contains("candidate ABI argument bindings: arg0=x0, arg1=x1, arg2=x2; callee types unresolved"), "{call}");
    }
    #[test] fn joins_method_encoding_and_cf_stub_with_direct_call() {
        let symbols=[(0x2000,"_CFRelease".into())];
        let objc="  instance method 0x3000  name=x  types=v24@0:8@16  imp=0x0000000000001000 resolved=-[Widget setObject:] confidence=validated\n";
        assert!(objc_method("-[Widget setObject:]","v24@0:8@16").is_some());
        let (report,pseudo)=recover(&[0x1000],&symbols,objc,&[(0x1000,0x1004,0x2000)],0x1100);
        assert!(report.contains("- (void)setObject:(id)arg0;"),"{report}");
        assert!(report.contains("void CFRelease(CFTypeRef cf);"));
        assert!(pseudo.contains("call_target(0x0000000000002000);"));
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;
    #[test]
    fn debug_join() {
        let signature = objc_method("-[Widget setObject:]", "v24@0:8@16").unwrap();
        let code = [0xb4000061u32,0xaa0003e2,0x14000002,0xaa0103e2,0xd65f03c0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let analysis = super::super::semantic::parameter_value_names(&code,0x1000,0x1000,0x1014,0,&[],&parameter_name_seeds(Some(&signature),"",0x1000));
        for (pc, map) in &analysis.inputs { eprintln!("{pc:#x}: {map:?}"); }
        eprintln!("notes: {:?}", analysis.notes);
        assert!(analysis.notes.contains_key(&0x1010));
    }
}
