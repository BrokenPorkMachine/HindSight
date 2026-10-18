//! Target-local Objective-C member evidence, separate from host runtime facts.

#[derive(Clone, Debug)]
pub struct IvarLayout {
    pub record: u64,
    pub offset_pointer: u64,
    pub name: String,
    pub encoding: String,
    pub offset: u32,
    pub size: u32,
    pub alignment: u32,
}

#[derive(Clone, Debug)]
pub struct PropertyLayout {
    pub record: u64,
    pub name: String,
    pub attributes: String,
    pub encoding: String,
    pub getter: String,
    pub setter: Option<String>,
    pub ivar: Option<String>,
    pub readonly: bool,
    pub nonatomic: bool,
    pub ownership: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct MethodLayout {
    pub implementation: u64,
    pub class_method: bool,
}

#[derive(Clone, Debug)]
pub struct ClassLayout {
    pub address: u64,
    pub name: String,
    pub instance_start: u32,
    pub instance_size: u32,
    pub ivars: Vec<IvarLayout>,
    pub properties: Vec<PropertyLayout>,
    pub methods: Vec<MethodLayout>,
    pub ivars_complete: bool,
    pub properties_complete: bool,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
        && value.bytes().next().is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

impl ClassLayout {
    pub fn member(&self, offset: u64, bytes: u8, floating: bool) -> Option<&IvarLayout> {
        if !self.ivars_complete { return None; }
        let end = offset.checked_add(u64::from(bytes))?;
        if offset < u64::from(self.instance_start) || end > u64::from(self.instance_size) { return None; }
        let overlapping = self.ivars.iter().filter(|ivar| {
            u64::from(ivar.offset) < end && offset < u64::from(ivar.offset) + u64::from(ivar.size)
        }).collect::<Vec<_>>();
        if overlapping.len() != 1 { return None; }
        let member = overlapping[0];
        let encoding = member.encoding.trim_start_matches(['r', 'n', 'N', 'o', 'O', 'R', 'V']);
        if u64::from(member.offset) != offset || member.size != u32::from(bytes)
            || !identifier(&member.name) || matches!(encoding.as_bytes().first(), Some(b'{' | b'[' | b'(' | b'b'))
            || matches!(encoding, "f" | "d") != floating { return None; }
        Some(member)
    }
}

impl PropertyLayout {
    pub fn parse(record: u64, name: String, attributes: String) -> Option<Self> {
        if !identifier(&name) || attributes.len() > 2048 || !attributes.is_ascii() || attributes.bytes().any(|byte| byte.is_ascii_control()) { return None; }
        // Split attributes only outside quoted names and aggregate encodings.
        let mut fields = std::collections::BTreeMap::new();
        let mut quoted = false;
        let mut nesting = 0usize;
        let mut begin = 0;
        for (index, byte) in attributes.bytes().chain(std::iter::once(b',')).enumerate() {
            match byte {
                b'"' => quoted = !quoted,
                b'{' | b'[' | b'(' if !quoted => nesting = nesting.checked_add(1)?,
                b'}' | b']' | b')' if !quoted => nesting = nesting.checked_sub(1)?,
                b',' if !quoted && nesting == 0 => {
                    let field = attributes.get(begin..index)?;
                    let key = *field.as_bytes().first()?;
                    if fields.insert(key, field.get(1..)?.to_owned()).is_some() { return None; }
                    begin = index + 1;
                }
                _ => {}
            }
            if nesting > 8 { return None; }
        }
        if quoted || nesting != 0 || begin != attributes.len() + 1 { return None; }
        let encoding = fields.get(&b'T')?.clone();
        let readonly = fields.contains_key(&b'R');
        let getter = fields.get(&b'G').cloned().unwrap_or_else(|| name.clone());
        if !identifier(&getter) { return None; }
        let setter = if readonly { None } else {
            let conventional = format!("set{}{}:", name[..1].to_ascii_uppercase(), &name[1..]);
            let setter = fields.get(&b'S').cloned().unwrap_or(conventional);
            if !setter.ends_with(':') || !identifier(setter.strip_suffix(':')?) { return None; }
            Some(setter)
        };
        let ivar = fields.get(&b'V').cloned();
        if ivar.as_ref().is_some_and(|name| !identifier(name)) { return None; }
        let ownerships = [(b'C', "copy"), (b'&', "strong"), (b'W', "weak")];
        let ownership = ownerships.iter().filter(|(key, _)| fields.contains_key(key)).map(|(_, value)| *value).collect::<Vec<_>>();
        if ownership.len() > 1 { return None; }
        Some(Self { record, name, attributes: attributes.clone(), encoding, getter, setter, ivar,
            readonly, nonatomic: fields.contains_key(&b'N'), ownership: ownership.first().copied() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_attributes_preserve_custom_accessors_ownership_and_ivar() {
        let property = PropertyLayout::parse(0x2000, "label".into(), "T@\"NSString\",C,N,GdisplayLabel,SsetDisplayLabel:,V_label".into()).unwrap();
        assert_eq!(property.encoding, "@\"NSString\"");
        assert_eq!(property.getter, "displayLabel");
        assert_eq!(property.setter.as_deref(), Some("setDisplayLabel:"));
        assert_eq!(property.ivar.as_deref(), Some("_label"));
        assert_eq!(property.ownership, Some("copy"));
        assert!(property.nonatomic);
        assert!(!property.readonly);
        assert!(PropertyLayout::parse(0, "value".into(), "Ti,R".into()).unwrap().setter.is_none());
        for attributes in ["Ti,Ti", "Ti,C,&", "Ti,Gbad:name", "Ti,Sbroken", "T@\"broken", "T{bad=i", "Ti,Vbad-name"] {
            assert!(PropertyLayout::parse(0, "value".into(), attributes.into()).is_none(), "{attributes}");
        }
    }

    #[test]
    fn member_recovery_rejects_wrong_width_bank_interior_and_overlapping_fields() {
        let field = IvarLayout { record: 0x2000, offset_pointer: 0x3000, name: "_weight".into(), encoding: "d".into(), offset: 8, size: 8, alignment: 8 };
        let mut class = ClassLayout { address: 0x1000, name: "Widget".into(), instance_start: 8, instance_size: 16,
            ivars: vec![field.clone()], properties: vec![], methods:vec![], ivars_complete:true, properties_complete:true };
        assert!(class.member(8, 8, true).is_some());
        for (offset, width, floating) in [(8, 4, true), (8, 8, false), (12, 4, true), (16, 8, true)] {
            assert!(class.member(offset, width, floating).is_none());
        }
        class.ivars.push(field);
        assert!(class.member(8, 8, true).is_none());
    }
}
