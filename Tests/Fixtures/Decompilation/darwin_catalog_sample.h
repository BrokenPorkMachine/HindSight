// SPDX-License-Identifier: CC0-1.0
#import <Foundation/Foundation.h>
#include <stdint.h>

typedef struct HSNativeCatalogPair {
    uint32_t tag;
    double weight;
} HSNativeCatalogPair;

unsigned long HSNativeCatalogCount(unsigned long value);
void HSNativeCatalogCallback(void (*callback)(int));

@interface HSNativeCatalogFixture : NSObject
@property(nonatomic) long count;
@property(nonatomic) double weight;
@property(nonatomic, copy, getter=displayLabel, setter=setDisplayLabel:) NSString *label;
- (double)score:(double)factor count:(NSUInteger)count;
- (HSNativeCatalogPair)pair;
+ (NSRange)rangeForCount:(NSUInteger)count;
@end
