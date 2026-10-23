// SPDX-License-Identifier: CC0-1.0
#import "darwin_catalog_sample.h"

__attribute__((visibility("default"), noinline, used))
unsigned long HSNativeCatalogCount(unsigned long value) { return value + 3; }

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
int main(void) { return HSNativeCatalogCount(2) == 5 ? 0 : 1; }
#endif
