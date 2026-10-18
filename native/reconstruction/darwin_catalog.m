// Isolated catalog of this helper's Darwin runtime, never an arbitrary target loader.
#import <Foundation/Foundation.h>
#import <CoreFoundation/CoreFoundation.h>
#import <CoreGraphics/CoreGraphics.h>
#include <objc/runtime.h>
#include <mach-o/dyld.h>
#include <mach-o/loader.h>
#include <mach/mach.h>
#include <dlfcn.h>
#include <uuid/uuid.h>
#include <sys/sysctl.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

static NSString *text(const char *value) {
    if (!value) return @"";
    return [[NSString alloc] initWithBytes:value length:strnlen(value, 16384) encoding:NSUTF8StringEncoding] ?: @"";
}
static NSString *hex(uint64_t value) { return [NSString stringWithFormat:@"0x%llx", (unsigned long long)value]; }

static NSDictionary *image_for_address(const void *address) {
    Dl_info info = {0};
    if (!address || !dladdr(address, &info) || !info.dli_fbase) return @{};
    NSMutableDictionary *result = [@{@"path": text(info.dli_fname), @"runtime_base": hex((uintptr_t)info.dli_fbase)} mutableCopy];
    const struct mach_header_64 *header = info.dli_fbase;
    if (header->magic != MH_MAGIC_64 || header->ncmds > 8192 || header->sizeofcmds > 16 * 1024 * 1024) return result;
    result[@"cpu_type"] = @(header->cputype);
    result[@"cpu_subtype"] = @(header->cpusubtype);
    const uint8_t *commands = (const uint8_t *)(header + 1);
    size_t at = 0;
    uint64_t text_vmaddr = 0;
    BOOL has_text = NO;
    for (uint32_t index = 0; index < header->ncmds; index++) {
        if (at > header->sizeofcmds || header->sizeofcmds - at < sizeof(struct load_command)) break;
        const struct load_command *command = (const void *)(commands + at);
        if (command->cmdsize < sizeof(*command) || command->cmdsize > header->sizeofcmds - at) break;
        if (command->cmd == LC_UUID && command->cmdsize >= sizeof(struct uuid_command)) {
            char uuid[37]; uuid_unparse_lower(((const struct uuid_command *)command)->uuid, uuid);
            result[@"uuid"] = text(uuid);
        } else if (command->cmd == LC_SEGMENT_64 && command->cmdsize >= sizeof(struct segment_command_64)) {
            const struct segment_command_64 *segment = (const void *)command;
            if (!strncmp(segment->segname, "__TEXT", sizeof(segment->segname))) {
                text_vmaddr = segment->vmaddr; has_text = YES;
            }
        } else if (command->cmd == LC_BUILD_VERSION && command->cmdsize >= sizeof(struct build_version_command)) {
            const struct build_version_command *build = (const void *)command;
            result[@"platform"] = @(build->platform);
            result[@"min_os"] = @(build->minos);
            result[@"sdk"] = @(build->sdk);
        } else if (command->cmd == LC_ID_DYLIB && command->cmdsize >= sizeof(struct dylib_command)) {
            const struct dylib_command *dylib = (const void *)command;
            uint32_t offset = dylib->dylib.name.offset;
            if (offset >= sizeof(*dylib) && offset < command->cmdsize) {
                const char *name = (const char *)command + offset;
                size_t remaining = command->cmdsize - offset;
                if (memchr(name, 0, remaining)) result[@"install_name"] = text(name);
            }
            result[@"current_version"] = @(dylib->dylib.current_version);
            result[@"compatibility_version"] = @(dylib->dylib.compatibility_version);
        }
        at += command->cmdsize;
    }
    if (has_text && (uintptr_t)address >= (uintptr_t)header) {
        uint64_t offset = (uintptr_t)address - (uintptr_t)header;
        if (offset <= UINT64_MAX - text_vmaddr) {
            result[@"preferred_address"] = hex(text_vmaddr + offset);
            result[@"image_offset"] = hex(offset);
        }
    }
    if (info.dli_sname) {
        result[@"nearest_symbol"] = text(info.dli_sname);
        result[@"symbol_exact"] = @((uintptr_t)info.dli_saddr == (uintptr_t)address);
    }
    return result;
}

static NSDictionary *type_layout(const char *encoding) {
    if (!encoding) return @{};
    @try {
        NSUInteger size = 0, alignment = 0;
        const char *end = NSGetSizeAndAlignment(encoding, &size, &alignment);
        if (end && !*end) return @{@"encoding": text(encoding), @"size": @(size), @"alignment": @(alignment),
                                   @"source": @"Foundation NSGetSizeAndAlignment"};
    } @catch (NSException *exception) { (void)exception; }
    return @{@"encoding": text(encoding), @"layout": @"unresolved"};
}

static NSArray *method_records(Class cls, BOOL class_methods, BOOL *truncated) {
    unsigned count = 0;
    Method *methods = class_copyMethodList(class_methods ? object_getClass(cls) : cls, &count);
    NSMutableArray *records = [NSMutableArray array];
    for (unsigned index = 0; index < count && index < 512; index++) {
        Method method = methods[index];
        SEL selector = method_getName(method);
        const char *name = sel_getName(selector);
        NSMutableArray *arguments = [NSMutableArray array];
        unsigned argument_count = method_getNumberOfArguments(method);
        for (unsigned argument = 0; argument < argument_count && argument < 64; argument++) {
            char *type = method_copyArgumentType(method, argument);
            [arguments addObject:type_layout(type)]; free(type);
        }
        if (argument_count > 64) *truncated = YES;
        char *return_type = method_copyReturnType(method);
        IMP implementation = method_getImplementation(method);
        [records addObject:@{@"selector": text(name), @"class_method": @(class_methods),
            @"name": [NSString stringWithFormat:@"%c[%@ %@]", class_methods ? '+' : '-', text(class_getName(cls)), text(name)],
            @"encoding": text(method_getTypeEncoding(method)), @"argument_count": @(argument_count),
            @"arguments": arguments, @"return_type": type_layout(return_type),
            @"runtime_imp": hex((uintptr_t)implementation), @"implementation_image": image_for_address((const void *)implementation)}];
        free(return_type);
    }
    free(methods);
    if (count > 512) *truncated = YES;
    return [records sortedArrayUsingDescriptors:@[[NSSortDescriptor sortDescriptorWithKey:@"selector" ascending:YES]]];
}

static NSArray *property_records(Class cls, BOOL class_properties, BOOL *truncated) {
    unsigned count = 0;
    objc_property_t *properties = class_copyPropertyList(class_properties ? object_getClass(cls) : cls, &count);
    NSMutableArray *records = [NSMutableArray array];
    for (unsigned index = 0; index < count && index < 256; index++) {
        objc_property_t property = properties[index];
        unsigned attribute_count = 0;
        objc_property_attribute_t *attributes = property_copyAttributeList(property, &attribute_count);
        NSMutableDictionary *fields = [NSMutableDictionary dictionary];
        for (unsigned attribute = 0; attribute < attribute_count && attribute < 64; attribute++) {
            fields[text(attributes[attribute].name)] = text(attributes[attribute].value);
        }
        if (attribute_count > 64) *truncated = YES;
        free(attributes);
        [records addObject:@{@"name": text(property_getName(property)), @"class_property": @(class_properties),
            @"attributes": text(property_getAttributes(property)), @"fields": fields}];
    }
    free(properties);
    if (count > 256) *truncated = YES;
    return [records sortedArrayUsingDescriptors:@[[NSSortDescriptor sortDescriptorWithKey:@"name" ascending:YES]]];
}

static NSDictionary *class_record(Class cls, BOOL *truncated) {
    NSMutableArray *ivars = [NSMutableArray array];
    unsigned count = 0;
    Ivar *members = class_copyIvarList(cls, &count);
    for (unsigned index = 0; index < count && index < 256; index++) {
        Ivar ivar = members[index];
        [ivars addObject:@{@"name": text(ivar_getName(ivar)), @"offset": @(ivar_getOffset(ivar)),
                          @"type": type_layout(ivar_getTypeEncoding(ivar))}];
    }
    if (count > 256) *truncated = YES;
    free(members);
    return @{@"name": text(class_getName(cls)), @"superclass": text(class_getName(class_getSuperclass(cls))),
        @"metaclass_name": text(class_getName(object_getClass(cls))), @"instance_size": @(class_getInstanceSize(cls)),
        @"declaring_image_path": text(class_getImageName(cls)), @"image": image_for_address((__bridge const void *)cls),
        @"instance_methods": method_records(cls, NO, truncated), @"class_methods": method_records(cls, YES, truncated),
        @"properties": property_records(cls, NO, truncated), @"class_properties": property_records(cls, YES, truncated),
        @"ivars": [ivars sortedArrayUsingDescriptors:@[[NSSortDescriptor sortDescriptorWithKey:@"name" ascending:YES]]],
        @"membership": @"declared-only; superclass records describe inherited members"};
}

#define FIELD(T, F) @{@"name": @#F, @"offset": @(offsetof(T, F)), @"size": @(sizeof(((T *)0)->F))}
#define RECORD(T, ...) @{@"name": @#T, @"encoding": text(@encode(T)), @"size": @(sizeof(T)), @"alignment": @(_Alignof(T)), @"fields": @[__VA_ARGS__], @"source": @"Apple SDK compiler sizeof/alignof/offsetof"}

int main(int argc, const char **argv) {
    @autoreleasepool {
        NSMutableArray *classes = [NSMutableArray array], *symbols = [NSMutableArray array];
        NSMutableArray *missing_classes = [NSMutableArray array], *missing_symbols = [NSMutableArray array];
        BOOL truncated = NO;
        for (int index = 1; index < argc; index += 2) {
            if (index + 1 >= argc || strlen(argv[index + 1]) > 1024) return 2;
            if (!strcmp(argv[index], "--class") && classes.count + missing_classes.count < 32) {
                Class cls = objc_lookUpClass(argv[index + 1]);
                if (cls) [classes addObject:class_record(cls, &truncated)];
                else [missing_classes addObject:text(argv[index + 1])];
            } else if (!strcmp(argv[index], "--symbol") && symbols.count + missing_symbols.count < 128) {
                void *address = dlsym(RTLD_DEFAULT, argv[index + 1]);
                if (address) [symbols addObject:@{@"lookup_name": text(argv[index + 1]), @"runtime_address": hex((uintptr_t)address),
                                                  @"image": image_for_address(address), @"source": @"Darwin dlsym(RTLD_DEFAULT)"}];
                else [missing_symbols addObject:text(argv[index + 1])];
            } else return 2;
        }
        char build[256] = {0}; size_t build_size = sizeof(build);
        if (sysctlbyname("kern.osversion", build, &build_size, NULL, 0)) build[0] = 0;
        NSArray *layouts = @[
            RECORD(NSRange, FIELD(NSRange, location), FIELD(NSRange, length)),
            RECORD(CFRange, FIELD(CFRange, location), FIELD(CFRange, length)),
            RECORD(CGPoint, FIELD(CGPoint, x), FIELD(CGPoint, y)),
            RECORD(CGSize, FIELD(CGSize, width), FIELD(CGSize, height)),
            RECORD(CGRect, FIELD(CGRect, origin), FIELD(CGRect, size)),
            RECORD(mach_msg_header_t, FIELD(mach_msg_header_t, msgh_bits), FIELD(mach_msg_header_t, msgh_size), FIELD(mach_msg_header_t, msgh_id))
        ];
        NSDictionary *document = @{@"schema": @1, @"producer": @"hindsight-darwin-native-v1", @"scope": @"helper-process loaded runtime; no target loaded",
            @"host_os": NSProcessInfo.processInfo.operatingSystemVersionString, @"os_build": text(build),
#if defined(__arm64e__)
            @"architecture": @"arm64e",
#elif defined(__arm64__)
            @"architecture": @"arm64",
#elif defined(__x86_64__)
            @"architecture": @"x86_64",
#else
            @"architecture": @"unknown",
#endif
            @"pointer_size": @(sizeof(void *)), @"classes": classes, @"symbols": symbols, @"types": layouts,
            @"missing_classes": missing_classes, @"missing_symbols": missing_symbols, @"truncated": @(truncated)};
        NSError *error = nil;
        NSData *encoded = [NSJSONSerialization dataWithJSONObject:document options:NSJSONWritingSortedKeys error:&error];
        if (!encoded || encoded.length > 8 * 1024 * 1024) return 3;
        if (fwrite(encoded.bytes, 1, encoded.length, stdout) != encoded.length) return 4;
        return 0;
    }
}
