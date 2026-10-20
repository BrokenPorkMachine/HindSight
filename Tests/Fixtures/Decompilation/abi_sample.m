// Original fixture; released into the public domain. Analyzed, never executed.
#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdio.h>

static volatile long argument_sink;
extern void *known_memset(void *, int, unsigned long) __asm__("_memset");

@protocol ABIParent
- (NSInteger)parentValue;
@end

@protocol ABIRecovered <ABIParent>
- (int)castValue:(signed char)value;
- (id<ABIParent>)echoParent:(id<ABIParent>)value;
@optional
+ (id)makeOptional;
- (double)optionalRatio:(float)ratio;
@end

@protocol ABICategory
- (int)categoryValue;
@end

@interface ABISample : NSObject <ABIRecovered>
- (void *)pointer;
- (double)scalar;
- (float)single;
- (NSRange)range:(NSRange)value;
- (CGRect)rectangle:(CGRect)value;
- (CGAffineTransform)affine:(CGAffineTransform)value;
- (double)manyFP:(double)a b:(double)b c:(double)c d:(double)d e:(double)e
               f:(double)f g:(double)g h:(double)h i:(double)i tail:(int)tail;
- (void)packed:(long)a b:(long)b c:(long)c d:(long)d e:(long)e f:(long)f
           byte:(char)byte half:(short)half word:(int)word pointer:(double *)pointer;
@end

@implementation ABISample
- (NSInteger)parentValue { return 3; }
- (int)castValue:(signed char)value { return value; }
- (id<ABIParent>)echoParent:(id<ABIParent>)value { return value; }
- (void *)pointer { return (void *)0x1234; }
- (double)scalar { return 1.5; }
- (float)single { return 2.5f; }
- (NSRange)range:(NSRange)value { return value; }
- (CGRect)rectangle:(CGRect)value { return value; }
- (CGAffineTransform)affine:(CGAffineTransform)value { return value; }
- (double)manyFP:(double)a b:(double)b c:(double)c d:(double)d e:(double)e
               f:(double)f g:(double)g h:(double)h i:(double)i tail:(int)tail {
    return i + tail;
}
- (void)packed:(long)a b:(long)b c:(long)c d:(long)d e:(long)e f:(long)f
           byte:(char)byte half:(short)half word:(int)word pointer:(double *)pointer {
    argument_sink = a + b + c + d + e + f + byte + half + word + (long)pointer;
}
@end

@interface ABIChild : ABISample
@end

@implementation ABIChild
@end


@interface ABISample (RecoveredCategory) <ABICategory>
@end

@implementation ABISample (RecoveredCategory)
- (int)categoryValue { return 9; }
@end

int main(void) {
    @autoreleasepool {
        ABISample *sample = [ABIChild new];
        char buffer[8];
        known_memset(buffer, -1, sizeof(buffer));
        printf("%p %f\n", [sample pointer], [sample scalar]);
        return 0;
    }
}
