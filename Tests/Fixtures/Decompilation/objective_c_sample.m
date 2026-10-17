// Original test fixture; released into the public domain.
#import <Foundation/Foundation.h>

@interface RoadmapScorer : NSObject
- (NSInteger)score:(NSInteger)value;
- (NSInteger)increment:(NSInteger)value;
@end

@implementation RoadmapScorer
- (NSInteger)score:(NSInteger)value {
    if (value < 0) return -value;
    return value * 3 + 1;
}
- (NSInteger)increment:(NSInteger)value {
    return value + 1;
}
@end

int main(void) {
    @autoreleasepool {
        id scorer = [RoadmapScorer new];
        return (int)[scorer score:7];
    }
}
