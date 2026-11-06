// Original HindSight qualification fixture; CC0 / public domain.
unsigned long hs_copy_chain(unsigned long original);
unsigned long hs_frame_spill(unsigned long original);
unsigned long hs_pair_spill(unsigned long first, unsigned long second);
double hs_fp_frame(double original);
unsigned long hs_preindexed(const unsigned long *owner);
unsigned long hs_load_overlap(const unsigned long *owner);
unsigned long hs_csel_same(unsigned long original);
double hs_fcsel_same(double original);
unsigned long hs_xor_zero(unsigned long original);
unsigned long hs_bic_zero(unsigned long original);
unsigned long hs_w_overwrite(unsigned long original);
unsigned long hs_add_sub_zero(unsigned long original);
unsigned long hs_shifted_zero_ops(unsigned long original);
unsigned long hs_zero_distance_shift(unsigned long original);
unsigned int hs_w_add_sub_zero(unsigned int original);
unsigned long hs_call_preserved(unsigned long original);
void hs_call_clobber(void);
double hs_call_preserved_fp(double original);
