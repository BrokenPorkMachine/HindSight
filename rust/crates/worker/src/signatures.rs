//! Source signature evidence recovered from names and Objective-C runtime type encodings.
//! A declaration is emitted only when metadata or an exact API catalog entry supplies it.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub language: &'static str,
    pub declaration: String,
    pub source: &'static str,
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
        // Aggregates and unknown encodings cannot safely be rendered as a
        // pointer: their ABI passing convention may be different.
        b'?' | b'{' | b'(' | b'[' => return None,
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
    if let Some(declaration) = foundation_method(name) {
        return Some(Signature { language: "Objective-C / Objective-C++", declaration: declaration.into(), source: "exact Foundation SDK method declaration; encoding arity validated" });
    }
    Some(Signature { language: "Objective-C / Objective-C++", declaration, source: "validated Objective-C method type encoding" })
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
    recover_internal(starts, symbols, objc_records, calls, scan_end, None)
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
    tail_calls: &[(u64,u64,u64)], jump_tables: &str, objc_selectors: &BTreeMap<u64,String>,
) -> (String, String) {
    recover_internal(starts, symbols, objc_records, calls, scan_end,
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
struct AbiParameter { name: String, register: String, general_register: Option<u8> }

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

fn abi_parameters(signature: Option<&Signature>) -> Vec<AbiParameter> {
    let Some(signature) = signature else { return Vec::new() };
    let is_objc = signature.language == "Objective-C / Objective-C++";
    let parameters = if is_objc { objc_parameters(signature) } else { c_parameters(signature) };
    let mut gp = if is_objc { 2u8 } else { 0u8 };
    let mut fp = 0u8;
    let mut stack_slot = 0u8;
    let mut result = Vec::new();
    if is_objc {
        result.push(AbiParameter { name: "self".into(), register: "x0".into(), general_register: Some(0) });
        result.push(AbiParameter { name: "_cmd".into(), register: "x1".into(), general_register: Some(1) });
    }
    for (ty, name) in parameters {
        if (ty.contains("float") || ty.contains("double")) && fp < 8 {
            let register = format!("{}{fp}", if ty.contains("float") && !ty.contains("double") { "s" } else { "d" });
            result.push(AbiParameter { name, register, general_register: None });
            fp += 1;
        } else if gp < 8 {
            result.push(AbiParameter { name, register: format!("x{gp}"), general_register: Some(gp) });
            gp += 1;
        } else {
            let offset = stack_slot as usize * 8;
            result.push(AbiParameter { name, register: format!("stack argument (slot {stack_slot}, sp+0x{offset:x})"), general_register: None });
            stack_slot += 1;
        }
    }
    result
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
        name: format!("arg{index}"), register: format!("x{index}"), general_register: Some(index),
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
            (index < 8).then(|| AbiParameter { name: format!("arg{index}"), register: format!("x{index}"), general_register: Some(index) })
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
                    let args=target_signature.map(|signature|abi_parameters(Some(signature))).unwrap_or_else(||candidate_abi_parameters(parameter_evidence,target));
                    let bindings=args.iter().map(|argument|format!("{}={}",argument.name,argument.register)).collect::<Vec<_>>().join(", ");
                    let mut evidence=Vec::new();
                    let rendered=args.iter().map(|argument|{
                        if let Some(register)=argument.general_register {
                            if let Some((value,provenance))=values.get(&register){
                                evidence.push(format!("{}({})={value:#x} {provenance}",argument.name,argument.register));return format!("{value:#x}")
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
        if signature.is_some_and(|sig|sig.declaration.starts_with("- (void)")||sig.declaration.starts_with("+ (void)")||sig.declaration.starts_with("void ")){
            return "return; /* void return supported by recovered signature */".into();
        }
        return "return x0; /* inferred ABI result; source return type unresolved */".into();
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
                        let return_statement = if return_statement.starts_with("return /* ABI result register x0")
                            && signature.is_some_and(|sig| sig.declaration.starts_with("- (void)") || sig.declaration.starts_with("+ (void)") || sig.declaration.starts_with("void "))
                        { "return;" }
                        else { return_statement.trim_end_matches(';') };
                        out.push_str(&format!("    if (!({condition})) {return_statement}; /* structured from {address:#x} and {return_address:#x} */\n"));
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
                        if target_signature.is_some_and(|sig| sig.declaration.starts_with("void ") || sig.declaration.starts_with("- (void)") || sig.declaration.starts_with("+ (void)")) {
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
            if signature.is_some_and(|sig| sig.declaration.starts_with("- (void)") || sig.declaration.starts_with("+ (void)") || sig.declaration.starts_with("void ")) {
                "return; /* void return supported by recovered signature */".into()
            } else {
                "return x0; /* inferred ABI result; source return type unresolved */".into()
            }
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
    arm64: Option<Arm64RenderContext<'_>>,
) -> (String, String) {
    let mut by_address = BTreeMap::<u64,(String,Signature)>::new();
    let mut name_only = BTreeMap::<String,Signature>::new();
    for (address,name) in symbols { if let Some(sig) = symbol(name) {
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
    let mut signatures = String::from("Signature evidence: exact SDK ABI declarations, validated Objective-C type encodings, and supported Itanium mangled parameter types. Unknown source types remain unresolved.\n");
    for (address,(name,sig)) in by_address.iter().take(700) {
        let row = format!("{address:#018x} {name} language={} source={} declaration={}\n",sig.language,sig.source,sig.declaration);
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
        assert!(objc_method("-[A rect:]", "v48@0:8{CGRect={CGPoint=dd}{CGSize=dd}}16").is_none());
    }
    #[test] fn exact_catalog_and_cpp() {
        assert_eq!(symbol("_CFRelease").unwrap().declaration,"void CFRelease(CFTypeRef cf);");
        assert!(symbol("_CFReleaseExtra").is_none());
        assert_eq!(symbol("__ZN3Foo3barEi").unwrap().declaration,"/* return type unresolved */ Foo::bar(int arg0);");
        assert!(symbol("__ZN3Foo3barESt6vectorIiE").is_none());
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
