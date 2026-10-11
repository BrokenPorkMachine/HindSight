use iphem_core::sysreg::{key, min_el, SysReg};

#[test]
fn generic_timer_system_register_keys_decode_with_correct_access_levels() {
    assert_eq!(key(0xdf10), Some(SysReg::CntpTvalEl0));
    assert_eq!(key(0xdf11), Some(SysReg::CntpCtlEl0));
    assert_eq!(key(0xdf12), Some(SysReg::CntpCvalEl0));
    assert_eq!(key(0xdf18), Some(SysReg::CntvTvalEl0));
    assert_eq!(key(0xdf19), Some(SysReg::CntvCtlEl0));
    assert_eq!(key(0xdf1a), Some(SysReg::CntvCvalEl0));
    assert_eq!(key(0xe703), Some(SysReg::CntvoffEl2));
    assert_eq!(min_el(SysReg::CntpCtlEl0), 0);
    assert_eq!(min_el(SysReg::CntvoffEl2), 2);
}

/// CNTPCTSS_EL0 (S3_3_C2_C3_0, key 0xdf03) is the Secure physical-counter
/// view; it shares the same global counter as CNTPCT_EL0 but is readable at
/// EL0 without the EL2 offset applied.
#[test]
fn cntpctss_el0_decodes_and_is_el0_readable() {
    assert_eq!(key(0xdf03), Some(SysReg::CntpctssEl0));
    assert_eq!(min_el(SysReg::CntpctssEl0), 0);
}

/// CNTP_OFF_EL2 (S3_4_C2_C3_0, key 0xe320) is the EL2 physical-counter
/// offset. It is EL2-or-higher and writable.
#[test]
fn cntpoff_el2_decodes_and_is_el2_writable() {
    assert_eq!(key(0xe320), Some(SysReg::CntpoffEl2));
    assert_eq!(min_el(SysReg::CntpoffEl2), 2);
}
