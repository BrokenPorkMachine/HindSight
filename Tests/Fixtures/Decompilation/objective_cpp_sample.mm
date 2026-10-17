// Original test fixture; released into the public domain.
#import <Foundation/Foundation.h>

class NativeScore {
public:
    int score(int value) const { return value > 8 ? value - 3 : value + 4; }
};

@interface MixedScorer : NSObject
- (NSInteger)score:(NSInteger)value;
@end

@implementation MixedScorer
- (NSInteger)score:(NSInteger)value {
    NativeScore native;
    return native.score((int)value);
}
@end

int main(void) {
    @autoreleasepool {
        id scorer = [MixedScorer new];
        return (int)[scorer score:12];
    }
}
