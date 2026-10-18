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

fn parse_prototype_database(contents: &str, source_sha256: &str) -> Result<BTreeMap<String,Signature>, &'static str> {
    if contents.len()>4*1024*1024||contents.lines().count()>20_000{return Err("External prototype catalog exceeds byte/record budget")}
    let mut bound=false;let mut result=BTreeMap::new();
    for line in contents.lines() {
        if let Some(expected)=line.strip_prefix("# source_sha256="){if expected.len()!=64||!expected.bytes().all(|byte|byte.is_ascii_hexdigit())||!expected.eq_ignore_ascii_case(source_sha256){return Err("External prototype catalog source SHA-256 mismatch")}bound=true;continue}
        let line=line.trim();if line.is_empty()||line.starts_with('#'){continue}
        if line.len()>2048{return Err("External prototype catalog record exceeds length budget")}
        let Some((symbol,declaration))=line.split_once('\t')else{return Err("External prototype records require a tab between symbol and declaration")};
        let symbol=symbol.trim();let declaration=declaration.trim();
        if !clean(symbol)||!declaration.ends_with(';')||declaration.len()>1024||declaration.chars().any(|ch|ch.is_control()||matches!(ch,'\\'|'{'|'}'|'#'|'='|'"')){return Err("External prototype catalog contains an unsafe declaration")}
        let Some((head,arguments))=declaration[..declaration.len()-1].split_once('(')else{return Err("External prototype declaration is malformed")};
        if !arguments.ends_with(')')||arguments[..arguments.len()-1].contains(['(',')']){return Err("External prototype declaration has unsupported nesting")}
        let declared=head.split_whitespace().last().map(|name|name.trim_start_matches('*')).unwrap_or("");
        if declared.is_empty()||!declared.bytes().all(|byte|byte.is_ascii_alphanumeric()||byte==b'_')||symbol.trim_start_matches('_')!=declared{return Err("External prototype symbol and declaration name differ")}
        result.insert(symbol.to_owned(),Signature{language:"C",declaration:declaration.to_owned(),source:"hash-bound analyst C prototype catalog"});
    }
    if !bound{return Err("External prototype catalog requires source_sha256 binding")}Ok(result)
}

pub fn external_prototype_database(source_sha256:&str)->io::Result<BTreeMap<String,Signature>>{
    let Some(path)=std::env::var_os("HINDSIGHT_PROTOTYPE_DB")else{return Ok(BTreeMap::new())};let path=std::path::PathBuf::from(path);
    if !path.is_absolute()||path.to_string_lossy().len()>16*1024{return Err(io::Error::new(io::ErrorKind::InvalidInput,"HINDSIGHT_PROTOTYPE_DB must be an absolute path"))}
    let mut contents=String::new();std::fs::File::open(path)?.take(4*1024*1024+1).read_to_string(&mut contents)?;
    parse_prototype_database(&contents,source_sha256).map_err(|message|io::Error::new(io::ErrorKind::InvalidData,message))
}

fn clean(text: &str) -> bool {
    !text.is_empty() && text.len() <= 1024 && !text.chars().any(|c| c.is_control() || matches!(c, '\\' | ';' | '{' | '}'))
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
                if !clean(name) { return None } format!("{name} *")
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

pub fn objc_method(name: &str, encoding: &str) -> Option<Signature> {
    if !clean(name) || encoding.is_empty() || encoding.len() > 1024 || !encoding.is_ascii() || encoding.bytes().any(|b| b.is_ascii_control()) || !(name.starts_with("-[") || name.starts_with("+[")) || !name.ends_with(']') { return None }
    let bytes = encoding.as_bytes(); let mut at = 0;
    let result = objc_type(bytes, &mut at, 0)?; skip_number(bytes, &mut at);
    let self_type = objc_type(bytes, &mut at, 0)?; skip_number(bytes, &mut at);
    let selector_type = objc_type(bytes, &mut at, 0)?; skip_number(bytes, &mut at);
    if !matches!(self_type.as_str(), "id" | "Class") || selector_type != "SEL" { return None }
    let body = &name[2..name.len()-1]; let (_, selector) = body.split_once(' ')?;
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

/// A class RO protocol list proves a declared conformance. It does not prove
/// that any required method has a local implementation or resolve dispatch.
pub fn objc_conformance_declarations(records: &str) -> String {
    let valid=|value:&str|!value.is_empty()&&value.len()<=256&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'$'|b'.'));
    let mut seen=std::collections::BTreeSet::new();let mut output=String::new();
    for line in records.lines().take(8192) {
        let line=line.trim();if line.len()>4096||!line.starts_with("reconstructed class "){continue}
        let Some(name)=line.split("  name=").nth(1).and_then(|tail|tail.split_whitespace().next())else{continue};
        let Some(protocols)=line.split_whitespace().find_map(|field|field.strip_prefix("conforms="))else{continue};
        if !valid(name){continue}let conformances=protocols.split(',').filter(|protocol|valid(protocol)).take(32).collect::<Vec<_>>();
        if conformances.is_empty(){continue}let key=(name.to_owned(),conformances.join(","));if !seen.insert(key){continue}
        let row=format!("class={name} conforms={} source=validated-class-ro-protocol-list implementation-coverage=unproven dispatch-targets=unresolved\n@interface {name} <{}>\n@end\n",conformances.join(","),conformances.join(", "));
        if output.len()+row.len()>12_000||seen.len()>128{output.push_str("Objective-C conformance output truncated at declaration/byte budget.\n");break}output.push_str(&row);
    }
    if !output.is_empty(){output.insert_str(0,"Objective-C class conformance evidence; conformance does not establish method implementations.\n")}output
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
        b'f' => "float".into(), b'd' => "double".into(),
        b'P' => format!("{} *", cpp_type(bytes, at, depth + 1)?),
        b'R' => format!("{} &", cpp_type(bytes, at, depth + 1)?),
        b'O' => format!("{} &&", cpp_type(bytes, at, depth + 1)?),
        b'K' => format!("const {}", cpp_type(bytes, at, depth + 1)?),
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

pub fn symbol(name: &str) -> Option<Signature> {
    if !clean(name) { return None }
    if let Some((declaration, language)) = api(name) {
        return Some(Signature { language, declaration: declaration.into(), source: "exact SDK ABI catalog" });
    }
    let mangled = name.strip_prefix("__Z").or_else(|| name.strip_prefix("_Z"))?;
    let bytes = mangled.as_bytes(); let mut at = 0; let mut names = Vec::new();
    if bytes.first() == Some(&b'N') {
        at += 1;
        while bytes.get(at) != Some(&b'E') { names.push(cpp_component(bytes, &mut at)?); if names.len() > 16 { return None } }
        at += 1;
    } else { names.push(cpp_component(bytes, &mut at)?); }
    let mut args = Vec::new();
    if bytes.get(at) == Some(&b'v') && at + 1 == bytes.len() { at += 1; }
    else { while at < bytes.len() { args.push(cpp_type(bytes, &mut at, 0)?); if args.len() > 32 { return None } } }
    if at != bytes.len() { return None }
    let declaration = format!("/* return type unresolved */ {}({});", names.join("::"), args.iter().enumerate().map(|(i,t)| format!("{t} arg{i}")).collect::<Vec<_>>().join(", "));
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
}

#[allow(clippy::too_many_arguments)]
pub fn recover_arm64(
    starts: &[u64], symbols: &[(u64, String)], objc_records: &str,
    calls: &[(u64,u64,u64)], scan_end: u64, code: &[u8], base: u64,
    file_offset: u64, data_ranges: &[(u64,u64,u16)], parameter_evidence: &str,
    tail_calls: &[(u64,u64,u64)], jump_tables: &str, objc_selectors: &BTreeMap<u64,String>, external_prototypes:&BTreeMap<String,Signature>,
) -> (String, String) {
    recover_internal(starts, symbols, objc_records, calls, scan_end, external_prototypes,
        Some(Arm64RenderContext { code, base, file_offset, data_ranges, parameter_evidence, tail_calls, jump_tables, objc_selectors }))
}

#[derive(Default)]
struct SwitchEvidence { branch_register: u8, index_register: Option<u8>, cases: Vec<(u64,u64)> }

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
            if let Some(evidence)=result.get_mut(&address){evidence.index_register=Some(register);}
        }
        if line.trim_start().starts_with("case-index=") {
            let case=line.split("case-index=").nth(1).and_then(|s|s.split_whitespace().next()).and_then(|s|s.parse::<u64>().ok());
            let target=line.split("target=").nth(1).and_then(|s|s.split_whitespace().next()).and_then(|s|u64::from_str_radix(s.trim_start_matches("0x"),16).ok());
            if let (Some(case),Some(target))=(case,target){if let Some(evidence)=result.get_mut(&address){evidence.cases.push((case,target));}}
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
        if let Some((head, _)) = sig.declaration.split_once('(') {
            let candidate = head.trim().trim_end_matches('*').split_whitespace().last().unwrap_or("");
            if !candidate.is_empty() && candidate.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b':') {
                return candidate.into();
            }
        }
    }
    let mut candidate = name.strip_prefix('_').unwrap_or(name);
    if let Some(stripped) = candidate.strip_prefix('_') { candidate = stripped; }
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
    // Parameter arrays decay to pointers. Preserve the declarator's source name.
    if let Some(open) = value.find('[') {
        let head = value[..open].trim_end();
        if let Some(split) = head.rfind(char::is_whitespace) {
            let name = head[split..].trim();
            if name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return (format!("{} *", head[..split].trim()), name.into());
            }
        }
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
enum AbiType { Integer(u8), Float(u8), IntegerAggregate(u8), FloatAggregate(u8) }

/// Apple arm64 LP64 scalar ABI. Unknown typedefs/aggregates do not receive
/// guessed locations: their layout can change both banks and the stack cursor.
fn uncomment_type(ty: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut rest = ty;
    while let Some((head, tail)) = rest.split_once("/*") {
        normalized.push_str(head);
        rest = tail.split_once("*/")?.1;
    }
    normalized.push_str(rest);
    Some(normalized)
}

fn abi_type(ty: &str) -> Option<AbiType> {
    let normalized = uncomment_type(ty)?;
    let ty = normalized.trim();
    if ty.contains('{') || ty.contains('}') || ty.contains('[') { return None; }
    if ty.contains('*') || ty.contains('&') || ty.contains("(^") { return Some(AbiType::Integer(8)); }
    let ty = ty.split_whitespace().filter(|word| !matches!(*word,
        "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Nonnull" | "_Nullable" | "__kindof"
    )).collect::<Vec<_>>().join(" ");
    Some(match ty.as_str() {
        "CFRange" | "NSRange" => AbiType::IntegerAggregate(2),
        "CGPoint" | "CGSize" => AbiType::FloatAggregate(2), "CGRect" => AbiType::FloatAggregate(4),
        "float" => AbiType::Float(4), "double" | "long double" | "CGFloat" => AbiType::Float(8),
        "bool" | "_Bool" | "BOOL" | "Boolean" | "char" | "signed char" | "unsigned char" | "int8_t" | "uint8_t" | "Int8" | "UInt8" => AbiType::Integer(1),
        "short" | "short int" | "signed short" | "unsigned short" | "unsigned short int" | "int16_t" | "uint16_t" | "Int16" | "UInt16" | "unichar" => AbiType::Integer(2),
        "int" | "signed" | "signed int" | "unsigned" | "unsigned int" | "int32_t" | "uint32_t" | "Int32" | "UInt32" | "CFStringEncoding" => AbiType::Integer(4),
        "long" | "long int" | "unsigned long" | "unsigned long int" | "long long" | "long long int" | "unsigned long long" | "unsigned long long int" |
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
    let pointer = normalized.contains('*') || normalized.contains('&') || normalized.contains("(^") || matches!(simple.as_str(),
        "id" | "Class" | "SEL" | "instancetype" | "dispatch_queue_t" | "dispatch_block_t" |
        "CFTypeRef" | "CFAllocatorRef" | "CFStringRef" | "CFArrayRef" | "CFDictionaryRef" | "CFBooleanRef" | "CFNumberRef" | "CFDataRef" |
        "CFBundleRef" | "CFURLRef" | "CFPropertyListRef" | "CFErrorRef" | "CFRunLoopRef");
    if pointer { return Some(AbiIntegerKind::Pointer); }
    if matches!(simple.as_str(), "bool" | "_Bool" | "BOOL") { return Some(AbiIntegerKind::Boolean); }
    let signed = matches!(simple.as_str(), "char" | "signed char" | "short" | "short int" | "signed short" |
        "int" | "signed" | "signed int" | "long" | "long int" | "long long" | "long long int" |
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
    let mut gp = if is_objc { 2u8 } else { 0u8 };
    let mut fp = 0u8;
    let mut stack_slot = 0usize;
    let mut stack_offset = 0usize;
    let mut blocked = signature.language == "C++ / Objective-C++" && signature.declaration.contains("::");
    let mut result = Vec::new();
    if is_objc {
        result.push(AbiParameter { name: "self".into(), register: "x0".into(), general_register: Some(0), stack_scalar: None, source_type: Some("id".into()) });
        result.push(AbiParameter { name: "_cmd".into(), register: "x1".into(), general_register: Some(1), stack_scalar: None, source_type: Some("SEL".into()) });
    }
    for (ty, name) in parameters {
        let scalar = abi_type(&ty);
        if blocked || scalar.is_none() {
            let reason = if blocked { "earlier layout or implicit C++ receiver unresolved" } else { "type layout unresolved" };
            result.push(AbiParameter { name, register: format!("unresolved_argument /* {reason}: {ty} */"), general_register: None, stack_scalar: None, source_type: Some(ty) });
            blocked = true;
            continue;
        }
        let scalar = scalar.unwrap_or(AbiType::Integer(8));
        if let AbiType::IntegerAggregate(words) | AbiType::FloatAggregate(words) = scalar {
            let (bank, prefix) = if matches!(scalar, AbiType::FloatAggregate(_)) { (&mut fp, "d") } else { (&mut gp, "x") };
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
                result.push(AbiParameter { name, register: format!("stack argument (slot {stack_slot}, sp+0x{stack_offset:x}, {bytes} bytes; Apple arm64 SDK aggregate {ty})"), general_register: None, stack_scalar: None, source_type: Some(ty) });
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

fn render_signature_return(signature: Option<&Signature>) -> String {
    let Some(ty) = signature.and_then(signature_return_type) else {
        return "return x0; /* inferred ABI result; source return type unresolved */".into();
    };
    if ty == "void" { return "return; /* void return supported by recovered signature */".into(); }
    let register = match abi_type(&ty) {
        Some(AbiType::Float(4)) => "s0", Some(AbiType::Float(_)) => "d0",
        Some(AbiType::Integer(bytes)) if bytes <= 4 => "w0", Some(AbiType::Integer(_)) => "x0",
        Some(AbiType::IntegerAggregate(words) | AbiType::FloatAggregate(words)) => {
            let prefix = if matches!(abi_type(&ty), Some(AbiType::FloatAggregate(_))) { "d" } else { "x" };
            let registers = (0..words).map(|index| format!("{prefix}{index}")).collect::<Vec<_>>().join(", ");
            return format!("return aggregate<{ty}>({registers}); /* Apple arm64 known SDK aggregate result */");
        }
        None => return format!("return /* result ABI unresolved for {ty}; aggregate/HFA/indirect-result layout required */;"),
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

fn replace_register_names(statement: &str, names: &BTreeMap<String, String>) -> String {
    let bytes = statement.as_bytes();
    let mut output = String::with_capacity(statement.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_') { cursor += 1; }
            let token = &statement[start..cursor];
            output.push_str(names.get(token).map(String::as_str).unwrap_or(token));
        } else {
            let ch = statement[cursor..].chars().next().unwrap();
            output.push(ch);
            cursor += ch.len_utf8();
        }
    }
    output
}

fn assigned_register(statement: &str) -> Option<String> {
    let lhs = statement.split_once('=')?.0.trim();
    let register = lhs.split_whitespace().last()?;
    if register.len() >= 2 && matches!(register.as_bytes()[0], b'x' | b'w' | b's' | b'd' | b'v')
        && register[1..].bytes().all(|byte| byte.is_ascii_digit()) {
        Some(register.to_string())
    } else { None }
}

fn copied_register(statement: &str) -> Option<String> {
    let (_, rhs) = statement.split_once('=')?;
    let source = rhs.split_once(';').map_or(rhs, |(expression, _)| expression).trim();
    let bytes = source.as_bytes();
    if bytes.len() < 2 || !matches!(bytes[0], b'x' | b'w' | b's' | b'd' | b'v')
        || !bytes[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(source.to_string())
}

fn register_bank(register: &str) -> Option<u8> {
    match register.as_bytes().first()? {
        b'x' | b'w' => Some(0),
        b's' | b'd' | b'v' => Some(1),
        _ => None,
    }
}

fn bind_register_name(names: &mut BTreeMap<String, String>, destination: &str, name: String) {
    if let Some(index) = destination.strip_prefix('x').or_else(|| destination.strip_prefix('w')) {
        names.insert(format!("x{index}"), name.clone());
        names.insert(format!("w{index}"), name);
    } else if let Some(index) = destination.strip_prefix('s').or_else(|| destination.strip_prefix('d')).or_else(|| destination.strip_prefix('v')) {
        names.insert(format!("s{index}"), name.clone());
        names.insert(format!("d{index}"), name.clone());
        names.insert(format!("v{index}"), name);
    }
}

fn parameter_names(signature: Option<&Signature>, report: &str, address: u64) -> BTreeMap<String, String> {
    let parameters = signature.map(|signature| abi_parameters(Some(signature))).unwrap_or_else(|| {
        incoming_parameter_candidates(report, address).into_iter().filter_map(|register| {
            let index = register.strip_prefix('x')?.parse::<u8>().ok()?;
            (index < 8).then(|| AbiParameter { name: format!("arg{index}"), register: format!("x{index}"), general_register: Some(index), stack_scalar: None, source_type: None })
        }).collect()
    });
    let mut names = BTreeMap::new();
    for parameter in parameters {
        if let Some(index) = parameter.general_register {
            names.insert(format!("x{index}"), parameter.name.clone());
            names.insert(format!("w{index}"), parameter.name);
        } else if let Some(index) = parameter.register.strip_prefix('s').or_else(|| parameter.register.strip_prefix('d')).and_then(|value| value.parse::<u8>().ok()) {
            names.insert(format!("s{index}"), parameter.name.clone());
            names.insert(format!("d{index}"), parameter.name.clone());
            names.insert(format!("v{index}"), parameter.name);
        } else {
            names.insert(parameter.register, parameter.name);
        }
    }
    names
}

fn parameter_aliases_by_instruction(
    instructions: &[(u64, String)], signature: Option<&Signature>, report: &str, address: u64,
) -> BTreeMap<u64, BTreeMap<String, String>> {
    // Register names have a single source-level meaning only along a straight
    // line. At joins and loop headers we keep the machine registers explicit.
    if instructions.iter().any(|(_, statement)| statement.starts_with("if (") || statement.starts_with("goto ")) {
        return BTreeMap::new();
    }
    let mut names = parameter_names(signature, report, address);
    let mut result = BTreeMap::new();
    for (pc, statement) in instructions {
        let copied_name = assigned_register(statement).and_then(|destination| {
            copied_register(statement).and_then(|source| {
                (register_bank(&source) == register_bank(&destination))
                    .then(|| names.get(&source).map(|source_name| {
                        let physical_destination = destination.strip_prefix('w')
                            .map(|index| format!("x{index}"))
                            .unwrap_or_else(|| destination.clone());
                        format!("{source_name}_copy_{physical_destination}")
                    })).flatten()
            }).map(|name| (destination, name))
        });
        let mut instruction_names = names.clone();
        if let Some((destination, copied_name)) = &copied_name {
            bind_register_name(&mut instruction_names, destination, copied_name.clone());
        }
        result.insert(*pc, instruction_names);
        if statement.starts_with("call(") {
            names.clear(); // AAPCS64 caller-saved registers may be clobbered.
        } else if let Some(destination) = assigned_register(statement) {
            if let Some(index) = destination.strip_prefix('x').or_else(|| destination.strip_prefix('w')) {
                names.remove(&format!("x{index}")); names.remove(&format!("w{index}"));
            } else if let Some(index) = destination.strip_prefix('s').or_else(|| destination.strip_prefix('d')).or_else(|| destination.strip_prefix('v')) {
                names.remove(&format!("s{index}")); names.remove(&format!("d{index}")); names.remove(&format!("v{index}"));
            } else { names.remove(&destination); }
            if let Some((_, name)) = copied_name { bind_register_name(&mut names, &destination, name); }
        }
    }
    result
}

fn substitute_parameter_names(statement: &str, names: &BTreeMap<String, String>) -> String {
    replace_register_names(statement, names)
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
                let known=super::semantic::known_call_argument_values(code,base,start,address);
                let known_float=super::semantic::known_call_float_argument_values(code,base,start,address);
                let known_stack=super::semantic::known_call_stack_argument_values(code,base,start,address);
                if let Some(selector)=objc_selectors.get(&address){
                    let parts=selector.split(':').collect::<Vec<_>>();
                    let argument_count=parts.len().saturating_sub(1);
                    if parts.iter().all(|part|part.bytes().all(|byte|byte.is_ascii_alphanumeric()||byte==b'_'))
                        &&(argument_count==0||selector.ends_with(':'))
                        &&parts.iter().take(argument_count).all(|part|!part.is_empty())&&argument_count<=6{
                        let values=known.iter().map(|(register,value,_)|(*register,*value)).collect::<BTreeMap<_,_>>();
                        let receiver=values.get(&0).map(|value|format!("{value:#x}")).unwrap_or_else(||"x0".into());
                        let mut message=format!("[{receiver}");
                        if argument_count==0{message.push(' ');message.push_str(selector);}
                        else{for (index,part) in parts.iter().take(argument_count).enumerate(){let argument_register=index as u8+2;let argument=values.get(&argument_register).map(|value|format!("{value:#x}")).unwrap_or_else(||format!("x{argument_register}"));message.push(' ');message.push_str(part);message.push(':');message.push_str(&argument);}}
                        message.push(']');
                        return format!("{message}; /* objc_msgSend selector validated through __objc_selrefs at {address:#x}; receiver and argument types unresolved */");
                    }
                }
                let known_values=known.iter().map(|(register,value,provenance)|format!("x{register}={value:#x} {provenance}")).collect::<Vec<_>>();
                return if let Some(name)=target_name{
                    let target_signature=signatures.get(&target).map(|(_,signature)|signature);
                    let values=known.iter().map(|(register,value,provenance)|(*register,(*value,provenance.as_str()))).collect::<BTreeMap<_,_>>();
                    let float_values=known_float.iter().map(|(register,value,provenance)|(*register,(*value,provenance.as_str()))).collect::<BTreeMap<_,_>>();
                    let stack_values=known_stack.iter().map(|(offset,bytes,value,provenance)|(*offset,(*bytes,*value,provenance.as_str()))).collect::<BTreeMap<_,_>>();
                    let args=target_signature.map(|signature|abi_parameters(Some(signature))).unwrap_or_else(||candidate_abi_parameters(parameter_evidence,target));
                    let bindings=args.iter().map(|argument|format!("{}={}",argument.name,argument.register)).collect::<Vec<_>>().join(", ");
                    let mut evidence=Vec::new();
                    let mut rendered=args.iter().map(|argument|{
                        if let Some(register)=argument.general_register {
                            if let Some((value,provenance))=values.get(&register){
                                let (rendered, cast) = typed_gp_argument(*value, argument.source_type.as_deref());
                                evidence.push(format!("{}({})={value:#x} {provenance}{}",argument.name,argument.register,
                                    cast.map(|cast| format!(" [{cast}]")).unwrap_or_default()));
                                return rendered;
                            }
                        }
                        if let (Some((offset, width)), Some(ty))=(argument.stack_scalar,argument.source_type.as_deref()) {
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
                        argument.register.clone()
                    }).collect::<Vec<_>>();
                    if target_signature.is_some_and(|sig| sig.declaration.contains("...")) {
                        rendered.push("/* variadic arguments unresolved; Apple arm64 stack ABI */".into());
                    }
                    let arguments=if args.is_empty()&&target_signature.is_none(){if known_values.is_empty(){"/* prototype and ABI arguments unresolved */".into()}else{format!("/* prototype unresolved; known ABI register values: {} */",known_values.join(", "))}}else{rendered.join(", ")};
                    let resolved=evidence.join(", ");
                    let binding_note=if bindings.is_empty(){"argument register bindings unresolved".into()}else if target_signature.is_some(){format!("argument bindings: {bindings}")}else{format!("candidate ABI argument bindings: {bindings}; callee types unresolved")};
                    let evidence_note=if resolved.is_empty(){"no argument constants proven in the straight-line call block".to_string()}else{format!("constant argument evidence: {resolved}")};
                    format!("{}({}); /* {} {target:#x}; {}; {binding_note}; {evidence_note} */",callable_name(name,target_signature),arguments,if indirect{"indirect target resolved through a register"}else{"direct call target"},if target_signature.is_some(){"arguments follow recovered prototype arity"}else if args.is_empty(){"callee prototype and entry-use arity unavailable"}else{"arity candidate from callee entry-register uses"})
                }else{format!("call(/* unresolved target {target:#x}; {}; {} */);",if indirect{"indirect target recovered from a register"}else{"direct BL"},if known_values.is_empty(){"arguments unresolved".to_string()}else{format!("known ABI register values: {}",known_values.join(", "))})};
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
    tail_calls: &[(u64,u64,u64)], jump_tables: &str, objc_selectors: &BTreeMap<u64,String>,
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
    let parameter_aliases = parameter_aliases_by_instruction(&instructions, signature, parameter_evidence, start);
    let switch_evidence=parse_switch_evidence(jump_tables);
    let mut labels = instructions.iter().filter_map(|(_,statement)| {
        let target = statement.strip_prefix("goto 0x")
            .or_else(|| statement.strip_prefix("if (").and_then(|rest| rest.split_once(") goto 0x").map(|(_,target)|target)))?;
        let digits = target.split(|c: char| !c.is_ascii_hexdigit()).next()?;
        u64::from_str_radix(digits,16).ok().filter(|target| instruction_addresses.contains(target))
    }).collect::<std::collections::BTreeSet<_>>();
    for evidence in switch_evidence.values(){for &(_,target) in &evidence.cases{if instruction_addresses.contains(&target){labels.insert(target);}}}
    let incoming_branches=instructions.iter().filter_map(|(source,statement)|branch_target_statement(statement).map(|target|(target,*source))).collect::<Vec<_>>();

    let mut out = String::new();
    out.push_str(&format!("// {start:#018x}..{end:#018x}; decoded machine operations translated to C-like statements\n"));
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
            out.push_str(&format!("    {address:#018x}: switch (x{index_register}) {{ /* bounded jump-table candidate; index interpretation and case range unresolved */\n"));
            for &(case,target) in &evidence.cases {
                let destination=if instruction_addresses.contains(&target){format!("goto loc_{target:x};")}else{format!("goto {target:#x}; /* target outside this recovered function */")};
                out.push_str(&format!("        case {case}: {destination} /* table target {target:#x} */\n"));
            }
            out.push_str(&format!("        default: goto x{}; /* unresolved indirect target */\n    }}\n",evidence.branch_register));
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
                            out.push_str(&format!("    {header:#018x}: while (!({condition})) {{ /* natural loop */\n"));
                            for (body_address,body_statement) in &instructions[instruction_index+1..backedge]{
                                let rendered=render_linear_statement(body_statement,*body_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
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
                        out.push_str(&format!("        {body_address:#018x}: {rendered}\n"));
                    }
                    out.push_str(&format!("    }} while ({condition}); /* backedge at {:#x} */\n",instructions[loop_tail].0));
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
                                        out.push_str(&format!("    {address:#018x}: if (!({condition})) {{ /* structured diamond; else={else_start:#x}, join={join:#x} */\n"));
                                        for (arm_address,arm_statement) in &instructions[instruction_index+1..then_branch]{
                                            let rendered=render_linear_statement(arm_statement,*arm_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
                                            out.push_str(&format!("        {arm_address:#018x}: {rendered}\n"));
                                        }
                                        out.push_str("    } else {\n");
                                        for (arm_address,arm_statement) in &instructions[else_index..join_index]{
                                            let rendered=render_linear_statement(arm_statement,*arm_address,start,symbols,signatures,signature,parameter_evidence,code,base,stable_stack,objc_selectors);
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
                        out.push_str(&format!("    if (!({condition})) {return_statement} /* structured from {address:#x} and {return_address:#x} */\n"));
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
                        let arguments = if args.is_empty() && target_signature.is_none() { "/* AAPCS64 argument registers unresolved */".into() } else { args.iter().map(|argument|argument.register.as_str()).collect::<Vec<_>>().join(", ") };
                        let callee = callable_name(name,target_signature);
                        if signature_returns_void(target_signature) {
                            format!("{callee}({arguments}); return; /* cross-function B tail-call candidate */")
                        } else {
                            format!("return {callee}({arguments}); /* cross-function B tail-call candidate; return compatibility unresolved */")
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
        let statement = substitute_parameter_names(&statement, parameter_aliases.get(&address).unwrap_or(&BTreeMap::new()));
        let statement=if stable_stack{localize_stack_slot_accesses(&statement)}else{statement};
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
    for line in objc_records.lines().filter(|line| line.contains(" method ") && line.contains("confidence=validated")) {
        let Some(imp) = line.split(" imp=").nth(1).and_then(|s| s.split_whitespace().next()).and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"),16).ok()) else { continue };
        if imp == 0 { continue }
        let Some(name) = line.split(" resolved=").nth(1).and_then(|s| s.split(" confidence=").next()) else { continue };
        let Some(encoding) = line.split(" types=").nth(1).and_then(|s| s.split(" imp=").next()) else { continue };
        if let Some(sig) = objc_method(name.trim(),encoding.trim()) { by_address.insert(imp,(name.trim().into(),sig)); }
    }
    let mut signatures = String::from("Signature evidence: exact SDK ABI declarations, validated Objective-C type encodings, hash-bound analyst C prototypes, and supported Itanium mangled parameter types. Unknown source types remain unresolved.\n");
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
    for (i,&start) in starts.iter().take(200).enumerate() {
        let end = starts.get(i+1).copied().unwrap_or(scan_end);
        if pseudo.len() > 55_000 { break }
        let named = by_address.get(&start);
        let name = named.map(|(name,_)| name.as_str()).or_else(||symbols.iter().find(|(address,_)| *address == start).map(|(_,name)|name.as_str())).unwrap_or("<unnamed>");
        let signature = named.map(|(_,signature)| signature);
        if let Some(context) = &arm64 {
            let end = starts.get(i+1).copied().unwrap_or(scan_end);
            if let Some(body) = render_arm64_body(start, end, name, signature, symbols, &by_address, context.code, context.base, context.file_offset, context.data_ranges, context.parameter_evidence, context.tail_calls, context.jump_tables, context.objc_selectors) {
                if pseudo.len() + body.len() > 58_000 { break; }
                pseudo.push_str(&body);
                pseudo.push('\n');
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
    }
    (signatures,pseudo)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn terminal_statements_cannot_be_folded_into_structured_regions() {
        assert!(is_linear_statement("x0 = x0 + 1;"));
        assert!(!is_linear_statement("return x0;"));
        assert!(!is_linear_statement("goto 0x1000;"));
        assert!(!is_linear_statement("if (x0 == 0) goto 0x1000;"));
    }
    #[test] fn objc_encoding() {
        let s=objc_method("-[NSString substringFromIndex:]", "@\"NSString\"24@0:8Q16").unwrap();
        assert_eq!(s.declaration,"- (NSString *)substringFromIndex:(NSUInteger)from;");
        assert!(objc_method("-[A x:]", "v24@0:8").is_none());
        assert_eq!(objc_method("-[A rect:]", "v48@0:8{CGRect={CGPoint=dd}{CGSize=dd}}16").unwrap().declaration, "- (void)rect:(CGRect)arg0;");
        assert!(objc_method("-[A rect:]", "v48@0:8{CGRect=qqqq}16").is_none());
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
    #[test] fn exact_catalog_and_cpp() {
        assert_eq!(symbol("_CFRelease").unwrap().declaration,"void CFRelease(CFTypeRef cf);");
        assert!(symbol("_CFReleaseExtra").is_none());
        assert_eq!(symbol("__ZN3Foo3barEi").unwrap().declaration,"/* return type unresolved */ Foo::bar(int arg0);");
        assert!(symbol("__ZN3Foo3barESt6vectorIiE").is_none());
    }
    #[test] fn hash_bound_external_c_prototypes_supply_types_and_reject_unsafe_records() {
        let digest="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let records=format!("# source_sha256={digest}\n_custom\tint custom(const char *name, size_t count);\n_empty\tvoid empty(void);\n");
        let parsed=parse_prototype_database(&records,digest).unwrap();
        let signature=parsed.get("_custom").unwrap();
        assert_eq!(signature.source,"hash-bound analyst C prototype catalog");
        assert_eq!(signature.declaration,"int custom(const char *name, size_t count);");
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
        ] {assert!(parse_prototype_database(&invalid,digest).is_err(),"{invalid}")}
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
            assert!(call.contains("stack argument (slot 0, sp+0x0"),"{description}: {call}");
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
        let records="reconstructed class 0x0000000000001000  name=Worker  super=0x0  ro=0x2000  conforms=Primary,Tracing\nreconstructed class 0x0000000000003000  name=Plain  super=0x0  ro=0x4000\n";
        let output=objc_conformance_declarations(&(records.to_string()+records));
        assert_eq!(output.matches("class=Worker conforms=Primary,Tracing").count(),1,"{output}");
        assert!(output.contains("@interface Worker <Primary, Tracing>"),"{output}");
        assert!(output.contains("implementation-coverage=unproven dispatch-targets=unresolved"),"{output}");
        assert!(!output.contains("Plain"));
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
        let body = render_arm64_body(0x1000, 0x1004, "_pointer", Some(&pointer), &[], &BTreeMap::new(), &code, 0x1000, 0, &[], "", &[], "", &BTreeMap::new()).unwrap();
        assert!(body.contains("return x0;"), "{body}");
        assert!(!body.contains("void return supported"), "{body}");
    }
    #[test] fn variadic_calls_show_fixed_prototype_and_unknown_anonymous_arguments() {
        let signature = symbol("_printf").unwrap();
        let signatures = BTreeMap::from([(0x2000, ("_printf".into(), signature))]);
        let call = render_linear_statement("call(0x2000);", 0x1000, 0x1000, &[], &signatures, None, "", &[], 0x1000, false, &BTreeMap::new());
        assert!(call.starts_with("printf(x0, /* variadic arguments unresolved; Apple arm64 stack ABI */);"), "{call}");
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
    #[test] fn parameter_names_flow_until_the_register_is_redefined() {
        assert_eq!(copied_register("x6 = x4;"), Some("x4".into()));
        assert_eq!(copied_register("x6 = x4 + 1;"), None);
        let signature = objc_method("-[Widget setObject:]", "v24@0:8@16").unwrap();
        let instructions = vec![
            (0x0ffc, "x4 = x2;".into()),
            (0x1000, "x2 = *(x0 + 0x10);".into()),
            (0x1004, "x5 = x4;".into()),
            (0x1008, "x3 = x2;".into()),
            (0x100c, "return x5;".into()),
        ];
        let aliases = parameter_aliases_by_instruction(&instructions, Some(&signature), "", 0x1000);
        assert_eq!(substitute_parameter_names(&instructions[0].1, &aliases[&0x0ffc]), "arg0_copy_x4 = arg0;");
        assert_eq!(substitute_parameter_names(&instructions[1].1, &aliases[&0x1000]), "arg0 = *(self + 0x10);");
        assert_eq!(substitute_parameter_names(&instructions[2].1, &aliases[&0x1004]), "arg0_copy_x4_copy_x5 = arg0_copy_x4;");
        assert_eq!(substitute_parameter_names(&instructions[3].1, &aliases[&0x1008]), "x3 = x2;");
        assert_eq!(substitute_parameter_names(&instructions[4].1, &aliases[&0x100c]), "return arg0_copy_x4_copy_x5;");
        assert_eq!(substitute_parameter_names("w2 = w2;", &parameter_names(Some(&signature), "", 0)), "arg0 = arg0;");
    }
    #[test] fn parameter_aliases_stop_at_control_flow_and_calls() {
        let signature = symbol("_CFRelease").unwrap();
        let branched = vec![(0x1000, "if (x0 == 0) goto 0x1008;".into()), (0x1004, "return x0;".into())];
        assert!(parameter_aliases_by_instruction(&branched, Some(&signature), "", 0x1000).is_empty());
        let called = vec![(0x1000, "call(0x2000);".into()), (0x1004, "return x0;".into())];
        let aliases = parameter_aliases_by_instruction(&called, Some(&signature), "", 0x1000);
        assert_eq!(substitute_parameter_names("return x0;", &aliases[&0x1000]), "return cf;");
        assert_eq!(substitute_parameter_names("return x0;", &aliases[&0x1004]), "return x0;");
    }
    #[test] fn scalar_fp_parameter_names_cover_single_and_double_register_views() {
        let signature = symbol("_CFNumberGetValue").unwrap();
        let names = parameter_names(Some(&signature), "", 0);
        assert_eq!(names.get("x1").map(String::as_str), Some("theType"));
        let float_signature = Signature { language: "C", declaration: "double scale(float factor, double value);".into(), source: "test prototype" };
        let names = parameter_names(Some(&float_signature), "", 0);
        assert_eq!(names.get("s0").map(String::as_str), Some("factor"));
        assert_eq!(names.get("d0").map(String::as_str), Some("factor"));
        assert_eq!(names.get("d1").map(String::as_str), Some("value"));
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
        assert!(data_only.contains("unresolved target 0x3000"), "{data_only}");
    }
    #[test] fn direct_calls_use_candidate_arity_without_prototype() {
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
