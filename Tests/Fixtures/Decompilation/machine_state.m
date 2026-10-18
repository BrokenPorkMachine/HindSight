/* CC0 / public-domain qualification source. Native runtime supplies exact IMP. */
__attribute__((objc_root_class)) @interface HSMachineStateFixture
- (unsigned long)mix:(unsigned long)a with:(unsigned long)b;
@end
@implementation HSMachineStateFixture
- (unsigned long)mix:(unsigned long)a with:(unsigned long)b { return (a^b)+3; }
@end
