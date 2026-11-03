// SPDX-License-Identifier: CC0-1.0
#import "darwin_catalog_sample.h"

__attribute__((visibility("default"), noinline, used))
unsigned long HSNativeCatalogCount(unsigned long value) { return value + 3; }

__attribute__((visibility("default"), noinline, used))
double HSNativeCatalogRatio(unsigned long value) { return (double)value / 2.0; }
__attribute__((visibility("default"), noinline, used))
NSRange HSNativeCatalogRange(unsigned long value) { return NSMakeRange(value, value + 1); }
__attribute__((visibility("default"), noinline, used))
CGRect HSNativeCatalogRect(unsigned long value) { return CGRectMake((double)value, 2, 3, 4); }
__attribute__((visibility("default"), noinline, used))
CGAffineTransform HSNativeCatalogTransform(unsigned long value) { return (CGAffineTransform){1, 0, 0, 1, (double)value, 0}; }

void HSNativeCatalogCallback(void (*callback)(int)) { callback(7); }



@implementation HSNativeCatalogFixture
- (double)score:(double)factor count:(NSUInteger)count { return self.weight * factor + (double)count; }
- (HSNativeCatalogPair)pair { return (HSNativeCatalogPair){(uint32_t)self.count, self.weight}; }
- (unsigned long)scalarCount:(unsigned long)value { return value + 3; }
+ (NSRange)rangeForCount:(NSUInteger)count { return NSMakeRange(0, count); }
@end

@implementation HSNativeCatalogDerived
- (long)derivedValue { return 7; }
- (id)copyWithZone:(NSZone *)zone { (void)zone; return self; }
@end


#ifdef HS_CATALOG_STANDALONE
__attribute__((noinline, used))
double HSNativeCatalogTail(void) { return HSNativeCatalogRatio(6); }

__attribute__((noinline, used))
double HSNativeCatalogStack(double a, double b, double c, double d, double e, double f, double g, double h, double ninth, double tenth) {
    return a + b + c + d + e + f + g + h + ninth + tenth;
}

int main(int argc, char **argv) {
    (void)argv;
    NSRange range = HSNativeCatalogRange((unsigned long)argc);
    CGRect rect = HSNativeCatalogRect((unsigned long)argc);
    double ratio = HSNativeCatalogRatio((unsigned long)argc);
    CGAffineTransform transform = HSNativeCatalogTransform((unsigned long)argc);
    double stacked = HSNativeCatalogStack(argc, argc, argc, argc, argc, argc, argc, argc, 1.0f, -1.0);
    return (int)(HSNativeCatalogCount(2) + range.length + rect.size.width + ratio + transform.a + stacked);
}
#endif
