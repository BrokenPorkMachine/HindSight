// Original test fixture; released into the public domain.
#import <Foundation/Foundation.h>

@interface RoadmapScorer : NSObject {
    NSInteger x0;
}
- (NSInteger)score:(NSInteger)value;
- (NSInteger)increment:(NSInteger)value;
- (NSInteger)combine:(NSInteger)first :(NSInteger)second;
- (NSString *)parentDescription;
- (NSInteger)storedValue;
@end

@implementation RoadmapScorer
- (NSInteger)score:(NSInteger)value {
    if (value < 0) return -value;
    return value * 3 + 1;
}
- (NSInteger)increment:(NSInteger)value {
    return value + 1;
}
- (NSInteger)combine:(NSInteger)first :(NSInteger)second {
    return first + second;
}
- (NSString *)parentDescription {
    return [super description];
}
- (NSInteger)storedValue {
    return x0;
}
@end

int main(void) {
    @autoreleasepool {
        id scorer = [RoadmapScorer new];
        return (int)([scorer score:7] + [scorer combine:1 :2]);
    }
}
