// SPDX-License-Identifier: CC0-1.0
#import <Foundation/Foundation.h>
#include <stdint.h>

typedef struct HSNativeCatalogPair {
    uint32_t tag;
    double weight;
} HSNativeCatalogPair;

unsigned long HSNativeCatalogCount(unsigned long value);
double HSNativeCatalogRatio(unsigned long value);
double HSNativeCatalogStack(double a, double b, double c, double d, double e, double f, double g, double h, double ninth, double tenth);
NSRange HSNativeCatalogRange(unsigned long value);
CGRect HSNativeCatalogRect(unsigned long value);
CGAffineTransform HSNativeCatalogTransform(unsigned long value);
unsigned long _HSNativeUnderscored(unsigned long value);
void HSNativeCatalogCallback(void (*callback)(int));

@interface HSNativeCatalogFixture : NSObject
@property(nonatomic) long count;
@property(nonatomic) double weight;
@property(nonatomic, copy, getter=displayLabel, setter=setDisplayLabel:) NSString *label;
- (double)score:(double)factor count:(NSUInteger)count;
- (HSNativeCatalogPair)pair;
- (unsigned long)scalarCount:(unsigned long)value;
+ (NSRange)rangeForCount:(NSUInteger)count;
@end

@interface HSNativeCatalogDerived : HSNativeCatalogFixture <NSCopying>
- (long)derivedValue;
@end
